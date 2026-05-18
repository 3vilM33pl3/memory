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
        SELECT ms.id AS source_id, m.id AS memory_id, m.summary, ms.file_path, ms.source_kind
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
        let source_kind = parse_source_kind(&row.try_get::<String, _>("source_kind")?);
        let verification = verify_source_path(
            source_id,
            memory_id,
            memory_summary,
            source_kind,
            file_path,
            &repo_root,
        );
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

pub(crate) fn verify_source_path(
    source_id: Uuid,
    memory_id: Uuid,
    memory_summary: String,
    source_kind: SourceKind,
    file_path: Option<String>,
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
        status,
        reason,
        resolved_path,
    }
}
