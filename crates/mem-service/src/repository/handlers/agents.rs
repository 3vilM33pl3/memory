use crate::prelude::*;
use crate::*;

#[derive(Debug, Default, Deserialize)]
pub(crate) struct AgentWorkspacesQuery {
    project: Option<String>,
    include_finished: Option<bool>,
}

pub(crate) async fn list_agent_workspaces(
    State(state): State<AppState>,
    Query(query): Query<AgentWorkspacesQuery>,
) -> Result<Json<AgentWorkspaceListResponse>, ApiError> {
    let project = query
        .project
        .unwrap_or_else(|| "memory".to_string())
        .trim()
        .to_string();
    if project.is_empty() {
        return Err(ApiError::validation(ValidationError::new(
            "project must be non-empty",
        )));
    }
    if !state.is_primary() {
        let path = format!(
            "/v1/agents/workspaces?project={}&include_finished={}",
            urlencoding::encode(&project),
            query.include_finished.unwrap_or(false)
        );
        return Ok(Json(proxy_get_json(&state, &path).await?));
    }
    let response = fetch_agent_workspaces(
        &state.pool()?,
        &project,
        query.include_finished.unwrap_or(false),
    )
    .await
    .map_err(ApiError::sql)?;
    Ok(Json(response))
}

pub(crate) async fn start_agent_workspace(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<AgentWorkspaceStartRequest>,
) -> Result<Json<AgentWorkspaceRecord>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/agents/workspaces/start", &request, true).await?,
        ));
    }
    Ok(Json(
        upsert_agent_workspace_start(&state.pool()?, &request)
            .await
            .map_err(ApiError::sql)?,
    ))
}

pub(crate) async fn heartbeat_agent_workspace(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<Uuid>,
    Json(request): Json<AgentWorkspaceHeartbeatRequest>,
) -> Result<Json<AgentWorkspaceRecord>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(
                &state,
                &format!("/v1/agents/workspaces/{workspace_id}/heartbeat"),
                &request,
                true,
            )
            .await?,
        ));
    }
    update_agent_workspace_heartbeat(&state.pool()?, workspace_id, &request)
        .await
        .map_err(ApiError::sql)?
        .ok_or_else(|| ApiError::not_found("agent workspace not found"))
        .map(Json)
}

pub(crate) async fn finish_agent_workspace(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<Uuid>,
    Json(request): Json<AgentWorkspaceFinishRequest>,
) -> Result<Json<AgentWorkspaceRecord>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(
                &state,
                &format!("/v1/agents/workspaces/{workspace_id}/finish"),
                &request,
                true,
            )
            .await?,
        ));
    }
    finish_agent_workspace_record(&state.pool()?, workspace_id, &request)
        .await
        .map_err(ApiError::sql)?
        .ok_or_else(|| ApiError::not_found("agent workspace not found"))
        .map(Json)
}

pub(crate) async fn fetch_agent_workspaces(
    pool: &PgPool,
    project: &str,
    include_finished: bool,
) -> Result<AgentWorkspaceListResponse, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT
            aw.id, p.slug AS project, aw.repo_root, aw.worktree_path, aw.branch,
            aw.task, aw.base_commit, aw.head_commit, aw.dirty_files, aw.agent_cli,
            aw.agent_session_id, aw.hostname, aw.writer_id, aw.profile,
            aw.service_endpoint, aw.started_at, aw.last_heartbeat_at,
            aw.finished_at, aw.status, aw.finish_summary, aw.pushed_branch,
            aw.merged_commit
        FROM agent_workspaces aw
        JOIN projects p ON p.id = aw.project_id
        WHERE p.slug = $1
          AND ($2 OR aw.status = 'active')
        ORDER BY aw.status ASC, aw.last_heartbeat_at DESC, aw.started_at DESC
        "#,
    )
    .bind(project)
    .bind(include_finished)
    .fetch_all(pool)
    .await?;

    let mut workspaces = rows
        .into_iter()
        .map(row_to_agent_workspace)
        .collect::<Result<Vec<_>, _>>()?;
    annotate_workspace_warnings(&mut workspaces);
    let warnings = aggregate_workspace_warnings(&workspaces);
    Ok(AgentWorkspaceListResponse {
        project: project.to_string(),
        workspaces,
        warnings,
    })
}

pub(crate) async fn upsert_agent_workspace_start(
    pool: &PgPool,
    request: &AgentWorkspaceStartRequest,
) -> Result<AgentWorkspaceRecord, sqlx::Error> {
    let project_id = crate::repository::handlers::bundle::upsert_project_slug(pool, &request.project).await?;
    let workspace_id = Uuid::new_v4();
    let session_key = agent_session_key(request.agent_session_id.as_deref());
    let row = sqlx::query(
        r#"
        INSERT INTO agent_workspaces (
            id, project_id, repo_root, worktree_path, branch, task, base_commit,
            head_commit, dirty_files, agent_cli, agent_session_id,
            agent_session_key, hostname, writer_id, profile, service_endpoint,
            status, started_at, last_heartbeat_at, updated_at
        )
        VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
            $13, $14, $15, $16, 'active', now(), now(), now()
        )
        ON CONFLICT (project_id, repo_root, branch, agent_session_key)
        DO UPDATE SET
            worktree_path = EXCLUDED.worktree_path,
            task = EXCLUDED.task,
            base_commit = COALESCE(EXCLUDED.base_commit, agent_workspaces.base_commit),
            head_commit = EXCLUDED.head_commit,
            dirty_files = EXCLUDED.dirty_files,
            agent_cli = EXCLUDED.agent_cli,
            hostname = EXCLUDED.hostname,
            writer_id = EXCLUDED.writer_id,
            profile = EXCLUDED.profile,
            service_endpoint = EXCLUDED.service_endpoint,
            status = 'active',
            finished_at = NULL,
            finish_summary = NULL,
            pushed_branch = NULL,
            merged_commit = NULL,
            last_heartbeat_at = now(),
            updated_at = now()
        RETURNING
            agent_workspaces.id, $17::text AS project, repo_root, worktree_path,
            branch, task, base_commit, head_commit, dirty_files, agent_cli,
            agent_session_id, hostname, writer_id, profile, service_endpoint,
            started_at, last_heartbeat_at, finished_at, status, finish_summary,
            pushed_branch, merged_commit
        "#,
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(&request.repo_root)
    .bind(&request.worktree_path)
    .bind(&request.branch)
    .bind(normalize_optional(request.task.as_deref()))
    .bind(normalize_optional(request.base_commit.as_deref()))
    .bind(normalize_optional(request.head_commit.as_deref()))
    .bind(&request.dirty_files)
    .bind(normalize_agent_cli(&request.agent_cli))
    .bind(normalize_optional(request.agent_session_id.as_deref()))
    .bind(session_key)
    .bind(normalize_optional(request.hostname.as_deref()))
    .bind(normalize_optional(request.writer_id.as_deref()))
    .bind(normalize_optional(request.profile.as_deref()))
    .bind(normalize_optional(request.service_endpoint.as_deref()))
    .bind(&request.project)
    .fetch_one(pool)
    .await?;

    let mut workspace = row_to_agent_workspace(row)?;
    annotate_workspace_warnings(std::slice::from_mut(&mut workspace));
    Ok(workspace)
}

pub(crate) async fn update_agent_workspace_heartbeat(
    pool: &PgPool,
    workspace_id: Uuid,
    request: &AgentWorkspaceHeartbeatRequest,
) -> Result<Option<AgentWorkspaceRecord>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        UPDATE agent_workspaces aw
        SET head_commit = COALESCE($2, head_commit),
            dirty_files = $3,
            service_endpoint = COALESCE($4, service_endpoint),
            last_heartbeat_at = now(),
            updated_at = now()
        FROM projects p
        WHERE aw.project_id = p.id
          AND aw.id = $1
        RETURNING
            aw.id, p.slug AS project, aw.repo_root, aw.worktree_path, aw.branch,
            aw.task, aw.base_commit, aw.head_commit, aw.dirty_files, aw.agent_cli,
            aw.agent_session_id, aw.hostname, aw.writer_id, aw.profile,
            aw.service_endpoint, aw.started_at, aw.last_heartbeat_at,
            aw.finished_at, aw.status, aw.finish_summary, aw.pushed_branch,
            aw.merged_commit
        "#,
    )
    .bind(workspace_id)
    .bind(normalize_optional(request.head_commit.as_deref()))
    .bind(&request.dirty_files)
    .bind(normalize_optional(request.service_endpoint.as_deref()))
    .fetch_optional(pool)
    .await?;
    row.map(row_to_agent_workspace).transpose()
}

pub(crate) async fn finish_agent_workspace_record(
    pool: &PgPool,
    workspace_id: Uuid,
    request: &AgentWorkspaceFinishRequest,
) -> Result<Option<AgentWorkspaceRecord>, sqlx::Error> {
    let status = request
        .status
        .clone()
        .unwrap_or(AgentWorkspaceStatus::Completed)
        .to_string();
    let row = sqlx::query(
        r#"
        UPDATE agent_workspaces aw
        SET status = $2,
            head_commit = COALESCE($3, head_commit),
            dirty_files = $4,
            finish_summary = $5,
            pushed_branch = $6,
            merged_commit = $7,
            finished_at = now(),
            last_heartbeat_at = now(),
            updated_at = now()
        FROM projects p
        WHERE aw.project_id = p.id
          AND aw.id = $1
        RETURNING
            aw.id, p.slug AS project, aw.repo_root, aw.worktree_path, aw.branch,
            aw.task, aw.base_commit, aw.head_commit, aw.dirty_files, aw.agent_cli,
            aw.agent_session_id, aw.hostname, aw.writer_id, aw.profile,
            aw.service_endpoint, aw.started_at, aw.last_heartbeat_at,
            aw.finished_at, aw.status, aw.finish_summary, aw.pushed_branch,
            aw.merged_commit
        "#,
    )
    .bind(workspace_id)
    .bind(status)
    .bind(normalize_optional(request.head_commit.as_deref()))
    .bind(&request.dirty_files)
    .bind(normalize_optional(request.finish_summary.as_deref()))
    .bind(request.pushed_branch)
    .bind(normalize_optional(request.merged_commit.as_deref()))
    .fetch_optional(pool)
    .await?;
    row.map(row_to_agent_workspace).transpose()
}

fn row_to_agent_workspace(row: sqlx::postgres::PgRow) -> Result<AgentWorkspaceRecord, sqlx::Error> {
    let dirty_files: Vec<String> = row.try_get("dirty_files")?;
    Ok(AgentWorkspaceRecord {
        id: row.try_get("id")?,
        project: row.try_get("project")?,
        repo_root: row.try_get("repo_root")?,
        worktree_path: row.try_get("worktree_path")?,
        branch: row.try_get("branch")?,
        task: row.try_get("task")?,
        base_commit: row.try_get("base_commit")?,
        head_commit: row.try_get("head_commit")?,
        dirty_count: dirty_files.len(),
        dirty_files,
        agent_cli: row.try_get("agent_cli")?,
        agent_session_id: row.try_get("agent_session_id")?,
        hostname: row.try_get("hostname")?,
        writer_id: row.try_get("writer_id")?,
        profile: row.try_get("profile")?,
        service_endpoint: row.try_get("service_endpoint")?,
        started_at: row.try_get("started_at")?,
        last_heartbeat_at: row.try_get("last_heartbeat_at")?,
        finished_at: row.try_get("finished_at")?,
        status: parse_workspace_status(&row.try_get::<String, _>("status")?),
        finish_summary: row.try_get("finish_summary")?,
        pushed_branch: row.try_get("pushed_branch")?,
        merged_commit: row.try_get("merged_commit")?,
        warnings: Vec::new(),
    })
}

fn annotate_workspace_warnings(workspaces: &mut [AgentWorkspaceRecord]) {
    let active_indexes = workspaces
        .iter()
        .enumerate()
        .filter_map(|(index, workspace)| (workspace.status == AgentWorkspaceStatus::Active).then_some(index))
        .collect::<Vec<_>>();

    for index in &active_indexes {
        if workspaces[*index].dirty_count > 0 {
            workspaces[*index].warnings.push(workspace_warning(
                "dirty_workspace",
                DiagnosticSeverity::Warning,
                format!(
                    "{} has {} dirty file(s)",
                    workspaces[*index].branch, workspaces[*index].dirty_count
                ),
            ));
        }
        if is_stale(workspaces[*index].last_heartbeat_at) {
            workspaces[*index].warnings.push(workspace_warning(
                "stale_heartbeat",
                DiagnosticSeverity::Warning,
                format!("{} has not heartbeated recently", workspaces[*index].branch),
            ));
        }
    }

    for left_pos in 0..active_indexes.len() {
        for right_pos in (left_pos + 1)..active_indexes.len() {
            let left = active_indexes[left_pos];
            let right = active_indexes[right_pos];
            if workspaces[left].branch == workspaces[right].branch {
                let message = format!(
                    "Multiple active agents are on branch {}",
                    workspaces[left].branch
                );
                push_pair_warning(workspaces, left, right, "same_branch", message);
            }
            if workspaces[left].worktree_path == workspaces[right].worktree_path {
                let message = format!(
                    "Multiple active agents share worktree {}",
                    workspaces[left].worktree_path
                );
                push_pair_warning(workspaces, left, right, "same_worktree", message);
            }
            let overlap = dirty_overlap(&workspaces[left], &workspaces[right]);
            if !overlap.is_empty() {
                let message = format!(
                    "Dirty file overlap with another active workspace: {}",
                    overlap.join(", ")
                );
                push_pair_warning(workspaces, left, right, "dirty_file_overlap", message);
            }
        }
    }
}

fn aggregate_workspace_warnings(workspaces: &[AgentWorkspaceRecord]) -> Vec<AgentWorkspaceWarning> {
    let mut seen = std::collections::BTreeSet::new();
    let mut warnings = Vec::new();
    for workspace in workspaces {
        for warning in &workspace.warnings {
            let key = format!("{}:{}", warning.code, warning.message);
            if seen.insert(key) {
                warnings.push(warning.clone());
            }
        }
    }
    warnings
}

fn push_pair_warning(
    workspaces: &mut [AgentWorkspaceRecord],
    left: usize,
    right: usize,
    code: &str,
    message: String,
) {
    let warning = workspace_warning(code, DiagnosticSeverity::Warning, message);
    workspaces[left].warnings.push(warning.clone());
    workspaces[right].warnings.push(warning);
}

fn dirty_overlap(left: &AgentWorkspaceRecord, right: &AgentWorkspaceRecord) -> Vec<String> {
    let right_files = right.dirty_files.iter().collect::<std::collections::BTreeSet<_>>();
    left.dirty_files
        .iter()
        .filter(|file| right_files.contains(file))
        .take(8)
        .cloned()
        .collect()
}

fn workspace_warning(
    code: &str,
    severity: DiagnosticSeverity,
    message: String,
) -> AgentWorkspaceWarning {
    AgentWorkspaceWarning {
        code: code.to_string(),
        severity,
        message,
    }
}

fn parse_workspace_status(value: &str) -> AgentWorkspaceStatus {
    match value {
        "completed" => AgentWorkspaceStatus::Completed,
        "abandoned" => AgentWorkspaceStatus::Abandoned,
        _ => AgentWorkspaceStatus::Active,
    }
}

fn normalize_agent_cli(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "unknown".to_string()
    } else {
        trimmed.to_string()
    }
}

fn normalize_optional(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn agent_session_key(agent_session_id: Option<&str>) -> String {
    normalize_optional(agent_session_id).unwrap_or_else(|| "manual".to_string())
}

fn is_stale(last_heartbeat_at: chrono::DateTime<chrono::Utc>) -> bool {
    chrono::Utc::now()
        .signed_duration_since(last_heartbeat_at)
        .num_minutes()
        > 30
}
