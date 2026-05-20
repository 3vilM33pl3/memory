use crate::prelude::*;
use crate::*;

pub(crate) async fn verify_provenance(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ProvenanceVerificationRequest>,
) -> Result<Json<ProvenanceVerificationResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/provenance/verify", &request, true).await?,
        ));
    }
    verify_project_provenance(state.pool()?, &request)
        .await
        .map(Json)
        .map_err(ApiError::sql)
}

pub(crate) async fn verify_project_provenance(
    pool: &PgPool,
    request: &ProvenanceVerificationRequest,
) -> Result<ProvenanceVerificationResponse, sqlx::Error> {
    let project_row = sqlx::query("SELECT id, root_path FROM projects WHERE slug = $1")
        .bind(&request.project)
        .fetch_optional(pool)
        .await?;
    let Some(project_row) = project_row else {
        return Ok(ProvenanceVerificationResponse {
            project: request.project.clone(),
            repo_root: request.repo_root.clone().unwrap_or_default(),
            dry_run: request.dry_run,
            checked_at: chrono::Utc::now(),
            checked_count: 0,
            verified_count: 0,
            missing_file_count: 0,
            missing_symbol_count: 0,
            unverifiable_count: 0,
            stale_count: 0,
            stored_count: 0,
            warnings: vec![DiagnosticInfo {
                code: "project_not_found".to_string(),
                source: "memory".to_string(),
                component: "service".to_string(),
                operation: "verify_provenance".to_string(),
                severity: DiagnosticSeverity::Warning,
                message: format!("Project `{}` was not found.", request.project),
                raw_error: None,
                explanation: None,
                fix_hint: Some(
                    "Create or initialize the project before verifying provenance.".to_string(),
                ),
                doctor_hint: None,
                command_hint: Some(format!("memory init --project {}", request.project)),
            }],
            items: Vec::new(),
        });
    };
    let project_id: Uuid = project_row.try_get("id")?;
    let repo_root = request
        .repo_root
        .clone()
        .unwrap_or_else(|| project_row.try_get("root_path").unwrap_or_default());
    let checked_at = chrono::Utc::now();
    let rows = sqlx::query(
        r#"
        SELECT ms.id AS source_id, m.id AS memory_id, m.summary, ms.file_path,
               ms.symbol_name, ms.symbol_kind, ms.source_kind
        FROM memory_sources ms
        JOIN memory_entries m ON m.id = ms.memory_entry_id
        WHERE m.project_id = $1
          AND m.status = 'active'
          AND COALESCE(m.is_tombstone, false) = false
        ORDER BY m.updated_at DESC, ms.created_at ASC
        "#,
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let source_id: Uuid = row.try_get("source_id")?;
        let memory_id: Uuid = row.try_get("memory_id")?;
        let memory_summary: String = row.try_get("summary")?;
        let file_path: Option<String> = row.try_get("file_path")?;
        let symbol_name: Option<String> = row.try_get("symbol_name")?;
        let symbol_kind: Option<String> = row.try_get("symbol_kind")?;
        let source_kind = parse_source_kind(&row.try_get::<String, _>("source_kind")?);
        let mut verification = verify_source_path(
            source_id,
            memory_id,
            memory_summary,
            source_kind,
            file_path,
            symbol_name,
            symbol_kind,
            &repo_root,
        );
        verify_source_symbol(pool, project_id, &repo_root, &mut verification).await?;
        items.push(verification);
    }

    let mut stored_count = 0;
    if !request.dry_run {
        for item in &items {
            sqlx::query(
                r#"
                INSERT INTO memory_source_verifications
                    (source_id, status, checked_at, reason, resolved_path)
                VALUES ($1, $2, $3, $4, $5)
                ON CONFLICT (source_id) DO UPDATE SET
                    status = EXCLUDED.status,
                    checked_at = EXCLUDED.checked_at,
                    reason = EXCLUDED.reason,
                    resolved_path = EXCLUDED.resolved_path
                "#,
            )
            .bind(item.source_id)
            .bind(item.status.as_str())
            .bind(checked_at)
            .bind(&item.reason)
            .bind(&item.resolved_path)
            .execute(pool)
            .await?;
            stored_count += 1;
        }
    }

    let verified_count = items
        .iter()
        .filter(|item| item.status == SourceProvenanceStatus::Verified)
        .count();
    let missing_file_count = items
        .iter()
        .filter(|item| item.status == SourceProvenanceStatus::MissingFile)
        .count();
    let missing_symbol_count = items
        .iter()
        .filter(|item| item.status == SourceProvenanceStatus::MissingSymbol)
        .count();
    let unverifiable_count = items
        .iter()
        .filter(|item| item.status == SourceProvenanceStatus::Unverifiable)
        .count();
    let stale_count = items
        .iter()
        .filter(|item| item.status == SourceProvenanceStatus::Stale)
        .count();
    let warnings = items
        .iter()
        .filter(|item| {
            matches!(
                item.status,
                SourceProvenanceStatus::MissingFile
                    | SourceProvenanceStatus::MissingSymbol
                    | SourceProvenanceStatus::Stale
            )
        })
        .map(|item| DiagnosticInfo {
            code: "stale_memory_provenance".to_string(),
            source: "memory".to_string(),
            component: "service".to_string(),
            operation: "verify_provenance".to_string(),
            severity: DiagnosticSeverity::Warning,
            message: format!(
                "Memory {} cites {} with provenance status {}",
                item.memory_id,
                item.file_path.as_deref().unwrap_or("<unknown source path>"),
                item.status.as_str()
            ),
            raw_error: None,
            explanation: item.reason.clone(),
            fix_hint: Some("Review the cited path and update or replace the memory.".to_string()),
            doctor_hint: None,
            command_hint: Some(format!(
                "memory verify-provenance --project {} --repo-root {}",
                request.project, repo_root
            )),
        })
        .collect();

    Ok(ProvenanceVerificationResponse {
        project: request.project.clone(),
        repo_root,
        dry_run: request.dry_run,
        checked_at,
        checked_count: items.len(),
        verified_count,
        missing_file_count,
        missing_symbol_count,
        unverifiable_count,
        stale_count,
        stored_count,
        warnings,
        items,
    })
}

pub(crate) async fn run_provenance_reverify_scheduler(state: AppState) -> Result<()> {
    tokio::time::sleep(StdDuration::from_secs(10)).await;
    let interval = state
        .config
        .provenance
        .reverify_interval
        .max(StdDuration::from_secs(60));
    loop {
        if state.is_primary()
            && let Err(error) = reverify_all_projects_once(&state).await
        {
            let mut runtime = state
                .provenance
                .lock()
                .expect("provenance runtime mutex poisoned");
            runtime.status = "error".to_string();
            runtime.error = Some(error.to_string());
        }
        tokio::time::sleep(interval).await;
    }
}

pub(crate) async fn reverify_all_projects_once(state: &AppState) -> Result<()> {
    let Some(pool) = state.pool.clone() else {
        return Ok(());
    };
    let projects = sqlx::query(
        r#"
        SELECT DISTINCT p.slug, p.root_path
        FROM projects p
        JOIN memory_entries m ON m.project_id = p.id
        JOIN memory_sources ms ON ms.memory_entry_id = m.id
        WHERE m.status = 'active'
          AND COALESCE(m.is_tombstone, false) = false
        ORDER BY p.slug
        "#,
    )
    .fetch_all(&pool)
    .await
    .context("list projects for provenance reverification")?;

    {
        let mut runtime = state
            .provenance
            .lock()
            .expect("provenance runtime mutex poisoned");
        runtime.status = "running".to_string();
        runtime.last_started_at = Some(chrono::Utc::now());
        runtime.last_finished_at = None;
        runtime.last_project = None;
        runtime.checked_count = 0;
        runtime.stale_count = 0;
        runtime.error = None;
    }

    let mut checked_count = 0;
    let mut stale_count = 0;
    let mut last_project = None;
    for row in projects {
        let project: String = row.try_get("slug")?;
        let repo_root: String = row.try_get("root_path")?;
        {
            let mut runtime = state
                .provenance
                .lock()
                .expect("provenance runtime mutex poisoned");
            runtime.last_project = Some(project.clone());
        }
        let response = verify_project_provenance(
            &pool,
            &ProvenanceVerificationRequest {
                project: project.clone(),
                repo_root: Some(repo_root),
                dry_run: false,
            },
        )
        .await
        .with_context(|| format!("verify provenance for project {project}"))?;
        checked_count += response.checked_count;
        stale_count +=
            response.missing_file_count + response.missing_symbol_count + response.stale_count;
        last_project = Some(project);
    }

    let mut runtime = state
        .provenance
        .lock()
        .expect("provenance runtime mutex poisoned");
    runtime.status = "ok".to_string();
    runtime.last_finished_at = Some(chrono::Utc::now());
    runtime.last_project = last_project;
    runtime.checked_count = checked_count;
    runtime.stale_count = stale_count;
    runtime.error = None;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn verify_source_path(
    source_id: Uuid,
    memory_id: Uuid,
    memory_summary: String,
    source_kind: SourceKind,
    file_path: Option<String>,
    symbol_name: Option<String>,
    symbol_kind: Option<String>,
    repo_root: &str,
) -> SourceProvenanceVerification {
    let mut resolved_path = None;
    let (status, reason) = match (&source_kind, file_path.as_deref()) {
        (SourceKind::File, Some(path)) if !path.trim().is_empty() => {
            let source_path = FsPath::new(path);
            if !source_path.is_absolute() && repo_root.trim().is_empty() {
                return SourceProvenanceVerification {
                    source_id,
                    memory_id,
                    memory_summary,
                    source_kind,
                    file_path,
                    symbol_name,
                    symbol_kind,
                    status: SourceProvenanceStatus::Unverifiable,
                    reason: Some(
                        "relative file source cannot be verified without a repo root".to_string(),
                    ),
                    resolved_path: None,
                };
            }
            let resolved = if source_path.is_absolute() {
                source_path.to_path_buf()
            } else {
                FsPath::new(repo_root).join(source_path)
            };
            resolved_path = Some(resolved.display().to_string());
            if resolved.exists() {
                (
                    SourceProvenanceStatus::Verified,
                    Some("file exists".to_string()),
                )
            } else {
                (
                    SourceProvenanceStatus::MissingFile,
                    Some("file source no longer exists at the resolved path".to_string()),
                )
            }
        }
        (SourceKind::File, _) => (
            SourceProvenanceStatus::Unverifiable,
            Some("file source has no file_path".to_string()),
        ),
        _ => (
            SourceProvenanceStatus::Unverifiable,
            Some(format!(
                "{} sources do not reference a file path",
                source_kind_name(&source_kind)
            )),
        ),
    };

    SourceProvenanceVerification {
        source_id,
        memory_id,
        memory_summary,
        source_kind,
        file_path,
        symbol_name,
        symbol_kind,
        status,
        reason,
        resolved_path,
    }
}

pub(crate) async fn verify_source_symbol(
    pool: &PgPool,
    project_id: Uuid,
    repo_root: &str,
    item: &mut SourceProvenanceVerification,
) -> Result<(), sqlx::Error> {
    if item.status != SourceProvenanceStatus::Verified {
        return Ok(());
    }
    let Some(symbol_name) = item
        .symbol_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    let Some(file_path) = item.file_path.as_deref() else {
        return Ok(());
    };
    let graph_file_path = graph_relative_file_path(file_path, repo_root);
    let latest_run = sqlx::query(
        r#"
        SELECT id
        FROM graph_extraction_runs
        WHERE project_id = $1
          AND status = 'completed'
        ORDER BY completed_at DESC NULLS LAST, started_at DESC, id DESC
        LIMIT 1
        "#,
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await?;
    let Some(latest_run) = latest_run else {
        item.status = SourceProvenanceStatus::Unverifiable;
        item.reason = Some(
            "symbol source cannot be verified without a completed code graph extraction"
                .to_string(),
        );
        return Ok(());
    };
    let latest_run_id: Uuid = latest_run.try_get("id")?;
    let symbol_kind = item
        .symbol_kind
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let row = sqlx::query(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM code_symbols cs
            WHERE cs.project_id = $1
              AND cs.extraction_run_id = $2
              AND cs.file_path = $3
              AND (cs.name = $4 OR cs.qualified_name = $4 OR cs.display_name = $4)
              AND ($5::text IS NULL OR cs.symbol_kind = $5)
        ) AS found
        "#,
    )
    .bind(project_id)
    .bind(latest_run_id)
    .bind(&graph_file_path)
    .bind(symbol_name)
    .bind(symbol_kind)
    .fetch_one(pool)
    .await?;
    let found: bool = row.try_get("found")?;
    if found {
        item.reason = Some(format!("file exists and symbol `{symbol_name}` is present"));
    } else {
        item.status = SourceProvenanceStatus::MissingSymbol;
        item.reason = Some(format!(
            "file exists but symbol `{symbol_name}` was not found in the latest code graph"
        ));
    }
    Ok(())
}

fn graph_relative_file_path(file_path: &str, repo_root: &str) -> String {
    let path = FsPath::new(file_path);
    if path.is_absolute()
        && let Ok(relative) = path.strip_prefix(repo_root)
    {
        return relative.display().to_string();
    }
    file_path.to_string()
}
