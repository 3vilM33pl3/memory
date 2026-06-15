use crate::prelude::*;
use crate::*;
use mem_api::{EffectiveLoopSettings, LoopActionKind};
use mem_loops::{
    budget_blocked, builtin_loop_definitions, evaluate_action, resolve_effective_settings,
    validate_definition,
};
use serde_json::json;

#[derive(Debug, SerdeDeserialize)]
pub(crate) struct LoopDefinitionQuery {
    pub(crate) project: Option<String>,
    pub(crate) repo_root: Option<String>,
}

#[derive(Debug, SerdeDeserialize)]
pub(crate) struct LoopRunsQuery {
    pub(crate) project: Option<String>,
    pub(crate) loop_id: Option<String>,
    pub(crate) status: Option<LoopRunStatus>,
    pub(crate) limit: Option<i64>,
}

#[derive(Debug, SerdeDeserialize)]
pub(crate) struct LoopApprovalsQuery {
    pub(crate) project: Option<String>,
    pub(crate) status: Option<LoopApprovalStatus>,
    pub(crate) limit: Option<i64>,
}

pub async fn register_builtin_loop_definitions(pool: &PgPool) -> Result<()> {
    for definition in builtin_loop_definitions() {
        validate_definition(&definition).map_err(|message| anyhow::anyhow!(message))?;
        let record = definition.to_record(chrono::Utc::now());
        sqlx::query(
            r#"
            UPDATE loop_definitions
            SET is_current = FALSE
            WHERE loop_id = $1 AND version <> $2
            "#,
        )
        .bind(&record.loop_id)
        .bind(record.version)
        .execute(pool)
        .await
        .context("mark old loop definitions inactive")?;

        sqlx::query(
            r#"
            INSERT INTO loop_definitions (
                id, loop_id, version, name, description, risk_level, default_mode,
                trigger_spec, context_spec, policy_spec, output_spec, is_current, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, TRUE, $12)
            ON CONFLICT (loop_id, version) DO UPDATE SET
                name = EXCLUDED.name,
                description = EXCLUDED.description,
                risk_level = EXCLUDED.risk_level,
                default_mode = EXCLUDED.default_mode,
                trigger_spec = EXCLUDED.trigger_spec,
                context_spec = EXCLUDED.context_spec,
                policy_spec = EXCLUDED.policy_spec,
                output_spec = EXCLUDED.output_spec,
                is_current = TRUE
            "#,
        )
        .bind(record.id)
        .bind(&record.loop_id)
        .bind(record.version)
        .bind(&record.name)
        .bind(&record.description)
        .bind(record.risk_level.as_str())
        .bind(record.default_mode.as_str())
        .bind(&record.trigger_spec)
        .bind(&record.context_spec)
        .bind(&record.policy_spec)
        .bind(&record.output_spec)
        .bind(record.created_at)
        .execute(pool)
        .await
        .context("upsert builtin loop definition")?;
    }
    Ok(())
}

pub(crate) async fn list_loop_definitions(
    State(state): State<AppState>,
    Query(query): Query<LoopDefinitionQuery>,
) -> Result<Json<LoopDefinitionsResponse>, ApiError> {
    if !state.is_primary() {
        return Ok(Json(proxy_get_json(&state, "/v1/loops").await?));
    }
    let pool = state.pool()?;
    let definitions = fetch_loop_definitions(pool).await?;
    let _ = query;
    Ok(Json(LoopDefinitionsResponse { definitions }))
}

pub(crate) async fn get_loop_definition(
    State(state): State<AppState>,
    Path(loop_id): Path<String>,
    Query(query): Query<LoopDefinitionQuery>,
) -> Result<Json<LoopDefinitionResponse>, ApiError> {
    if !state.is_primary() {
        return Ok(Json(
            proxy_get_json(&state, &format!("/v1/loops/{loop_id}")).await?,
        ));
    }
    let pool = state.pool()?;
    let definition = fetch_loop_definition(pool, &loop_id).await?;
    let effective_settings = if query.project.is_some() || query.repo_root.is_some() {
        Some(
            load_effective_loop_settings(
                pool,
                &definition,
                query.project.as_deref(),
                query.repo_root.as_deref(),
                false,
            )
            .await?,
        )
    } else {
        None
    };
    Ok(Json(LoopDefinitionResponse {
        definition,
        effective_settings,
    }))
}

pub(crate) async fn get_loop_global_state(
    State(state): State<AppState>,
) -> Result<Json<LoopGlobalStateResponse>, ApiError> {
    if !state.is_primary() {
        return Ok(Json(
            proxy_get_json(&state, "/v1/loops/global-kill-switch").await?,
        ));
    }
    Ok(Json(fetch_loop_global_state(state.pool()?).await?))
}

pub(crate) async fn update_loop_global_state(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<LoopGlobalStateUpdateRequest>,
) -> Result<Json<LoopGlobalStateResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/loops/global-kill-switch", &request, true).await?,
        ));
    }
    Ok(Json(
        store_loop_global_state(state.pool()?, &request).await?,
    ))
}

pub(crate) async fn enable_loop(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(loop_id): Path<String>,
    Json(mut request): Json<LoopSettingsUpdateRequest>,
) -> Result<Json<LoopSettingResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.enabled = Some(true);
    if request.mode.is_none() {
        let definition = fetch_loop_definition(state.pool()?, &loop_id).await?;
        request.mode = Some(definition.default_mode);
    }
    mutate_loop_setting(state, loop_id, request, true).await
}

pub(crate) async fn disable_loop(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(loop_id): Path<String>,
    Json(mut request): Json<LoopSettingsUpdateRequest>,
) -> Result<Json<LoopSettingResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.enabled = Some(false);
    request.mode = Some(LoopMode::Off);
    mutate_loop_setting(state, loop_id, request, false).await
}

pub(crate) async fn pause_loop(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(loop_id): Path<String>,
    Json(mut request): Json<LoopSettingsUpdateRequest>,
) -> Result<Json<LoopSettingResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    if request.paused_until.is_none() {
        return Err(ApiError::validation(ValidationError::new(
            "paused_until is required",
        )));
    }
    request.mode = Some(LoopMode::Paused);
    mutate_loop_setting(state, loop_id, request, false).await
}

pub(crate) async fn snooze_loop(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(loop_id): Path<String>,
    Json(mut request): Json<LoopSettingsUpdateRequest>,
) -> Result<Json<LoopSettingResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    if request.snoozed_until.is_none() {
        return Err(ApiError::validation(ValidationError::new(
            "snoozed_until is required",
        )));
    }
    request.mode = Some(LoopMode::Snoozed);
    mutate_loop_setting(state, loop_id, request, false).await
}

async fn mutate_loop_setting(
    state: AppState,
    loop_id: String,
    request: LoopSettingsUpdateRequest,
    requires_explicit_approval: bool,
) -> Result<Json<LoopSettingResponse>, ApiError> {
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        let path = if request.enabled == Some(true) {
            format!("/v1/loops/{loop_id}/enable")
        } else if request.paused_until.is_some() {
            format!("/v1/loops/{loop_id}/pause")
        } else if request.snoozed_until.is_some() {
            format!("/v1/loops/{loop_id}/snooze")
        } else {
            format!("/v1/loops/{loop_id}/disable")
        };
        return Ok(Json(proxy_post_json(&state, &path, &request, true).await?));
    }

    let pool = state.pool()?;
    let definition = fetch_loop_definition(pool, &loop_id).await?;
    let scope = resolve_loop_scope(pool, &request, true).await?;
    if requires_explicit_approval && !request.explicit_user_approval {
        let approval =
            create_loop_setting_approval(pool, &definition.loop_id, &scope, &request).await?;
        let setting =
            fetch_or_synthetic_loop_setting(pool, &definition.loop_id, &scope, chrono::Utc::now())
                .await?;
        let effective_settings = load_effective_loop_settings(
            pool,
            &definition,
            request.project.as_deref(),
            request.repo_root.as_deref(),
            false,
        )
        .await?;
        return Ok(Json(LoopSettingResponse {
            setting,
            effective_settings,
            approval: Some(approval),
        }));
    }

    let setting = upsert_loop_setting(pool, &definition.loop_id, &scope, &request).await?;
    let effective_settings = load_effective_loop_settings(
        pool,
        &definition,
        setting.project.as_deref(),
        setting.repo_root.as_deref(),
        false,
    )
    .await?;
    Ok(Json(LoopSettingResponse {
        setting,
        effective_settings,
        approval: None,
    }))
}

pub(crate) async fn run_loop(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(loop_id): Path<String>,
    Json(request): Json<LoopRunRequest>,
) -> Result<Json<LoopRunResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, &format!("/v1/loops/{loop_id}/run"), &request, true).await?,
        ));
    }
    Ok(Json(
        create_control_plane_loop_run(state.pool()?, &loop_id, &request).await?,
    ))
}

pub(crate) async fn list_loop_runs(
    State(state): State<AppState>,
    Query(query): Query<LoopRunsQuery>,
) -> Result<Json<LoopRunsResponse>, ApiError> {
    if !state.is_primary() {
        return Ok(Json(proxy_get_json(&state, "/v1/loops/runs").await?));
    }
    Ok(Json(fetch_loop_runs(state.pool()?, &query).await?))
}

pub(crate) async fn get_loop_run(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> Result<Json<LoopRunResponse>, ApiError> {
    if !state.is_primary() {
        return Ok(Json(
            proxy_get_json(&state, &format!("/v1/loops/runs/{run_id}")).await?,
        ));
    }
    Ok(Json(LoopRunResponse {
        run: fetch_loop_run_detail(state.pool()?, run_id).await?,
    }))
}

pub(crate) async fn cancel_loop_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(run_id): Path<Uuid>,
    Json(request): Json<LoopCancelRequest>,
) -> Result<Json<LoopRunResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(
                &state,
                &format!("/v1/loops/runs/{run_id}/cancel"),
                &request,
                true,
            )
            .await?,
        ));
    }
    Ok(Json(
        cancel_loop_run_record(state.pool()?, run_id, &request).await?,
    ))
}

pub(crate) async fn submit_loop_feedback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(run_id): Path<Uuid>,
    Json(request): Json<LoopFeedbackRequest>,
) -> Result<Json<LoopRunResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(
                &state,
                &format!("/v1/loops/runs/{run_id}/feedback"),
                &request,
                true,
            )
            .await?,
        ));
    }
    append_loop_trace(
        state.pool()?,
        run_id,
        "feedback",
        "User feedback",
        json!({"rating": request.rating, "note": request.note}),
        false,
    )
    .await?;
    Ok(Json(LoopRunResponse {
        run: fetch_loop_run_detail(state.pool()?, run_id).await?,
    }))
}

pub(crate) async fn list_loop_approvals(
    State(state): State<AppState>,
    Query(query): Query<LoopApprovalsQuery>,
) -> Result<Json<LoopApprovalsResponse>, ApiError> {
    if !state.is_primary() {
        return Ok(Json(proxy_get_json(&state, "/v1/loops/approvals").await?));
    }
    Ok(Json(fetch_loop_approvals(state.pool()?, &query).await?))
}

pub(crate) async fn approve_loop_approval(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(approval_id): Path<Uuid>,
    Json(request): Json<LoopApprovalDecisionRequest>,
) -> Result<Json<LoopApprovalDecisionResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(
                &state,
                &format!("/v1/loops/approvals/{approval_id}/approve"),
                &request,
                true,
            )
            .await?,
        ));
    }
    Ok(Json(
        resolve_loop_approval(
            state.pool()?,
            approval_id,
            LoopApprovalStatus::Approved,
            &request,
        )
        .await?,
    ))
}

pub(crate) async fn reject_loop_approval(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(approval_id): Path<Uuid>,
    Json(request): Json<LoopApprovalDecisionRequest>,
) -> Result<Json<LoopApprovalDecisionResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(
                &state,
                &format!("/v1/loops/approvals/{approval_id}/reject"),
                &request,
                true,
            )
            .await?,
        ));
    }
    Ok(Json(
        resolve_loop_approval(
            state.pool()?,
            approval_id,
            LoopApprovalStatus::Rejected,
            &request,
        )
        .await?,
    ))
}

pub async fn list_registered_loop_definitions(pool: &PgPool) -> Result<Vec<LoopDefinitionRecord>> {
    fetch_loop_definitions(pool)
        .await
        .map_err(|error| anyhow::anyhow!(error.message))
}

pub async fn record_control_plane_loop_run(
    pool: &PgPool,
    loop_id: &str,
    request: &LoopRunRequest,
) -> Result<LoopRunResponse> {
    create_control_plane_loop_run(pool, loop_id, request)
        .await
        .map_err(|error| anyhow::anyhow!(error.message))
}

pub async fn read_loop_run_detail(pool: &PgPool, run_id: Uuid) -> Result<LoopRunDetail> {
    fetch_loop_run_detail(pool, run_id)
        .await
        .map_err(|error| anyhow::anyhow!(error.message))
}

async fn fetch_loop_definitions(pool: &PgPool) -> Result<Vec<LoopDefinitionRecord>, ApiError> {
    let rows = sqlx::query(
        r#"
        SELECT id, loop_id, version, name, description, risk_level, default_mode,
               trigger_spec, context_spec, policy_spec, output_spec, created_at
        FROM loop_definitions
        WHERE is_current = TRUE
        ORDER BY loop_id
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(ApiError::sql)?;
    rows.into_iter().map(row_to_loop_definition).collect()
}

async fn fetch_loop_definition(
    pool: &PgPool,
    loop_id: &str,
) -> Result<LoopDefinitionRecord, ApiError> {
    let row = sqlx::query(
        r#"
        SELECT id, loop_id, version, name, description, risk_level, default_mode,
               trigger_spec, context_spec, policy_spec, output_spec, created_at
        FROM loop_definitions
        WHERE loop_id = $1 AND is_current = TRUE
        "#,
    )
    .bind(loop_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::sql)?
    .ok_or_else(|| ApiError::not_found("loop definition not found"))?;
    row_to_loop_definition(row)
}

async fn fetch_loop_global_state(pool: &PgPool) -> Result<LoopGlobalStateResponse, ApiError> {
    let row = sqlx::query(
        r#"
        SELECT kill_switch_enabled, updated_by, reason, updated_at
        FROM loop_global_state
        WHERE id = TRUE
        "#,
    )
    .fetch_one(pool)
    .await
    .map_err(ApiError::sql)?;
    Ok(LoopGlobalStateResponse {
        kill_switch_enabled: row.try_get("kill_switch_enabled").map_err(ApiError::sql)?,
        updated_by: row.try_get("updated_by").map_err(ApiError::sql)?,
        reason: row.try_get("reason").map_err(ApiError::sql)?,
        updated_at: row.try_get("updated_at").map_err(ApiError::sql)?,
    })
}

async fn store_loop_global_state(
    pool: &PgPool,
    request: &LoopGlobalStateUpdateRequest,
) -> Result<LoopGlobalStateResponse, ApiError> {
    let row = sqlx::query(
        r#"
        UPDATE loop_global_state
        SET kill_switch_enabled = $1,
            updated_by = $2,
            reason = $3,
            updated_at = now()
        WHERE id = TRUE
        RETURNING kill_switch_enabled, updated_by, reason, updated_at
        "#,
    )
    .bind(request.kill_switch_enabled)
    .bind(&request.updated_by)
    .bind(&request.reason)
    .fetch_one(pool)
    .await
    .map_err(ApiError::sql)?;
    Ok(LoopGlobalStateResponse {
        kill_switch_enabled: row.try_get("kill_switch_enabled").map_err(ApiError::sql)?,
        updated_by: row.try_get("updated_by").map_err(ApiError::sql)?,
        reason: row.try_get("reason").map_err(ApiError::sql)?,
        updated_at: row.try_get("updated_at").map_err(ApiError::sql)?,
    })
}

#[derive(Debug, Clone)]
struct ResolvedLoopScope {
    scope_type: LoopScopeType,
    scope_id: String,
    project: Option<String>,
    project_id: Option<Uuid>,
    repo_root: Option<String>,
}

async fn resolve_loop_scope(
    pool: &PgPool,
    request: &LoopSettingsUpdateRequest,
    create_project: bool,
) -> Result<ResolvedLoopScope, ApiError> {
    let scope_type = request.scope_type.clone().unwrap_or_else(|| {
        if request.repo_root.is_some() {
            LoopScopeType::Repo
        } else if request.project.is_some() {
            LoopScopeType::Project
        } else {
            LoopScopeType::User
        }
    });
    let scope_id = request
        .scope_id
        .clone()
        .or_else(|| request.repo_root.clone())
        .or_else(|| request.project.clone())
        .unwrap_or_else(|| "default".to_string());
    let project_id = match request.project.as_deref() {
        Some(project) if create_project => Some(
            upsert_project_slug(pool, project)
                .await
                .map_err(ApiError::sql)?,
        ),
        Some(project) => find_project_id(pool, project).await?,
        None => None,
    };
    Ok(ResolvedLoopScope {
        scope_type,
        scope_id,
        project: request.project.clone(),
        project_id,
        repo_root: request.repo_root.clone(),
    })
}

async fn upsert_loop_setting(
    pool: &PgPool,
    loop_id: &str,
    scope: &ResolvedLoopScope,
    request: &LoopSettingsUpdateRequest,
) -> Result<LoopSettingRecord, ApiError> {
    let row = sqlx::query(
        r#"
        INSERT INTO loop_settings (
            id, loop_id, scope_type, scope_id, project_id, repo_root, enabled, mode,
            budgets_json, approval_overrides_json, paused_until, snoozed_until,
            updated_by, reason, updated_at
        )
        VALUES (
            gen_random_uuid(), $1, $2, $3, $4, $5, $6, $7,
            $8, $9, $10, $11, $12, $13, now()
        )
        ON CONFLICT (loop_id, scope_type, scope_id) DO UPDATE SET
            project_id = EXCLUDED.project_id,
            repo_root = EXCLUDED.repo_root,
            enabled = EXCLUDED.enabled,
            mode = EXCLUDED.mode,
            budgets_json = EXCLUDED.budgets_json,
            approval_overrides_json = EXCLUDED.approval_overrides_json,
            paused_until = EXCLUDED.paused_until,
            snoozed_until = EXCLUDED.snoozed_until,
            updated_by = EXCLUDED.updated_by,
            reason = EXCLUDED.reason,
            updated_at = now()
        RETURNING
            id, loop_id, scope_type, scope_id,
            (SELECT slug FROM projects WHERE id = loop_settings.project_id) AS project,
            repo_root, enabled, mode, budgets_json, approval_overrides_json,
            paused_until, snoozed_until, updated_by, reason, updated_at
        "#,
    )
    .bind(loop_id)
    .bind(scope.scope_type.as_str())
    .bind(&scope.scope_id)
    .bind(scope.project_id)
    .bind(&scope.repo_root)
    .bind(request.enabled)
    .bind(request.mode.as_ref().map(LoopMode::as_str))
    .bind(&request.budgets)
    .bind(&request.approval_overrides)
    .bind(request.paused_until)
    .bind(request.snoozed_until)
    .bind(&request.updated_by)
    .bind(&request.reason)
    .fetch_one(pool)
    .await
    .map_err(ApiError::sql)?;
    row_to_loop_setting(row)
}

async fn fetch_or_synthetic_loop_setting(
    pool: &PgPool,
    loop_id: &str,
    scope: &ResolvedLoopScope,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<LoopSettingRecord, ApiError> {
    let row = sqlx::query(
        r#"
        SELECT
            ls.id, ls.loop_id, ls.scope_type, ls.scope_id,
            p.slug AS project, ls.repo_root, ls.enabled, ls.mode,
            ls.budgets_json, ls.approval_overrides_json, ls.paused_until,
            ls.snoozed_until, ls.updated_by, ls.reason, ls.updated_at
        FROM loop_settings ls
        LEFT JOIN projects p ON p.id = ls.project_id
        WHERE ls.loop_id = $1 AND ls.scope_type = $2 AND ls.scope_id = $3
        "#,
    )
    .bind(loop_id)
    .bind(scope.scope_type.as_str())
    .bind(&scope.scope_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::sql)?;
    if let Some(row) = row {
        return row_to_loop_setting(row);
    }
    Ok(LoopSettingRecord {
        id: Uuid::nil(),
        loop_id: loop_id.to_string(),
        scope_type: scope.scope_type.clone(),
        scope_id: scope.scope_id.clone(),
        project: scope.project.clone(),
        repo_root: scope.repo_root.clone(),
        enabled: Some(false),
        mode: Some(LoopMode::Off),
        budgets: None,
        approval_overrides: None,
        paused_until: None,
        snoozed_until: None,
        updated_by: None,
        reason: None,
        updated_at: now,
    })
}

async fn create_loop_setting_approval(
    pool: &PgPool,
    loop_id: &str,
    scope: &ResolvedLoopScope,
    request: &LoopSettingsUpdateRequest,
) -> Result<LoopApprovalRequestRecord, ApiError> {
    let row = sqlx::query(
        r#"
        INSERT INTO approval_requests (
            id, project_id, loop_id, action_type, proposed_action_json,
            risk_reason, status, requester, created_at
        )
        VALUES (
            gen_random_uuid(), $1, $2, 'enable_loop', $3,
            'Enabling a loop requires explicit user approval.', 'pending', $4, now()
        )
        RETURNING
            id, run_id, (SELECT slug FROM projects WHERE id = approval_requests.project_id) AS project,
            loop_id, action_type, proposed_action_json, risk_reason, status, requester,
            reviewer, decision_reason, created_at, resolved_at
        "#,
    )
    .bind(scope.project_id)
    .bind(loop_id)
    .bind(json!({
        "scope_type": scope.scope_type.as_str(),
        "scope_id": scope.scope_id,
        "project": scope.project,
        "repo_root": scope.repo_root,
        "request": request
    }))
    .bind(&request.updated_by)
    .fetch_one(pool)
    .await
    .map_err(ApiError::sql)?;
    row_to_loop_approval(row)
}

async fn load_effective_loop_settings(
    pool: &PgPool,
    definition: &LoopDefinitionRecord,
    project: Option<&str>,
    repo_root: Option<&str>,
    manual_run: bool,
) -> Result<EffectiveLoopSettings, ApiError> {
    let project_id = match project {
        Some(project) => find_project_id(pool, project).await?,
        None => None,
    };
    let rows = sqlx::query(
        r#"
        SELECT
            ls.id, ls.loop_id, ls.scope_type, ls.scope_id,
            p.slug AS project, ls.repo_root, ls.enabled, ls.mode,
            ls.budgets_json, ls.approval_overrides_json, ls.paused_until,
            ls.snoozed_until, ls.updated_by, ls.reason, ls.updated_at
        FROM loop_settings ls
        LEFT JOIN projects p ON p.id = ls.project_id
        WHERE ls.loop_id = $1
          AND (
            ls.scope_type IN ('user', 'workspace')
            OR ($2::uuid IS NOT NULL AND ls.project_id = $2)
            OR ($3::text IS NOT NULL AND ls.repo_root = $3)
          )
        "#,
    )
    .bind(&definition.loop_id)
    .bind(project_id)
    .bind(repo_root)
    .fetch_all(pool)
    .await
    .map_err(ApiError::sql)?;
    let settings = rows
        .into_iter()
        .map(row_to_loop_setting)
        .collect::<Result<Vec<_>, _>>()?;
    let global = fetch_loop_global_state(pool).await?;
    let mut effective = resolve_effective_settings(
        definition,
        &settings,
        global.kill_switch_enabled,
        manual_run,
        chrono::Utc::now(),
    );
    if let Some(reason) = budget_blocked(effective.budgets.as_ref()) {
        effective.blocked_reasons.push(reason);
    }
    Ok(effective)
}

async fn create_control_plane_loop_run(
    pool: &PgPool,
    loop_id: &str,
    request: &LoopRunRequest,
) -> Result<LoopRunResponse, ApiError> {
    let definition = fetch_loop_definition(pool, loop_id).await?;
    let project_id = match request.project.as_deref() {
        Some(project) => Some(
            upsert_project_slug(pool, project)
                .await
                .map_err(ApiError::sql)?,
        ),
        None => None,
    };
    let effective = load_effective_loop_settings(
        pool,
        &definition,
        request.project.as_deref(),
        request.repo_root.as_deref(),
        true,
    )
    .await?;
    let policy_decisions = [
        LoopActionKind::ReadMemory,
        LoopActionKind::ReadRepo,
        LoopActionKind::WriteMemoryProposal,
    ]
    .into_iter()
    .map(|action| evaluate_action(&effective.mode, action))
    .collect::<Vec<_>>();
    let blocked = !effective.blocked_reasons.is_empty()
        || !policy_decisions
            .iter()
            .any(|decision| decision.allowed && !decision.requires_approval);
    let status = if blocked {
        LoopRunStatus::Blocked
    } else {
        LoopRunStatus::Succeeded
    };
    let output_summary = if blocked {
        "Loop run blocked by policy or settings."
    } else {
        "Control-plane loop run recorded; real loop execution is not implemented in this slice."
    };
    let trigger_payload = request.trigger_payload.clone().unwrap_or_else(|| json!({}));
    let trigger_id = Uuid::new_v4();
    let payload_hash =
        hex_sha256(&serde_json::to_vec(&trigger_payload).map_err(|error| {
            ApiError::io(anyhow::anyhow!("serialize trigger payload: {error}"))
        })?);
    sqlx::query(
        r#"
        INSERT INTO trigger_events (
            id, source, event_type, project_id, repo_root, payload_hash,
            trust_level, payload_json, received_at
        )
        VALUES ($1, 'manual', 'manual_run', $2, $3, $4, 'high', $5, now())
        "#,
    )
    .bind(trigger_id)
    .bind(project_id)
    .bind(&request.repo_root)
    .bind(payload_hash)
    .bind(&trigger_payload)
    .execute(pool)
    .await
    .map_err(ApiError::sql)?;

    let scope_type = request.scope_type.clone().unwrap_or_else(|| {
        if request.repo_root.is_some() {
            LoopScopeType::Repo
        } else if request.project.is_some() {
            LoopScopeType::Project
        } else {
            LoopScopeType::User
        }
    });
    let scope_id = request
        .scope_id
        .clone()
        .or_else(|| request.repo_root.clone())
        .or_else(|| request.project.clone())
        .unwrap_or_else(|| "default".to_string());
    let run_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO loop_runs (
            id, loop_id, definition_id, definition_version, project_id, repo_root,
            scope_type, scope_id, trigger_event_id, mode, status, run_reason,
            started_at, finished_at, cost_json, output_summary, output_json,
            effective_settings_json, policy_decisions_json, blocked_reasons_json,
            trace_count, created_at, updated_at
        )
        VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
            now(), now(), '{}'::jsonb, $13, $14, $15, $16, $17, 0, now(), now()
        )
        "#,
    )
    .bind(run_id)
    .bind(&definition.loop_id)
    .bind(definition.id)
    .bind(definition.version)
    .bind(project_id)
    .bind(&request.repo_root)
    .bind(scope_type.as_str())
    .bind(&scope_id)
    .bind(trigger_id)
    .bind(effective.mode.as_str())
    .bind(status.as_str())
    .bind(&request.reason)
    .bind(output_summary)
    .bind(json!({
        "summary": output_summary,
        "dry_run": request.dry_run,
        "implemented": false
    }))
    .bind(
        serde_json::to_value(&effective).map_err(|error| {
            ApiError::io(anyhow::anyhow!("serialize effective settings: {error}"))
        })?,
    )
    .bind(
        serde_json::to_value(&policy_decisions).map_err(|error| {
            ApiError::io(anyhow::anyhow!("serialize policy decisions: {error}"))
        })?,
    )
    .bind(json!(effective.blocked_reasons))
    .execute(pool)
    .await
    .map_err(ApiError::sql)?;

    append_loop_trace(
        pool,
        run_id,
        "policy",
        "Policy evaluation",
        json!({ "decisions": policy_decisions }),
        false,
    )
    .await?;
    append_loop_trace(
        pool,
        run_id,
        "result",
        "Control-plane result",
        json!({ "summary": output_summary, "blocked": blocked }),
        false,
    )
    .await?;
    Ok(LoopRunResponse {
        run: fetch_loop_run_detail(pool, run_id).await?,
    })
}

async fn fetch_loop_runs(
    pool: &PgPool,
    query: &LoopRunsQuery,
) -> Result<LoopRunsResponse, ApiError> {
    let project_id = match query.project.as_deref() {
        Some(project) => find_project_id(pool, project).await?,
        None => None,
    };
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let rows = sqlx::query(
        r#"
        SELECT
            lr.id, lr.loop_id, lr.definition_version, p.slug AS project,
            lr.repo_root, lr.mode, lr.status, lr.started_at, lr.finished_at,
            lr.output_summary, lr.trace_count, lr.blocked_reasons_json
        FROM loop_runs lr
        LEFT JOIN projects p ON p.id = lr.project_id
        WHERE ($1::text IS NULL OR lr.loop_id = $1)
          AND ($2::uuid IS NULL OR lr.project_id = $2)
          AND ($3::text IS NULL OR lr.status = $3)
        ORDER BY lr.started_at DESC
        LIMIT $4
        "#,
    )
    .bind(&query.loop_id)
    .bind(project_id)
    .bind(query.status.as_ref().map(LoopRunStatus::as_str))
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(ApiError::sql)?;
    let runs = rows
        .into_iter()
        .map(|row| row_to_loop_run_summary(&row))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(LoopRunsResponse {
        total_returned: runs.len(),
        runs,
    })
}

async fn fetch_loop_run_detail(pool: &PgPool, run_id: Uuid) -> Result<LoopRunDetail, ApiError> {
    let row = sqlx::query(
        r#"
        SELECT
            lr.id, lr.loop_id, lr.definition_version, p.slug AS project,
            lr.repo_root, lr.mode, lr.status, lr.started_at, lr.finished_at,
            lr.output_summary, lr.trace_count, lr.blocked_reasons_json,
            lr.effective_settings_json, lr.policy_decisions_json, lr.cost_json, lr.output_json
        FROM loop_runs lr
        LEFT JOIN projects p ON p.id = lr.project_id
        WHERE lr.id = $1
        "#,
    )
    .bind(run_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::sql)?
    .ok_or_else(|| ApiError::not_found("loop run not found"))?;
    let summary = row_to_loop_run_summary(&row)?;
    let traces = fetch_loop_traces(pool, run_id).await?;
    Ok(LoopRunDetail {
        summary,
        effective_settings: row
            .try_get("effective_settings_json")
            .map_err(ApiError::sql)?,
        policy_decisions: row
            .try_get("policy_decisions_json")
            .map_err(ApiError::sql)?,
        cost: row.try_get("cost_json").map_err(ApiError::sql)?,
        output: row.try_get("output_json").map_err(ApiError::sql)?,
        traces,
    })
}

async fn cancel_loop_run_record(
    pool: &PgPool,
    run_id: Uuid,
    request: &LoopCancelRequest,
) -> Result<LoopRunResponse, ApiError> {
    sqlx::query(
        r#"
        UPDATE loop_runs
        SET cancel_requested_at = now(),
            status = CASE
                WHEN status IN ('queued', 'running') THEN 'cancelled'
                ELSE status
            END,
            output_json = output_json || jsonb_build_object('cancel_reason', $2::text),
            updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(run_id)
    .bind(&request.reason)
    .execute(pool)
    .await
    .map_err(ApiError::sql)?;
    append_loop_trace(
        pool,
        run_id,
        "cancel",
        "Cancel requested",
        json!({ "reason": request.reason }),
        false,
    )
    .await?;
    Ok(LoopRunResponse {
        run: fetch_loop_run_detail(pool, run_id).await?,
    })
}

async fn fetch_loop_traces(pool: &PgPool, run_id: Uuid) -> Result<Vec<LoopTraceRecord>, ApiError> {
    let rows = sqlx::query(
        r#"
        SELECT id, run_id, sequence, trace_type, title, payload_json, redacted, created_at
        FROM run_traces
        WHERE run_id = $1
        ORDER BY sequence
        "#,
    )
    .bind(run_id)
    .fetch_all(pool)
    .await
    .map_err(ApiError::sql)?;
    rows.into_iter().map(row_to_loop_trace).collect()
}

async fn append_loop_trace(
    pool: &PgPool,
    run_id: Uuid,
    trace_type: &str,
    title: &str,
    payload: serde_json::Value,
    redacted: bool,
) -> Result<LoopTraceRecord, ApiError> {
    let sequence: i32 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(sequence), 0) + 1 FROM run_traces WHERE run_id = $1",
    )
    .bind(run_id)
    .fetch_one(pool)
    .await
    .map_err(ApiError::sql)?;
    let row = sqlx::query(
        r#"
        INSERT INTO run_traces (id, run_id, sequence, trace_type, title, payload_json, redacted, created_at)
        VALUES (gen_random_uuid(), $1, $2, $3, $4, $5, $6, now())
        RETURNING id, run_id, sequence, trace_type, title, payload_json, redacted, created_at
        "#,
    )
    .bind(run_id)
    .bind(sequence)
    .bind(trace_type)
    .bind(title)
    .bind(&payload)
    .bind(redacted)
    .fetch_one(pool)
    .await
    .map_err(ApiError::sql)?;
    sqlx::query(
        "UPDATE loop_runs SET trace_count = trace_count + 1, updated_at = now() WHERE id = $1",
    )
    .bind(run_id)
    .execute(pool)
    .await
    .map_err(ApiError::sql)?;
    row_to_loop_trace(row)
}

async fn fetch_loop_approvals(
    pool: &PgPool,
    query: &LoopApprovalsQuery,
) -> Result<LoopApprovalsResponse, ApiError> {
    let project_id = match query.project.as_deref() {
        Some(project) => find_project_id(pool, project).await?,
        None => None,
    };
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let rows = sqlx::query(
        r#"
        SELECT
            ar.id, ar.run_id, p.slug AS project, ar.loop_id, ar.action_type,
            ar.proposed_action_json, ar.risk_reason, ar.status, ar.requester,
            ar.reviewer, ar.decision_reason, ar.created_at, ar.resolved_at
        FROM approval_requests ar
        LEFT JOIN projects p ON p.id = ar.project_id
        WHERE ($1::uuid IS NULL OR ar.project_id = $1)
          AND ($2::text IS NULL OR ar.status = $2)
        ORDER BY ar.created_at DESC
        LIMIT $3
        "#,
    )
    .bind(project_id)
    .bind(query.status.as_ref().map(LoopApprovalStatus::as_str))
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(ApiError::sql)?;
    let approvals = rows
        .into_iter()
        .map(row_to_loop_approval)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(LoopApprovalsResponse {
        total_returned: approvals.len(),
        approvals,
    })
}

async fn resolve_loop_approval(
    pool: &PgPool,
    approval_id: Uuid,
    status: LoopApprovalStatus,
    request: &LoopApprovalDecisionRequest,
) -> Result<LoopApprovalDecisionResponse, ApiError> {
    let row = sqlx::query(
        r#"
        UPDATE approval_requests
        SET status = $2,
            reviewer = $3,
            decision_reason = $4,
            resolved_at = now()
        WHERE id = $1
        RETURNING
            id, run_id, (SELECT slug FROM projects WHERE id = approval_requests.project_id) AS project,
            loop_id, action_type, proposed_action_json, risk_reason, status, requester,
            reviewer, decision_reason, created_at, resolved_at
        "#,
    )
    .bind(approval_id)
    .bind(status.as_str())
    .bind(&request.reviewer)
    .bind(&request.reason)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::sql)?
    .ok_or_else(|| ApiError::not_found("loop approval not found"))?;
    Ok(LoopApprovalDecisionResponse {
        approval: row_to_loop_approval(row)?,
    })
}

async fn find_project_id(pool: &PgPool, project: &str) -> Result<Option<Uuid>, ApiError> {
    sqlx::query_scalar("SELECT id FROM projects WHERE slug = $1")
        .bind(project)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::sql)
}

fn row_to_loop_definition(row: sqlx::postgres::PgRow) -> Result<LoopDefinitionRecord, ApiError> {
    Ok(LoopDefinitionRecord {
        id: row.try_get("id").map_err(ApiError::sql)?,
        loop_id: row.try_get("loop_id").map_err(ApiError::sql)?,
        version: row.try_get("version").map_err(ApiError::sql)?,
        name: row.try_get("name").map_err(ApiError::sql)?,
        description: row.try_get("description").map_err(ApiError::sql)?,
        risk_level: parse_loop_risk(
            row.try_get::<String, _>("risk_level")
                .map_err(ApiError::sql)?,
        )?,
        default_mode: parse_loop_mode(
            row.try_get::<String, _>("default_mode")
                .map_err(ApiError::sql)?,
        )?,
        trigger_spec: row.try_get("trigger_spec").map_err(ApiError::sql)?,
        context_spec: row.try_get("context_spec").map_err(ApiError::sql)?,
        policy_spec: row.try_get("policy_spec").map_err(ApiError::sql)?,
        output_spec: row.try_get("output_spec").map_err(ApiError::sql)?,
        created_at: row.try_get("created_at").map_err(ApiError::sql)?,
    })
}

fn row_to_loop_setting(row: sqlx::postgres::PgRow) -> Result<LoopSettingRecord, ApiError> {
    Ok(LoopSettingRecord {
        id: row.try_get("id").map_err(ApiError::sql)?,
        loop_id: row.try_get("loop_id").map_err(ApiError::sql)?,
        scope_type: parse_scope_type(
            row.try_get::<String, _>("scope_type")
                .map_err(ApiError::sql)?,
        )?,
        scope_id: row.try_get("scope_id").map_err(ApiError::sql)?,
        project: row.try_get("project").map_err(ApiError::sql)?,
        repo_root: row.try_get("repo_root").map_err(ApiError::sql)?,
        enabled: row.try_get("enabled").map_err(ApiError::sql)?,
        mode: row
            .try_get::<Option<String>, _>("mode")
            .map_err(ApiError::sql)?
            .map(parse_loop_mode)
            .transpose()?,
        budgets: row.try_get("budgets_json").map_err(ApiError::sql)?,
        approval_overrides: row
            .try_get("approval_overrides_json")
            .map_err(ApiError::sql)?,
        paused_until: row.try_get("paused_until").map_err(ApiError::sql)?,
        snoozed_until: row.try_get("snoozed_until").map_err(ApiError::sql)?,
        updated_by: row.try_get("updated_by").map_err(ApiError::sql)?,
        reason: row.try_get("reason").map_err(ApiError::sql)?,
        updated_at: row.try_get("updated_at").map_err(ApiError::sql)?,
    })
}

fn row_to_loop_run_summary(row: &sqlx::postgres::PgRow) -> Result<LoopRunSummary, ApiError> {
    Ok(LoopRunSummary {
        id: row.try_get("id").map_err(ApiError::sql)?,
        loop_id: row.try_get("loop_id").map_err(ApiError::sql)?,
        definition_version: row.try_get("definition_version").map_err(ApiError::sql)?,
        project: row.try_get("project").map_err(ApiError::sql)?,
        repo_root: row.try_get("repo_root").map_err(ApiError::sql)?,
        mode: parse_loop_mode(row.try_get::<String, _>("mode").map_err(ApiError::sql)?)?,
        status: parse_run_status(row.try_get::<String, _>("status").map_err(ApiError::sql)?)?,
        started_at: row.try_get("started_at").map_err(ApiError::sql)?,
        finished_at: row.try_get("finished_at").map_err(ApiError::sql)?,
        output_summary: row.try_get("output_summary").map_err(ApiError::sql)?,
        trace_count: row.try_get("trace_count").map_err(ApiError::sql)?,
        blocked_reasons: serde_json::from_value(
            row.try_get::<serde_json::Value, _>("blocked_reasons_json")
                .map_err(ApiError::sql)?,
        )
        .unwrap_or_default(),
    })
}

fn row_to_loop_trace(row: sqlx::postgres::PgRow) -> Result<LoopTraceRecord, ApiError> {
    Ok(LoopTraceRecord {
        id: row.try_get("id").map_err(ApiError::sql)?,
        run_id: row.try_get("run_id").map_err(ApiError::sql)?,
        sequence: row.try_get("sequence").map_err(ApiError::sql)?,
        trace_type: row.try_get("trace_type").map_err(ApiError::sql)?,
        title: row.try_get("title").map_err(ApiError::sql)?,
        payload: row.try_get("payload_json").map_err(ApiError::sql)?,
        redacted: row.try_get("redacted").map_err(ApiError::sql)?,
        created_at: row.try_get("created_at").map_err(ApiError::sql)?,
    })
}

fn row_to_loop_approval(row: sqlx::postgres::PgRow) -> Result<LoopApprovalRequestRecord, ApiError> {
    Ok(LoopApprovalRequestRecord {
        id: row.try_get("id").map_err(ApiError::sql)?,
        run_id: row.try_get("run_id").map_err(ApiError::sql)?,
        project: row.try_get("project").map_err(ApiError::sql)?,
        loop_id: row.try_get("loop_id").map_err(ApiError::sql)?,
        action_type: row.try_get("action_type").map_err(ApiError::sql)?,
        proposed_action: row.try_get("proposed_action_json").map_err(ApiError::sql)?,
        risk_reason: row.try_get("risk_reason").map_err(ApiError::sql)?,
        status: parse_approval_status(row.try_get::<String, _>("status").map_err(ApiError::sql)?)?,
        requester: row.try_get("requester").map_err(ApiError::sql)?,
        reviewer: row.try_get("reviewer").map_err(ApiError::sql)?,
        decision_reason: row.try_get("decision_reason").map_err(ApiError::sql)?,
        created_at: row.try_get("created_at").map_err(ApiError::sql)?,
        resolved_at: row.try_get("resolved_at").map_err(ApiError::sql)?,
    })
}

fn parse_loop_mode(value: String) -> Result<LoopMode, ApiError> {
    match value.as_str() {
        "off" => Ok(LoopMode::Off),
        "observe" => Ok(LoopMode::Observe),
        "suggest_only" => Ok(LoopMode::SuggestOnly),
        "draft_output" => Ok(LoopMode::DraftOutput),
        "autonomous_safe" => Ok(LoopMode::AutonomousSafe),
        "paused" => Ok(LoopMode::Paused),
        "snoozed" => Ok(LoopMode::Snoozed),
        _ => Err(ApiError::status_message(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("unknown loop mode: {value}"),
        )),
    }
}

fn parse_loop_risk(value: String) -> Result<LoopRiskLevel, ApiError> {
    match value.as_str() {
        "low" => Ok(LoopRiskLevel::Low),
        "medium" => Ok(LoopRiskLevel::Medium),
        "high" => Ok(LoopRiskLevel::High),
        "critical" => Ok(LoopRiskLevel::Critical),
        _ => Err(ApiError::status_message(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("unknown loop risk level: {value}"),
        )),
    }
}

fn parse_scope_type(value: String) -> Result<LoopScopeType, ApiError> {
    match value.as_str() {
        "user" => Ok(LoopScopeType::User),
        "workspace" => Ok(LoopScopeType::Workspace),
        "project" => Ok(LoopScopeType::Project),
        "repo" => Ok(LoopScopeType::Repo),
        _ => Err(ApiError::status_message(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("unknown loop scope type: {value}"),
        )),
    }
}

fn parse_run_status(value: String) -> Result<LoopRunStatus, ApiError> {
    match value.as_str() {
        "queued" => Ok(LoopRunStatus::Queued),
        "running" => Ok(LoopRunStatus::Running),
        "succeeded" => Ok(LoopRunStatus::Succeeded),
        "failed" => Ok(LoopRunStatus::Failed),
        "cancelled" => Ok(LoopRunStatus::Cancelled),
        "blocked" => Ok(LoopRunStatus::Blocked),
        _ => Err(ApiError::status_message(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("unknown loop run status: {value}"),
        )),
    }
}

fn parse_approval_status(value: String) -> Result<LoopApprovalStatus, ApiError> {
    match value.as_str() {
        "pending" => Ok(LoopApprovalStatus::Pending),
        "approved" => Ok(LoopApprovalStatus::Approved),
        "rejected" => Ok(LoopApprovalStatus::Rejected),
        "edited" => Ok(LoopApprovalStatus::Edited),
        _ => Err(ApiError::status_message(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("unknown loop approval status: {value}"),
        )),
    }
}
