use crate::prelude::*;
use crate::*;
use mem_api::{EffectiveLoopSettings, LoopActionKind};
use mem_loops::{
    ContextPackBuildInput, TriggerRouteCandidate, budget_blocked, build_context_pack,
    builtin_loop_definitions, diff_context_packs, estimate_tokens, evaluate_action,
    resolve_effective_settings, route_trigger_event, validate_definition,
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
    pub(crate) run_id: Option<Uuid>,
    pub(crate) loop_id: Option<String>,
    pub(crate) status: Option<LoopApprovalStatus>,
    pub(crate) limit: Option<i64>,
}

#[derive(Debug, SerdeDeserialize)]
pub(crate) struct LoopContextPackQuery {
    pub(crate) project: Option<String>,
    pub(crate) repo_root: Option<String>,
    pub(crate) run_id: Option<Uuid>,
    pub(crate) token_budget: Option<usize>,
    pub(crate) limit: Option<usize>,
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
    insert_loop_setting_audit(
        pool,
        Some(&definition.loop_id),
        Some(&setting),
        "setting_update",
        request.updated_by.as_deref(),
        request.reason.as_deref(),
        serde_json::to_value(&setting)
            .map_err(|error| ApiError::io(anyhow::anyhow!("serialize loop setting: {error}")))?,
    )
    .await?;
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

pub(crate) async fn route_loop_trigger(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<LoopTriggerRouteRequest>,
) -> Result<Json<LoopTriggerRouteResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/loops/triggers/route", &request, true).await?,
        ));
    }
    Ok(Json(
        route_loop_trigger_event_inner(state.pool()?, &request).await?,
    ))
}

pub(crate) async fn build_loop_context_pack(
    State(state): State<AppState>,
    Path(loop_id): Path<String>,
    Query(query): Query<LoopContextPackQuery>,
) -> Result<Json<LoopContextPackResponse>, ApiError> {
    if !state.is_primary() {
        let mut params = Vec::new();
        if let Some(project) = &query.project {
            params.push(format!("project={project}"));
        }
        if let Some(repo_root) = &query.repo_root {
            params.push(format!("repo_root={repo_root}"));
        }
        if let Some(run_id) = query.run_id {
            params.push(format!("run_id={run_id}"));
        }
        if let Some(token_budget) = query.token_budget {
            params.push(format!("token_budget={token_budget}"));
        }
        if let Some(limit) = query.limit {
            params.push(format!("limit={limit}"));
        }
        let suffix = if params.is_empty() {
            String::new()
        } else {
            format!("?{}", params.join("&"))
        };
        return Ok(Json(
            proxy_get_json(&state, &format!("/v1/loops/{loop_id}/context-pack{suffix}")).await?,
        ));
    }
    let request = LoopContextPackRequest {
        project: query.project,
        repo_root: query.repo_root,
        run_id: query.run_id,
        token_budget: query.token_budget.unwrap_or(4_000),
        limit: query.limit.unwrap_or(24),
    };
    request.validate().map_err(ApiError::validation)?;
    Ok(Json(
        build_loop_context_pack_response(state.pool()?, &loop_id, &request).await?,
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

pub(crate) async fn get_loop_run_context_pack(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> Result<Json<LoopContextPackResponse>, ApiError> {
    if !state.is_primary() {
        return Ok(Json(
            proxy_get_json(&state, &format!("/v1/loops/runs/{run_id}/context-pack")).await?,
        ));
    }
    let response = fetch_loop_run_context_pack(state.pool()?, run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("loop run context pack not found"))?;
    Ok(Json(response))
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
        resolve_loop_approval_decision(
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
        resolve_loop_approval_decision(
            state.pool()?,
            approval_id,
            LoopApprovalStatus::Rejected,
            &request,
        )
        .await?,
    ))
}

pub(crate) async fn edit_loop_approval(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(approval_id): Path<Uuid>,
    Json(request): Json<LoopApprovalDecisionRequest>,
) -> Result<Json<LoopApprovalDecisionResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    if request.edited_action.is_none() {
        return Err(ApiError::validation(ValidationError::new(
            "edited_action is required",
        )));
    }
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(
                &state,
                &format!("/v1/loops/approvals/{approval_id}/edit"),
                &request,
                true,
            )
            .await?,
        ));
    }
    Ok(Json(
        resolve_loop_approval_decision(
            state.pool()?,
            approval_id,
            LoopApprovalStatus::Edited,
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

pub async fn route_loop_trigger_event(
    pool: &PgPool,
    request: &LoopTriggerRouteRequest,
) -> Result<LoopTriggerRouteResponse> {
    route_loop_trigger_event_inner(pool, request)
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
    let response = LoopGlobalStateResponse {
        kill_switch_enabled: row.try_get("kill_switch_enabled").map_err(ApiError::sql)?,
        updated_by: row.try_get("updated_by").map_err(ApiError::sql)?,
        reason: row.try_get("reason").map_err(ApiError::sql)?,
        updated_at: row.try_get("updated_at").map_err(ApiError::sql)?,
    };
    insert_loop_setting_audit(
        pool,
        None,
        None,
        "global_kill_switch_update",
        request.updated_by.as_deref(),
        request.reason.as_deref(),
        serde_json::to_value(&response)
            .map_err(|error| ApiError::io(anyhow::anyhow!("serialize global state: {error}")))?,
    )
    .await?;
    Ok(response)
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

async fn insert_loop_setting_audit(
    pool: &PgPool,
    loop_id: Option<&str>,
    setting: Option<&LoopSettingRecord>,
    action: &str,
    actor: Option<&str>,
    reason: Option<&str>,
    payload: serde_json::Value,
) -> Result<(), ApiError> {
    sqlx::query(
        r#"
        INSERT INTO loop_setting_audit (
            id, loop_id, setting_id, action, scope_type, scope_id,
            repo_root, actor, reason, payload_json, created_at
        )
        VALUES (gen_random_uuid(), $1, $2, $3, $4, $5, $6, $7, $8, $9, now())
        "#,
    )
    .bind(loop_id)
    .bind(setting.map(|setting| setting.id))
    .bind(action)
    .bind(setting.map(|setting| setting.scope_type.as_str()))
    .bind(setting.map(|setting| setting.scope_id.as_str()))
    .bind(setting.and_then(|setting| setting.repo_root.as_deref()))
    .bind(actor)
    .bind(reason)
    .bind(payload)
    .execute(pool)
    .await
    .map_err(ApiError::sql)?;
    Ok(())
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

struct StoredTriggerEvent {
    event: LoopTriggerEventRecord,
    duplicate: bool,
    debounced: bool,
}

async fn route_loop_trigger_event_inner(
    pool: &PgPool,
    request: &LoopTriggerRouteRequest,
) -> Result<LoopTriggerRouteResponse, ApiError> {
    let definitions = fetch_route_candidate_definitions(pool, request).await?;
    let mut candidates = Vec::with_capacity(definitions.len());
    for definition in &definitions {
        let mut effective = load_effective_loop_settings(
            pool,
            definition,
            request.project.as_deref(),
            request.repo_root.as_deref(),
            false,
        )
        .await?;
        if let Some(reason) = budget_blocked(effective.budgets.as_ref())
            && !effective.blocked_reasons.contains(&reason)
        {
            effective.blocked_reasons.push(reason);
        }
        candidates.push(TriggerRouteCandidate {
            definition: definition.clone(),
            effective_settings: effective,
        });
    }
    let mut decisions = route_trigger_event(&request.event_type, candidates);

    if request.dry_run {
        return Ok(LoopTriggerRouteResponse {
            event: None,
            duplicate: false,
            debounced: false,
            decisions,
            runs: Vec::new(),
        });
    }

    let project_id = match request.project.as_deref() {
        Some(project) => Some(
            upsert_project_slug(pool, project)
                .await
                .map_err(ApiError::sql)?,
        ),
        None => None,
    };
    let stored = store_trigger_event(pool, request, project_id).await?;
    if stored.duplicate || stored.debounced {
        for decision in &mut decisions {
            if decision.eligible {
                decision.eligible = false;
                decision.skipped_reasons.push(if stored.duplicate {
                    "duplicate_trigger".to_string()
                } else {
                    "debounced_trigger".to_string()
                });
            }
        }
        return Ok(LoopTriggerRouteResponse {
            event: Some(stored.event),
            duplicate: stored.duplicate,
            debounced: stored.debounced,
            decisions,
            runs: Vec::new(),
        });
    }

    let mut runs = Vec::new();
    for decision in decisions.iter_mut().filter(|decision| decision.eligible) {
        let run_request = LoopRunRequest {
            project: request.project.clone(),
            repo_root: request.repo_root.clone(),
            scope_type: decision.scope_type.clone(),
            scope_id: decision.scope_id.clone(),
            dry_run: request.dry_run,
            reason: request.reason.clone().or_else(|| {
                Some(format!(
                    "Routed trigger {} from {}",
                    request.event_type, request.source
                ))
            }),
            trigger_payload: Some(request.payload.clone()),
        };
        let run = create_control_plane_loop_run_with_trigger(
            pool,
            &decision.loop_id,
            &run_request,
            stored.event.id,
            false,
        )
        .await?
        .run;
        decision.run_id = Some(run.summary.id);
        runs.push(run.summary);
    }

    Ok(LoopTriggerRouteResponse {
        event: Some(stored.event),
        duplicate: false,
        debounced: false,
        decisions,
        runs,
    })
}

async fn fetch_route_candidate_definitions(
    pool: &PgPool,
    request: &LoopTriggerRouteRequest,
) -> Result<Vec<LoopDefinitionRecord>, ApiError> {
    if request.candidate_loop_ids.is_empty() {
        return fetch_loop_definitions(pool).await;
    }
    let mut definitions = Vec::with_capacity(request.candidate_loop_ids.len());
    for loop_id in &request.candidate_loop_ids {
        definitions.push(fetch_loop_definition(pool, loop_id).await?);
    }
    Ok(definitions)
}

async fn store_trigger_event(
    pool: &PgPool,
    request: &LoopTriggerRouteRequest,
    project_id: Option<Uuid>,
) -> Result<StoredTriggerEvent, ApiError> {
    let payload_hash =
        hex_sha256(&serde_json::to_vec(&request.payload).map_err(|error| {
            ApiError::io(anyhow::anyhow!("serialize trigger payload: {error}"))
        })?);

    if let Some(seconds) = request.debounce_seconds
        && seconds > 0
        && let Some(event) =
            fetch_debounced_trigger_event(pool, request, project_id, &payload_hash, seconds).await?
    {
        return Ok(StoredTriggerEvent {
            event,
            duplicate: false,
            debounced: true,
        });
    }

    let inserted = sqlx::query(
        r#"
        INSERT INTO trigger_events (
            id, source, event_type, project_id, repo_root, payload_hash,
            dedupe_key, trust_level, payload_json, received_at
        )
        VALUES (gen_random_uuid(), $1, $2, $3, $4, $5, $6, $7, $8, now())
        ON CONFLICT (dedupe_key) WHERE dedupe_key IS NOT NULL DO NOTHING
        RETURNING
            id, source, event_type,
            (SELECT slug FROM projects WHERE id = trigger_events.project_id) AS project,
            repo_root, payload_hash, dedupe_key, trust_level, payload_json, received_at
        "#,
    )
    .bind(&request.source)
    .bind(&request.event_type)
    .bind(project_id)
    .bind(&request.repo_root)
    .bind(&payload_hash)
    .bind(&request.dedupe_key)
    .bind(request.trust_level.as_str())
    .bind(&request.payload)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::sql)?;

    if let Some(row) = inserted {
        return Ok(StoredTriggerEvent {
            event: row_to_trigger_event(row)?,
            duplicate: false,
            debounced: false,
        });
    }

    let event = fetch_trigger_event_by_dedupe_key(
        pool,
        request
            .dedupe_key
            .as_deref()
            .ok_or_else(|| ApiError::validation(ValidationError::new("dedupe_key is required")))?,
    )
    .await?;
    Ok(StoredTriggerEvent {
        event,
        duplicate: true,
        debounced: false,
    })
}

async fn fetch_debounced_trigger_event(
    pool: &PgPool,
    request: &LoopTriggerRouteRequest,
    project_id: Option<Uuid>,
    payload_hash: &str,
    debounce_seconds: i64,
) -> Result<Option<LoopTriggerEventRecord>, ApiError> {
    let row = sqlx::query(
        r#"
        SELECT
            id, source, event_type,
            (SELECT slug FROM projects WHERE id = trigger_events.project_id) AS project,
            repo_root, payload_hash, dedupe_key, trust_level, payload_json, received_at
        FROM trigger_events
        WHERE source = $1
          AND event_type = $2
          AND (($3::uuid IS NULL AND project_id IS NULL) OR project_id = $3)
          AND (($4::text IS NULL AND repo_root IS NULL) OR repo_root = $4)
          AND payload_hash = $5
          AND received_at >= now() - ($6::int * interval '1 second')
        ORDER BY received_at DESC
        LIMIT 1
        "#,
    )
    .bind(&request.source)
    .bind(&request.event_type)
    .bind(project_id)
    .bind(&request.repo_root)
    .bind(payload_hash)
    .bind(debounce_seconds as i32)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::sql)?;
    row.map(row_to_trigger_event).transpose()
}

async fn fetch_trigger_event_by_dedupe_key(
    pool: &PgPool,
    dedupe_key: &str,
) -> Result<LoopTriggerEventRecord, ApiError> {
    let row = sqlx::query(
        r#"
        SELECT
            id, source, event_type,
            (SELECT slug FROM projects WHERE id = trigger_events.project_id) AS project,
            repo_root, payload_hash, dedupe_key, trust_level, payload_json, received_at
        FROM trigger_events
        WHERE dedupe_key = $1
        "#,
    )
    .bind(dedupe_key)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::sql)?
    .ok_or_else(|| ApiError::not_found("trigger event not found"))?;
    row_to_trigger_event(row)
}

async fn create_control_plane_loop_run(
    pool: &PgPool,
    loop_id: &str,
    request: &LoopRunRequest,
) -> Result<LoopRunResponse, ApiError> {
    let trigger_payload = request.trigger_payload.clone().unwrap_or_else(|| json!({}));
    let trigger_record = store_trigger_event(
        pool,
        &LoopTriggerRouteRequest {
            source: "manual".to_string(),
            event_type: "manual_run".to_string(),
            project: request.project.clone(),
            repo_root: request.repo_root.clone(),
            payload: trigger_payload,
            dedupe_key: None,
            trust_level: LoopTrustLevel::High,
            debounce_seconds: None,
            dry_run: false,
            reason: request.reason.clone(),
            candidate_loop_ids: vec![loop_id.to_string()],
        },
        None,
    )
    .await?;
    create_control_plane_loop_run_with_trigger(
        pool,
        loop_id,
        request,
        trigger_record.event.id,
        true,
    )
    .await
}

async fn create_control_plane_loop_run_with_trigger(
    pool: &PgPool,
    loop_id: &str,
    request: &LoopRunRequest,
    trigger_event_id: Uuid,
    manual_run: bool,
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
        manual_run,
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
    .bind(trigger_event_id)
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
    let context_pack_request = LoopContextPackRequest {
        project: request.project.clone(),
        repo_root: request.repo_root.clone(),
        run_id: Some(run_id),
        token_budget: 4_000,
        limit: 24,
    };
    if context_pack_request.validate().is_ok() {
        let context_pack =
            build_loop_context_pack_response(pool, &definition.loop_id, &context_pack_request)
                .await?;
        append_loop_trace(
            pool,
            run_id,
            "context_pack",
            "Context pack",
            serde_json::to_value(&context_pack).map_err(|error| {
                ApiError::io(anyhow::anyhow!("serialize context pack trace: {error}"))
            })?,
            false,
        )
        .await?;
        sqlx::query(
            r#"
            UPDATE loop_runs
            SET output_json = output_json || jsonb_build_object(
                'context_pack_id', $2::uuid::text,
                'context_pack_tokens', $3::int,
                'context_pack_memory_count', $4::int
            ),
            updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(run_id)
        .bind(context_pack.pack.id)
        .bind(context_pack.pack.estimated_tokens as i32)
        .bind(context_pack.pack.memories.len() as i32)
        .execute(pool)
        .await
        .map_err(ApiError::sql)?;
    }
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
            lr.effective_settings_json, lr.policy_decisions_json, lr.cost_json, lr.output_json,
            lr.run_reason, lr.trigger_event_id
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
    let trigger_event_id: Option<Uuid> = row.try_get("trigger_event_id").map_err(ApiError::sql)?;
    let trigger_event = match trigger_event_id {
        Some(trigger_event_id) => fetch_trigger_event(pool, trigger_event_id).await?,
        None => None,
    };
    let traces = fetch_loop_traces(pool, run_id).await?;
    let memory_proposals = fetch_loop_memory_proposals(pool, run_id).await?;
    let context_response = context_pack_from_traces(&traces);
    Ok(LoopRunDetail {
        summary,
        run_reason: row.try_get("run_reason").map_err(ApiError::sql)?,
        trigger_event,
        effective_settings: row
            .try_get("effective_settings_json")
            .map_err(ApiError::sql)?,
        policy_decisions: row
            .try_get("policy_decisions_json")
            .map_err(ApiError::sql)?,
        cost: row.try_get("cost_json").map_err(ApiError::sql)?,
        output: row.try_get("output_json").map_err(ApiError::sql)?,
        traces,
        memory_proposals,
        context_pack: context_response
            .as_ref()
            .map(|response| response.pack.clone()),
        context_diff: context_response.and_then(|response| response.diff),
    })
}

async fn fetch_trigger_event(
    pool: &PgPool,
    trigger_event_id: Uuid,
) -> Result<Option<LoopTriggerEventRecord>, ApiError> {
    let row = sqlx::query(
        r#"
        SELECT
            te.id, te.source, te.event_type, p.slug AS project, te.repo_root,
            te.payload_hash, te.dedupe_key, te.trust_level, te.payload_json, te.received_at
        FROM trigger_events te
        LEFT JOIN projects p ON p.id = te.project_id
        WHERE te.id = $1
        "#,
    )
    .bind(trigger_event_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::sql)?;
    row.map(row_to_trigger_event).transpose()
}

async fn fetch_loop_memory_proposals(
    pool: &PgPool,
    run_id: Uuid,
) -> Result<Vec<LoopMemoryProposalRecord>, ApiError> {
    let rows = sqlx::query(
        r#"
        SELECT
            mp.id, mp.run_id, p.slug AS project, mp.loop_id, mp.proposal_type,
            mp.target_memory_id, mp.candidate_json, mp.evidence_json, mp.confidence,
            mp.risk_notes, mp.status, mp.created_at, mp.resolved_at
        FROM memory_proposals mp
        LEFT JOIN projects p ON p.id = mp.project_id
        WHERE mp.run_id = $1
        ORDER BY mp.created_at DESC
        "#,
    )
    .bind(run_id)
    .fetch_all(pool)
    .await
    .map_err(ApiError::sql)?;
    rows.into_iter().map(row_to_memory_proposal).collect()
}

async fn build_loop_context_pack_response(
    pool: &PgPool,
    loop_id: &str,
    request: &LoopContextPackRequest,
) -> Result<LoopContextPackResponse, ApiError> {
    let project = request
        .project
        .as_deref()
        .ok_or_else(|| ApiError::validation(ValidationError::new("project is required")))?;
    let memory_rows = crate::repository::fetch_project_memories(
        pool,
        project,
        Some("active"),
        request.limit as i64 * 3,
        0,
    )
    .await
    .map_err(ApiError::sql)?;
    let mut memories = Vec::new();
    for item in memory_rows.items.into_iter().take(request.limit * 2) {
        if let Some(memory) = crate::repository::handlers::memory::fetch_memory_entry(pool, item.id)
            .await
            .map_err(ApiError::sql)?
        {
            memories.push(memory);
        }
        if memories.len() >= request.limit {
            break;
        }
    }
    let generated_at = chrono::Utc::now();
    let instructions = context_instruction_refs(request.repo_root.as_deref());
    let current = build_context_pack(ContextPackBuildInput {
        loop_id: loop_id.to_string(),
        project: project.to_string(),
        repo_root: request.repo_root.clone(),
        run_id: request.run_id,
        generated_at,
        token_budget: request.token_budget,
        instructions,
        memories,
        metadata: json!({
            "builder": "deterministic",
            "memory_limit": request.limit,
            "token_estimator": "chars_div_4",
        }),
    });
    let previous = fetch_previous_context_pack(
        pool,
        loop_id,
        project,
        request.repo_root.as_deref(),
        request.run_id,
    )
    .await?;
    let diff = diff_context_packs(&current, previous.as_ref());
    Ok(LoopContextPackResponse {
        pack: current,
        diff,
    })
}

async fn fetch_loop_run_context_pack(
    pool: &PgPool,
    run_id: Uuid,
) -> Result<Option<LoopContextPackResponse>, ApiError> {
    let traces = fetch_loop_traces(pool, run_id).await?;
    Ok(context_pack_from_traces(&traces))
}

fn context_pack_from_traces(traces: &[LoopTraceRecord]) -> Option<LoopContextPackResponse> {
    traces
        .iter()
        .rev()
        .find(|trace| trace.trace_type == "context_pack")
        .and_then(|trace| {
            serde_json::from_value::<LoopContextPackResponse>(trace.payload.clone()).ok()
        })
}

async fn fetch_previous_context_pack(
    pool: &PgPool,
    loop_id: &str,
    project: &str,
    repo_root: Option<&str>,
    current_run_id: Option<Uuid>,
) -> Result<Option<LoopContextPack>, ApiError> {
    let row = sqlx::query(
        r#"
        SELECT rt.payload_json
        FROM run_traces rt
        JOIN loop_runs lr ON lr.id = rt.run_id
        JOIN projects p ON p.id = lr.project_id
        WHERE rt.trace_type = 'context_pack'
          AND lr.loop_id = $1
          AND p.slug = $2
          AND ($3::text IS NULL OR lr.repo_root = $3)
          AND ($4::uuid IS NULL OR lr.id <> $4)
        ORDER BY rt.created_at DESC
        LIMIT 1
        "#,
    )
    .bind(loop_id)
    .bind(project)
    .bind(repo_root)
    .bind(current_run_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::sql)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let payload: serde_json::Value = row.try_get("payload_json").map_err(ApiError::sql)?;
    Ok(serde_json::from_value::<LoopContextPackResponse>(payload)
        .ok()
        .map(|response| response.pack))
}

fn context_instruction_refs(repo_root: Option<&str>) -> Vec<LoopContextInstructionRef> {
    let Some(repo_root) = repo_root else {
        return Vec::new();
    };
    let candidates = [
        ("AGENTS.md", "repo agent instructions"),
        (".agents/memory-layer.toml", "repo memory configuration"),
        ("CONTRIBUTING.md", "contribution and validation guidance"),
        (
            "docs/developer/architecture/code-map.md",
            "developer code map",
        ),
    ];
    candidates
        .iter()
        .filter_map(|(relative, reason)| {
            let path = FsPath::new(repo_root).join(relative);
            let metadata = fs::metadata(&path).ok()?;
            if !metadata.is_file() {
                return None;
            }
            let contents = fs::read_to_string(&path).unwrap_or_default();
            Some(LoopContextInstructionRef {
                path: (*relative).to_string(),
                reason: (*reason).to_string(),
                estimated_tokens: estimate_tokens(&contents),
            })
        })
        .collect()
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
          AND ($3::uuid IS NULL OR ar.run_id = $3)
          AND ($4::text IS NULL OR ar.loop_id = $4)
        ORDER BY ar.created_at DESC
        LIMIT $5
        "#,
    )
    .bind(project_id)
    .bind(query.status.as_ref().map(LoopApprovalStatus::as_str))
    .bind(query.run_id)
    .bind(&query.loop_id)
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

pub async fn record_loop_approval_decision(
    pool: &PgPool,
    approval_id: Uuid,
    status: LoopApprovalStatus,
    request: &LoopApprovalDecisionRequest,
) -> Result<LoopApprovalDecisionResponse> {
    resolve_loop_approval_decision(pool, approval_id, status, request)
        .await
        .map_err(|error| anyhow::anyhow!(error.message))
}

async fn resolve_loop_approval_decision(
    pool: &PgPool,
    approval_id: Uuid,
    status: LoopApprovalStatus,
    request: &LoopApprovalDecisionRequest,
) -> Result<LoopApprovalDecisionResponse, ApiError> {
    if status == LoopApprovalStatus::Edited && request.edited_action.is_none() {
        return Err(ApiError::validation(ValidationError::new(
            "edited_action is required",
        )));
    }
    let row = sqlx::query(
        r#"
        UPDATE approval_requests
        SET status = $2,
            reviewer = $3,
            decision_reason = $4,
            proposed_action_json = COALESCE($5, proposed_action_json),
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
    .bind(&request.edited_action)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::sql)?
    .ok_or_else(|| ApiError::not_found("loop approval not found"))?;
    let approval = row_to_loop_approval(row)?;
    persist_approval_side_effects(pool, &approval, &status, request).await?;
    Ok(LoopApprovalDecisionResponse { approval })
}

async fn persist_approval_side_effects(
    pool: &PgPool,
    approval: &LoopApprovalRequestRecord,
    status: &LoopApprovalStatus,
    request: &LoopApprovalDecisionRequest,
) -> Result<(), ApiError> {
    if let Some(proposal_id) = approval_proposal_id(&approval.proposed_action) {
        sqlx::query(
            r#"
            UPDATE memory_proposals
            SET status = $2,
                resolved_at = now()
            WHERE id = $1
            "#,
        )
        .bind(proposal_id)
        .bind(status.as_str())
        .execute(pool)
        .await
        .map_err(ApiError::sql)?;
    }

    if let Some(run_id) = approval.run_id {
        if *status == LoopApprovalStatus::Rejected {
            block_run_for_rejected_approval(pool, run_id, approval, request).await?;
        }
        append_loop_trace(
            pool,
            run_id,
            "approval",
            approval_trace_title(status),
            json!({
                "approval_id": approval.id,
                "action_type": approval.action_type,
                "status": status.as_str(),
                "reviewer": request.reviewer,
                "reason": request.reason,
                "risk_reason": approval.risk_reason,
                "proposed_action": approval.proposed_action,
                "edited_action": request.edited_action
            }),
            false,
        )
        .await?;
    }

    Ok(())
}

async fn block_run_for_rejected_approval(
    pool: &PgPool,
    run_id: Uuid,
    approval: &LoopApprovalRequestRecord,
    request: &LoopApprovalDecisionRequest,
) -> Result<(), ApiError> {
    sqlx::query(
        r#"
        UPDATE loop_runs
        SET status = 'blocked',
            finished_at = COALESCE(finished_at, now()),
            output_summary = 'Loop run blocked by rejected approval.',
            output_json = output_json || jsonb_build_object(
                'approval_rejected', jsonb_build_object(
                    'approval_id', $2::uuid::text,
                    'action_type', $3::text,
                    'reason', $4::text
                )
            ),
            blocked_reasons_json = CASE
                WHEN COALESCE(blocked_reasons_json, '[]'::jsonb) ? 'approval_rejected'
                    THEN blocked_reasons_json
                ELSE COALESCE(blocked_reasons_json, '[]'::jsonb) || '["approval_rejected"]'::jsonb
            END,
            updated_at = now()
        WHERE id = $1
          AND status IN ('queued', 'running')
        "#,
    )
    .bind(run_id)
    .bind(approval.id)
    .bind(&approval.action_type)
    .bind(&request.reason)
    .execute(pool)
    .await
    .map_err(ApiError::sql)?;
    Ok(())
}

fn approval_trace_title(status: &LoopApprovalStatus) -> &'static str {
    match status {
        LoopApprovalStatus::Pending => "Approval pending",
        LoopApprovalStatus::Approved => "Approval accepted",
        LoopApprovalStatus::Rejected => "Approval rejected",
        LoopApprovalStatus::Edited => "Approval edited",
    }
}

fn approval_proposal_id(value: &serde_json::Value) -> Option<Uuid> {
    value
        .get("proposal_id")
        .or_else(|| value.get("memory_proposal_id"))
        .and_then(serde_json::Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
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

fn row_to_memory_proposal(
    row: sqlx::postgres::PgRow,
) -> Result<LoopMemoryProposalRecord, ApiError> {
    Ok(LoopMemoryProposalRecord {
        id: row.try_get("id").map_err(ApiError::sql)?,
        run_id: row.try_get("run_id").map_err(ApiError::sql)?,
        project: row.try_get("project").map_err(ApiError::sql)?,
        loop_id: row.try_get("loop_id").map_err(ApiError::sql)?,
        proposal_type: row.try_get("proposal_type").map_err(ApiError::sql)?,
        target_memory_id: row.try_get("target_memory_id").map_err(ApiError::sql)?,
        candidate: row.try_get("candidate_json").map_err(ApiError::sql)?,
        evidence: row.try_get("evidence_json").map_err(ApiError::sql)?,
        confidence: row.try_get("confidence").map_err(ApiError::sql)?,
        risk_notes: row.try_get("risk_notes").map_err(ApiError::sql)?,
        status: row.try_get("status").map_err(ApiError::sql)?,
        created_at: row.try_get("created_at").map_err(ApiError::sql)?,
        resolved_at: row.try_get("resolved_at").map_err(ApiError::sql)?,
    })
}

fn row_to_trigger_event(row: sqlx::postgres::PgRow) -> Result<LoopTriggerEventRecord, ApiError> {
    Ok(LoopTriggerEventRecord {
        id: row.try_get("id").map_err(ApiError::sql)?,
        source: row.try_get("source").map_err(ApiError::sql)?,
        event_type: row.try_get("event_type").map_err(ApiError::sql)?,
        project: row.try_get("project").map_err(ApiError::sql)?,
        repo_root: row.try_get("repo_root").map_err(ApiError::sql)?,
        payload_hash: row.try_get("payload_hash").map_err(ApiError::sql)?,
        dedupe_key: row.try_get("dedupe_key").map_err(ApiError::sql)?,
        trust_level: parse_trust_level(
            row.try_get::<String, _>("trust_level")
                .map_err(ApiError::sql)?,
        )?,
        payload: row.try_get("payload_json").map_err(ApiError::sql)?,
        received_at: row.try_get("received_at").map_err(ApiError::sql)?,
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

fn parse_trust_level(value: String) -> Result<LoopTrustLevel, ApiError> {
    match value.as_str() {
        "high" => Ok(LoopTrustLevel::High),
        "medium" => Ok(LoopTrustLevel::Medium),
        "low" => Ok(LoopTrustLevel::Low),
        "data_only" => Ok(LoopTrustLevel::DataOnly),
        _ => Err(ApiError::status_message(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("unknown loop trust level: {value}"),
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
