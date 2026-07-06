use crate::prelude::*;
use crate::*;
use mem_api::{EffectiveLoopSettings, LoopActionKind};
use mem_loops::{
    ContextPackBuildInput, MockLoopRunner, RunnerBudget, RunnerCapabilityProfile, RunnerInvocation,
    RunnerTaskPack, RunnerWorkspaceRef, TriggerRouteCandidate, WorktreeSandboxManager,
    budget_blocked, build_context_pack, builtin_loop_definitions, diff_context_packs,
    estimate_tokens, evaluate_action, invoke_runner_with_policy, resolve_effective_settings,
    route_trigger_event, validate_definition,
};
use serde_json::json;
use std::path::PathBuf;

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
pub(crate) struct LoopMemoryProposalsQuery {
    pub(crate) project: Option<String>,
    pub(crate) run_id: Option<Uuid>,
    pub(crate) loop_id: Option<String>,
    pub(crate) status: Option<String>,
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
    let pool = &state.pool()?;
    let definitions = fetch_loop_definitions(pool).await?;
    // Advisory learned-utility sidecar: project-scoped, ordered by utility,
    // never an input to modes or permission gates.
    let mut utilities = Vec::new();
    if state.config.procedural.enabled
        && let Some(project) = query.project.as_deref()
        && let Some(project_id) = find_project_id(pool, project).await?
    {
        let thresholds = mem_reinforce::RecommendationThresholds::from(&state.config.procedural);
        utilities = mem_reinforce::repository::list_procedural_utility(pool, project_id, "loop")
            .await
            .map_err(ApiError::io)?
            .into_iter()
            .map(|snapshot| {
                let recommendation = mem_reinforce::utility_recommendation(&snapshot, &thresholds);
                mem_api::LoopUtilityInfo {
                    loop_id: snapshot.producer_id,
                    utility: snapshot.utility,
                    update_count: snapshot.update_count,
                    recommendation,
                }
            })
            .collect();
    }
    Ok(Json(LoopDefinitionsResponse {
        definitions,
        utilities,
    }))
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
    let pool = &state.pool()?;
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
    Ok(Json(fetch_loop_global_state(&state.pool()?).await?))
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
        store_loop_global_state(&state.pool()?, &request).await?,
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
        let definition = fetch_loop_definition(&state.pool()?, &loop_id).await?;
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

    let pool = &state.pool()?;
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
    let response = create_control_plane_loop_run(&state.pool()?, &loop_id, &request).await?;
    // Consolidation is the one LLM-backed loop: after the deterministic report
    // is stored, synthesize insight proposals when enabled and not dry-run.
    // Runs where the LLM or a cluster fails are logged, never fatal.
    if loop_id == mem_loops::LOOP_MEMORY_CONSOLIDATION
        && state.config.consolidation.enabled
        && !state.config.consolidation.dry_run
        && !request.dry_run
        && let Some(project) = request.project.as_deref()
    {
        let run_id = Some(response.run.summary.id);
        if let Err(error) =
            crate::consolidate::emit_consolidation_proposals(&state, project, run_id).await
        {
            tracing::warn!(error = %error, "consolidation proposal emission failed");
        }
    }
    Ok(Json(response))
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
        route_loop_trigger_event_inner(&state.pool()?, &request).await?,
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
        build_loop_context_pack_response(&state.pool()?, &loop_id, &request).await?,
    ))
}

pub(crate) async fn list_loop_runs(
    State(state): State<AppState>,
    Query(query): Query<LoopRunsQuery>,
) -> Result<Json<LoopRunsResponse>, ApiError> {
    if !state.is_primary() {
        return Ok(Json(proxy_get_json(&state, "/v1/loops/runs").await?));
    }
    Ok(Json(fetch_loop_runs(&state.pool()?, &query).await?))
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
        run: fetch_loop_run_detail(&state.pool()?, run_id).await?,
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
    let response = fetch_loop_run_context_pack(&state.pool()?, run_id)
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
        cancel_loop_run_record(&state.pool()?, run_id, &request).await?,
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
        &state.pool()?,
        run_id,
        "feedback",
        "User feedback",
        json!({"rating": request.rating, "note": request.note}),
        false,
    )
    .await?;
    Ok(Json(LoopRunResponse {
        run: fetch_loop_run_detail(&state.pool()?, run_id).await?,
    }))
}

pub(crate) async fn list_loop_approvals(
    State(state): State<AppState>,
    Query(query): Query<LoopApprovalsQuery>,
) -> Result<Json<LoopApprovalsResponse>, ApiError> {
    if !state.is_primary() {
        return Ok(Json(proxy_get_json(&state, "/v1/loops/approvals").await?));
    }
    Ok(Json(fetch_loop_approvals(&state.pool()?, &query).await?))
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
            &state.pool()?,
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
            &state.pool()?,
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
            &state.pool()?,
            approval_id,
            LoopApprovalStatus::Edited,
            &request,
        )
        .await?,
    ))
}

pub(crate) async fn list_loop_memory_proposals(
    State(state): State<AppState>,
    Query(query): Query<LoopMemoryProposalsQuery>,
) -> Result<Json<LoopMemoryProposalsResponse>, ApiError> {
    if !state.is_primary() {
        return Ok(Json(
            proxy_get_json(&state, "/v1/loops/memory-proposals").await?,
        ));
    }
    Ok(Json(
        fetch_loop_memory_proposals_for_query(&state.pool()?, &query).await?,
    ))
}

pub(crate) async fn create_loop_memory_proposal(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<LoopMemoryProposalCreateRequest>,
) -> Result<Json<LoopMemoryProposalDecisionResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/loops/memory-proposals", &request, true).await?,
        ));
    }
    Ok(Json(
        insert_loop_memory_proposal(&state.pool()?, &request).await?,
    ))
}

pub(crate) async fn approve_loop_memory_proposal(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(proposal_id): Path<Uuid>,
    Json(request): Json<LoopMemoryProposalDecisionRequest>,
) -> Result<Json<LoopMemoryProposalDecisionResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(
                &state,
                &format!("/v1/loops/memory-proposals/{proposal_id}/approve"),
                &request,
                true,
            )
            .await?,
        ));
    }
    Ok(Json(
        resolve_loop_memory_proposal_decision(
            &state.pool()?,
            &state.config.procedural,
            proposal_id,
            "approved",
            &request,
        )
        .await?,
    ))
}

pub(crate) async fn reject_loop_memory_proposal(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(proposal_id): Path<Uuid>,
    Json(request): Json<LoopMemoryProposalDecisionRequest>,
) -> Result<Json<LoopMemoryProposalDecisionResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(
                &state,
                &format!("/v1/loops/memory-proposals/{proposal_id}/reject"),
                &request,
                true,
            )
            .await?,
        ));
    }
    Ok(Json(
        resolve_loop_memory_proposal_decision(
            &state.pool()?,
            &state.config.procedural,
            proposal_id,
            "rejected",
            &request,
        )
        .await?,
    ))
}

pub(crate) async fn edit_loop_memory_proposal(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(proposal_id): Path<Uuid>,
    Json(request): Json<LoopMemoryProposalDecisionRequest>,
) -> Result<Json<LoopMemoryProposalDecisionResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    if request.edited_candidate.is_none()
        && request.edited_evidence.is_none()
        && request.edited_risk_notes.is_none()
    {
        return Err(ApiError::validation(ValidationError::new(
            "at least one edited proposal field is required",
        )));
    }
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(
                &state,
                &format!("/v1/loops/memory-proposals/{proposal_id}/edit"),
                &request,
                true,
            )
            .await?,
        ));
    }
    Ok(Json(
        edit_loop_memory_proposal_record(&state.pool()?, proposal_id, &request).await?,
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

#[allow(clippy::too_many_arguments)]
async fn create_loop_run_approval(
    pool: &PgPool,
    project_id: Option<Uuid>,
    run_id: Uuid,
    loop_id: &str,
    action_type: LoopActionKind,
    proposed_action: serde_json::Value,
    risk_reason: &str,
    requester: Option<&str>,
) -> Result<LoopApprovalRequestRecord, ApiError> {
    let row = sqlx::query(
        r#"
        INSERT INTO approval_requests (
            id, project_id, run_id, loop_id, action_type, proposed_action_json,
            risk_reason, status, requester, created_at
        )
        VALUES (
            gen_random_uuid(), $1, $2, $3, $4, $5,
            $6, 'pending', $7, now()
        )
        RETURNING
            id, run_id, (SELECT slug FROM projects WHERE id = approval_requests.project_id) AS project,
            loop_id, action_type, proposed_action_json, risk_reason, status, requester,
            reviewer, decision_reason, created_at, resolved_at
        "#,
    )
    .bind(project_id)
    .bind(run_id)
    .bind(loop_id)
    .bind(action_type.as_str())
    .bind(proposed_action)
    .bind(risk_reason)
    .bind(requester)
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

fn policy_actions_for_loop(loop_id: &str) -> Vec<LoopActionKind> {
    let mut actions = vec![
        LoopActionKind::ReadMemory,
        LoopActionKind::ReadRepo,
        LoopActionKind::WriteMemoryProposal,
    ];
    if loop_id == mem_loops::LOOP_DRAFT_PR {
        actions.extend([
            LoopActionKind::CreateBranch,
            LoopActionKind::WriteRepo,
            LoopActionKind::RunCommand,
            LoopActionKind::InvokeRunner,
        ]);
    }
    actions
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
    let policy_decisions = policy_actions_for_loop(&definition.loop_id)
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

        if definition.loop_id == mem_loops::LOOP_CONTEXT_PACK_REFRESH && !blocked {
            let refresh = emit_context_pack_refresh_proposals(
                pool,
                run_id,
                request,
                &definition.loop_id,
                &context_pack,
            )
            .await?;
            sqlx::query(
                r#"
                UPDATE loop_runs
                SET output_summary = $2,
                    output_json = output_json || jsonb_build_object('context_refresh', $3::jsonb),
                    updated_at = now()
                WHERE id = $1
                "#,
            )
            .bind(run_id)
            .bind(&refresh.summary)
            .bind(&refresh.output)
            .execute(pool)
            .await
            .map_err(ApiError::sql)?;
        } else if definition.loop_id == mem_loops::LOOP_MEMORY_HYGIENE && !blocked {
            let hygiene =
                emit_memory_hygiene_proposals(pool, run_id, request, &definition.loop_id).await?;
            sqlx::query(
                r#"
                UPDATE loop_runs
                SET output_summary = $2,
                    output_json = output_json || jsonb_build_object('memory_hygiene', $3::jsonb),
                    updated_at = now()
                WHERE id = $1
                "#,
            )
            .bind(run_id)
            .bind(&hygiene.summary)
            .bind(&hygiene.output)
            .execute(pool)
            .await
            .map_err(ApiError::sql)?;
        } else if definition.loop_id == mem_loops::LOOP_CI_FAILURE_TRIAGE && !blocked {
            let triage = emit_ci_failure_triage_report(
                pool,
                run_id,
                request,
                &definition.loop_id,
                &effective.mode,
                &context_pack,
            )
            .await?;
            sqlx::query(
                r#"
                UPDATE loop_runs
                SET output_summary = $2,
                    output_json = output_json || jsonb_build_object('ci_triage', $3::jsonb),
                    updated_at = now()
                WHERE id = $1
                "#,
            )
            .bind(run_id)
            .bind(&triage.summary)
            .bind(&triage.output)
            .execute(pool)
            .await
            .map_err(ApiError::sql)?;
        } else if definition.loop_id == mem_loops::LOOP_AGENT_READY_ISSUE_TRIAGE && !blocked {
            let issue_triage = emit_agent_ready_issue_triage_report(
                pool,
                run_id,
                request,
                &definition.loop_id,
                &effective.mode,
                &context_pack,
            )
            .await?;
            sqlx::query(
                r#"
                UPDATE loop_runs
                SET output_summary = $2,
                    output_json = output_json || jsonb_build_object('issue_triage', $3::jsonb),
                    updated_at = now()
                WHERE id = $1
                "#,
            )
            .bind(run_id)
            .bind(&issue_triage.summary)
            .bind(&issue_triage.output)
            .execute(pool)
            .await
            .map_err(ApiError::sql)?;
        } else if definition.loop_id == mem_loops::LOOP_DRAFT_PR && !blocked {
            let draft_pr = emit_draft_pr_loop_report(
                pool,
                run_id,
                request,
                &definition.loop_id,
                &effective.mode,
                &context_pack,
            )
            .await?;
            sqlx::query(
                r#"
                UPDATE loop_runs
                SET status = $2,
                    output_summary = $3,
                    output_json = output_json || jsonb_build_object('draft_pr', $4::jsonb),
                    blocked_reasons_json = CASE
                        WHEN $5::text IS NULL THEN blocked_reasons_json
                        WHEN COALESCE(blocked_reasons_json, '[]'::jsonb) ? $5::text THEN blocked_reasons_json
                        ELSE COALESCE(blocked_reasons_json, '[]'::jsonb) || jsonb_build_array($5::text)
                    END,
                    updated_at = now()
                WHERE id = $1
                "#,
            )
            .bind(run_id)
            .bind(draft_pr.status.as_str())
            .bind(&draft_pr.summary)
            .bind(&draft_pr.output)
            .bind(&draft_pr.blocked_reason)
            .execute(pool)
            .await
            .map_err(ApiError::sql)?;
        } else if definition.loop_id == mem_loops::LOOP_REVIEWER_DRIFT_DETECTION && !blocked {
            let review = emit_reviewer_drift_report(
                pool,
                run_id,
                request,
                &definition.loop_id,
                &context_pack,
            )
            .await?;
            sqlx::query(
                r#"
                UPDATE loop_runs
                SET output_summary = $2,
                    output_json = output_json || jsonb_build_object('reviewer_drift', $3::jsonb),
                    updated_at = now()
                WHERE id = $1
                "#,
            )
            .bind(run_id)
            .bind(&review.summary)
            .bind(&review.output)
            .execute(pool)
            .await
            .map_err(ApiError::sql)?;
        } else if definition.loop_id == mem_loops::LOOP_SKILL_MINING && !blocked {
            let skill =
                emit_skill_mining_report(pool, run_id, request, &definition.loop_id).await?;
            sqlx::query(
                r#"
                UPDATE loop_runs
                SET output_summary = $2,
                    output_json = output_json || jsonb_build_object('skill_mining', $3::jsonb),
                    updated_at = now()
                WHERE id = $1
                "#,
            )
            .bind(run_id)
            .bind(&skill.summary)
            .bind(&skill.output)
            .execute(pool)
            .await
            .map_err(ApiError::sql)?;
        } else if definition.loop_id == mem_loops::LOOP_MEMORY_EVAL && !blocked {
            let eval = emit_memory_eval_report(pool, run_id, request, &context_pack).await?;
            sqlx::query(
                r#"
                UPDATE loop_runs
                SET output_summary = $2,
                    output_json = output_json || jsonb_build_object('memory_eval', $3::jsonb),
                    updated_at = now()
                WHERE id = $1
                "#,
            )
            .bind(run_id)
            .bind(&eval.summary)
            .bind(&eval.output)
            .execute(pool)
            .await
            .map_err(ApiError::sql)?;
        } else if definition.loop_id == mem_loops::LOOP_MEMORY_CONSOLIDATION && !blocked {
            // Deterministic clustering always runs and is stored as a report.
            // LLM synthesis + proposal emission happens separately where an
            // AppState (and thus the LLM client) is available.
            let cfg = mem_api::ConsolidationConfig::default();
            let project = request
                .project
                .as_deref()
                .ok_or_else(|| ApiError::validation(ValidationError::new("project is required")))?;
            let report =
                crate::repository::handlers::consolidation::run_memory_consolidation_default(
                    pool, project, &cfg,
                )
                .await?;
            let output = serde_json::to_value(&report).map_err(|error| {
                ApiError::io(anyhow::anyhow!("serialize consolidation report: {error}"))
            })?;
            sqlx::query(
                r#"
                UPDATE loop_runs
                SET output_summary = $2,
                    output_json = output_json || jsonb_build_object('consolidation', $3::jsonb),
                    updated_at = now()
                WHERE id = $1
                "#,
            )
            .bind(run_id)
            .bind(report.summary())
            .bind(&output)
            .execute(pool)
            .await
            .map_err(ApiError::sql)?;
        }
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

async fn fetch_loop_memory_proposals_for_query(
    pool: &PgPool,
    query: &LoopMemoryProposalsQuery,
) -> Result<LoopMemoryProposalsResponse, ApiError> {
    if let Some(status) = query.status.as_deref() {
        validate_memory_proposal_status(status)?;
    }
    let limit = query.limit.unwrap_or(50).clamp(1, 250);
    let rows = sqlx::query(
        r#"
        SELECT
            mp.id, mp.run_id, p.slug AS project, mp.loop_id, mp.proposal_type,
            mp.target_memory_id, mp.candidate_json, mp.evidence_json, mp.confidence,
            mp.risk_notes, mp.status, mp.created_at, mp.resolved_at
        FROM memory_proposals mp
        LEFT JOIN projects p ON p.id = mp.project_id
        WHERE ($1::text IS NULL OR p.slug = $1)
          AND ($2::uuid IS NULL OR mp.run_id = $2)
          AND ($3::text IS NULL OR mp.loop_id = $3)
          AND ($4::text IS NULL OR mp.status = $4)
        ORDER BY mp.created_at DESC
        LIMIT $5
        "#,
    )
    .bind(&query.project)
    .bind(query.run_id)
    .bind(&query.loop_id)
    .bind(&query.status)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(ApiError::sql)?;
    let proposals = rows
        .into_iter()
        .map(row_to_memory_proposal)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(LoopMemoryProposalsResponse {
        total_returned: proposals.len(),
        proposals,
    })
}

pub async fn create_memory_proposal_record(
    pool: &PgPool,
    request: &LoopMemoryProposalCreateRequest,
) -> Result<LoopMemoryProposalDecisionResponse> {
    insert_loop_memory_proposal(pool, request)
        .await
        .map_err(|error| anyhow::anyhow!(error.message))
}

/// Queue human-gated proposals for semantic-dedup pairs found after embedding
/// maintenance. Non-conflict pairs become `merge` proposals (duplicates);
/// conflict pairs become `link` proposals flagged as likely contradictions.
/// Pairs already covered by a pending semantic_dedup proposal are skipped.
pub async fn queue_semantic_dedup_proposals(
    pool: &PgPool,
    project: &str,
    duplicates: &[mem_curate::SemanticDuplicate],
) -> Result<usize> {
    let mut queued = 0usize;
    for duplicate in duplicates {
        let already_pending = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1
                FROM memory_proposals mp
                JOIN projects p ON p.id = mp.project_id
                WHERE p.slug = $1
                  AND mp.loop_id = 'semantic_dedup'
                  AND mp.status = 'pending'
                  AND (
                      (mp.target_memory_id = $2 AND mp.candidate_json->>'related_memory_id' = $3)
                      OR (mp.target_memory_id = $4 AND mp.candidate_json->>'related_memory_id' = $5)
                  )
            )
            "#,
        )
        .bind(project)
        .bind(duplicate.memory_id)
        .bind(duplicate.other_memory_id.to_string())
        .bind(duplicate.other_memory_id)
        .bind(duplicate.memory_id.to_string())
        .fetch_one(pool)
        .await?;
        if already_pending {
            continue;
        }

        let (proposal_type, relation_type, confidence, risk_notes) = if duplicate.conflict {
            (
                "link",
                "related_to",
                0.60f32,
                "High embedding similarity with low lexical overlap and supersede/negation \
                 cues: likely a contradiction between an old and a new fact, not a duplicate. \
                 Review which memory is current before acting.",
            )
        } else {
            (
                "merge",
                "duplicates",
                0.74f32,
                "Detected by chunk-embedding similarity; verify both memories describe the \
                 same fact before approving the merge relation.",
            )
        };
        let request = LoopMemoryProposalCreateRequest {
            project: project.to_string(),
            loop_id: "semantic_dedup".to_string(),
            proposal_type: proposal_type.to_string(),
            run_id: None,
            target_memory_id: Some(duplicate.memory_id),
            candidate: serde_json::json!({
                "related_memory_id": duplicate.other_memory_id,
                "relation_type": relation_type,
                "summary": format!(
                    "{} semantically similar memory {}",
                    if duplicate.conflict { "Review conflicting" } else { "Merge duplicate" },
                    duplicate.other_memory_id
                ),
                "evidence_summary": format!(
                    "Target `{}` and related `{}`.",
                    duplicate.summary, duplicate.other_summary
                )
            }),
            evidence: serde_json::json!([{
                "source_kind": "note",
                "excerpt": format!(
                    "Max chunk cosine similarity {:.3}, lexical overlap {:.2}. Target: {} `{}`. Related: {} `{}`.",
                    duplicate.similarity, duplicate.lexical_overlap,
                    duplicate.memory_id, duplicate.summary,
                    duplicate.other_memory_id, duplicate.other_summary
                )
            }]),
            confidence,
            risk_notes: Some(risk_notes.to_string()),
        };
        create_memory_proposal_record(pool, &request).await?;
        queued += 1;
    }
    Ok(queued)
}

pub async fn record_loop_memory_proposal_decision(
    pool: &PgPool,
    procedural: &mem_api::ProceduralConfig,
    proposal_id: Uuid,
    status: &str,
    request: &LoopMemoryProposalDecisionRequest,
) -> Result<LoopMemoryProposalDecisionResponse> {
    resolve_loop_memory_proposal_decision(pool, procedural, proposal_id, status, request)
        .await
        .map_err(|error| anyhow::anyhow!(error.message))
}

async fn insert_loop_memory_proposal(
    pool: &PgPool,
    request: &LoopMemoryProposalCreateRequest,
) -> Result<LoopMemoryProposalDecisionResponse, ApiError> {
    let project_id = upsert_project_slug(pool, &request.project)
        .await
        .map_err(ApiError::sql)?;
    let row = sqlx::query(
        r#"
        INSERT INTO memory_proposals (
            id, run_id, project_id, loop_id, proposal_type, target_memory_id,
            candidate_json, evidence_json, confidence, risk_notes, status, created_at
        )
        VALUES (
            gen_random_uuid(), $1, $2, $3, $4, $5,
            $6, $7, $8, $9, 'pending', now()
        )
        RETURNING
            id, run_id, (SELECT slug FROM projects WHERE id = memory_proposals.project_id) AS project,
            loop_id, proposal_type, target_memory_id, candidate_json, evidence_json,
            confidence, risk_notes, status, created_at, resolved_at
        "#,
    )
    .bind(request.run_id)
    .bind(project_id)
    .bind(&request.loop_id)
    .bind(&request.proposal_type)
    .bind(request.target_memory_id)
    .bind(&request.candidate)
    .bind(&request.evidence)
    .bind(request.confidence)
    .bind(&request.risk_notes)
    .fetch_one(pool)
    .await
    .map_err(ApiError::sql)?;
    let proposal = row_to_memory_proposal(row)?;
    append_memory_proposal_trace(pool, &proposal, "created", None).await?;
    Ok(LoopMemoryProposalDecisionResponse {
        proposal,
        memory_id: None,
    })
}

/// Test/CLI-facing wrapper mirroring [`record_loop_memory_proposal_decision`].
pub async fn record_loop_memory_proposal_edit(
    pool: &PgPool,
    proposal_id: Uuid,
    request: &LoopMemoryProposalDecisionRequest,
) -> Result<LoopMemoryProposalDecisionResponse> {
    edit_loop_memory_proposal_record(pool, proposal_id, request)
        .await
        .map_err(|error| anyhow::anyhow!(error.message))
}

async fn edit_loop_memory_proposal_record(
    pool: &PgPool,
    proposal_id: Uuid,
    request: &LoopMemoryProposalDecisionRequest,
) -> Result<LoopMemoryProposalDecisionResponse, ApiError> {
    let row = sqlx::query(
        r#"
        UPDATE memory_proposals
        SET candidate_json = COALESCE($2, candidate_json),
            evidence_json = COALESCE($3, evidence_json),
            risk_notes = COALESCE($4, risk_notes),
            status = 'pending',
            resolved_at = NULL,
            was_edited = TRUE
        WHERE id = $1
        RETURNING
            id, run_id, (SELECT slug FROM projects WHERE id = memory_proposals.project_id) AS project,
            loop_id, proposal_type, target_memory_id, candidate_json, evidence_json,
            confidence, risk_notes, status, created_at, resolved_at
        "#,
    )
    .bind(proposal_id)
    .bind(&request.edited_candidate)
    .bind(&request.edited_evidence)
    .bind(&request.edited_risk_notes)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::sql)?
    .ok_or_else(|| ApiError::not_found("loop memory proposal not found"))?;
    let proposal = row_to_memory_proposal(row)?;
    append_memory_proposal_trace(pool, &proposal, "edited", request.reason.as_deref()).await?;
    Ok(LoopMemoryProposalDecisionResponse {
        proposal,
        memory_id: None,
    })
}

async fn resolve_loop_memory_proposal_decision(
    pool: &PgPool,
    procedural: &mem_api::ProceduralConfig,
    proposal_id: Uuid,
    status: &str,
    request: &LoopMemoryProposalDecisionRequest,
) -> Result<LoopMemoryProposalDecisionResponse, ApiError> {
    validate_memory_proposal_status(status)?;
    if status == "rejected" {
        // One transaction covers the status write, the procedural-utility
        // reward, and its audit row, so learning can never diverge from the
        // recorded decision.
        let mut tx = pool.begin().await.map_err(ApiError::sql)?;
        let locked = lock_memory_proposal(&mut tx, proposal_id).await?;
        let undecided = locked.record.status == "pending" || locked.record.status == "edited";
        let row = sqlx::query(
            r#"
            UPDATE memory_proposals
            SET status = 'rejected',
                resolved_at = now()
            WHERE id = $1
            RETURNING
                id, run_id, (SELECT slug FROM projects WHERE id = memory_proposals.project_id) AS project,
                loop_id, proposal_type, target_memory_id, candidate_json, evidence_json,
                confidence, risk_notes, status, created_at, resolved_at
            "#,
        )
        .bind(proposal_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(ApiError::sql)?;
        // Reward only the first terminal decision — re-rejecting an already
        // resolved proposal must not double-penalize the loop.
        if undecided {
            emit_proposal_reward(
                &mut tx,
                procedural,
                &locked,
                mem_reinforce::RewardEvent::ProposalRejected,
            )
            .await?;
        }
        tx.commit().await.map_err(ApiError::sql)?;
        let proposal = row_to_memory_proposal(row)?;
        append_memory_proposal_trace(pool, &proposal, "rejected", request.reason.as_deref())
            .await?;
        return Ok(LoopMemoryProposalDecisionResponse {
            proposal,
            memory_id: None,
        });
    }
    if status != "approved" {
        return Err(ApiError::validation(ValidationError::new(
            "proposal decision must be approved or rejected",
        )));
    }

    let mut tx = pool.begin().await.map_err(ApiError::sql)?;
    let proposal = lock_memory_proposal(&mut tx, proposal_id).await?;
    if proposal.record.status != "pending" && proposal.record.status != "edited" {
        return Err(ApiError::validation(ValidationError::new(format!(
            "memory proposal is already {}",
            proposal.record.status
        ))));
    }
    let memory_id = apply_memory_proposal(&mut tx, &proposal).await?;
    let row = sqlx::query(
        r#"
        UPDATE memory_proposals
        SET status = 'approved',
            resolved_at = now()
        WHERE id = $1
        RETURNING
            id, run_id, (SELECT slug FROM projects WHERE id = memory_proposals.project_id) AS project,
            loop_id, proposal_type, target_memory_id, candidate_json, evidence_json,
            confidence, risk_notes, status, created_at, resolved_at
        "#,
    )
    .bind(proposal_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(ApiError::sql)?;
    let event = if proposal.was_edited {
        mem_reinforce::RewardEvent::ProposalEditedApproved
    } else {
        mem_reinforce::RewardEvent::ProposalApproved
    };
    emit_proposal_reward(&mut tx, procedural, &proposal, event).await?;
    tx.commit().await.map_err(ApiError::sql)?;

    let proposal = row_to_memory_proposal(row)?;
    append_memory_proposal_trace(pool, &proposal, "approved", request.reason.as_deref()).await?;
    Ok(LoopMemoryProposalDecisionResponse {
        proposal,
        memory_id,
    })
}

/// Applies the ACT-R delta-rule utility update + audit for one proposal
/// decision, inside the caller's transaction. Advisory learning only — this
/// never touches loop modes or permission gates.
async fn emit_proposal_reward(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    procedural: &mem_api::ProceduralConfig,
    proposal: &LockedMemoryProposal,
    event: mem_reinforce::RewardEvent,
) -> Result<(), ApiError> {
    if !procedural.enabled {
        return Ok(());
    }
    let params = mem_reinforce::UtilityParams::from(procedural);
    let rewards = mem_reinforce::ProceduralRewards::from(procedural);
    let reward = event.reward(&rewards);
    let update = mem_reinforce::repository::apply_utility_reward(
        &mut **tx,
        proposal.project_id,
        "loop",
        &proposal.record.loop_id,
        reward,
        &params,
    )
    .await
    .map_err(ApiError::io)?;
    mem_reinforce::repository::insert_utility_audit(
        &mut **tx,
        proposal.project_id,
        "loop",
        &proposal.record.loop_id,
        event.audit_reason(),
        reward,
        params.alpha,
        &update,
        serde_json::json!({
            "proposal_id": proposal.record.id,
            "run_id": proposal.record.run_id,
        }),
    )
    .await
    .map_err(ApiError::io)?;
    Ok(())
}

struct LockedMemoryProposal {
    record: LoopMemoryProposalRecord,
    project_id: Uuid,
    /// True when the proposal was human-edited before this decision; an
    /// edited-then-approved proposal earns only partial procedural reward.
    was_edited: bool,
}

async fn lock_memory_proposal(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    proposal_id: Uuid,
) -> Result<LockedMemoryProposal, ApiError> {
    let row = sqlx::query(
        r#"
        SELECT
            mp.id, mp.run_id, mp.project_id, p.slug AS project, mp.loop_id,
            mp.proposal_type, mp.target_memory_id, mp.candidate_json, mp.evidence_json,
            mp.confidence, mp.risk_notes, mp.status, mp.created_at, mp.resolved_at,
            mp.was_edited
        FROM memory_proposals mp
        LEFT JOIN projects p ON p.id = mp.project_id
        WHERE mp.id = $1
        FOR UPDATE OF mp
        "#,
    )
    .bind(proposal_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(ApiError::sql)?
    .ok_or_else(|| ApiError::not_found("loop memory proposal not found"))?;
    let project_id = row.try_get("project_id").map_err(ApiError::sql)?;
    let was_edited = row.try_get("was_edited").map_err(ApiError::sql)?;
    let record = row_to_memory_proposal(row)?;
    Ok(LockedMemoryProposal {
        record,
        project_id,
        was_edited,
    })
}

async fn apply_memory_proposal(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    proposal: &LockedMemoryProposal,
) -> Result<Option<Uuid>, ApiError> {
    match proposal.record.proposal_type.as_str() {
        "add" => insert_memory_from_proposal(tx, proposal, None)
            .await
            .map(Some),
        "update" => insert_memory_update_from_proposal(tx, proposal)
            .await
            .map(Some),
        "deprecate" => archive_memory_from_proposal(tx, proposal).await.map(Some),
        "merge" | "link" => link_memories_from_proposal(tx, proposal).await.map(Some),
        "consolidate" => apply_consolidation_proposal(tx, proposal).await.map(Some),
        _ => Err(ApiError::validation(ValidationError::new(
            "unsupported proposal_type",
        ))),
    }
}

/// Applies a `consolidate` proposal atomically: inserts the new `insight`
/// meta-memory (with member provenance carried as `memory` sources), then
/// links it to each member's latest active version with a `summarizes`
/// relation. One approval, one transaction — members stay `active`.
async fn apply_consolidation_proposal(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    proposal: &LockedMemoryProposal,
) -> Result<Uuid, ApiError> {
    let meta_id = insert_memory_from_proposal(tx, proposal, None).await?;
    let members = proposal
        .record
        .candidate
        .get("member_canonical_ids")
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .filter_map(|value| Uuid::parse_str(value).ok())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    for member_canonical_id in members {
        let member_version_id: Option<Uuid> = sqlx::query_scalar(
            r#"
            SELECT id FROM memory_entries
            WHERE canonical_id = $1
              AND status = 'active'
              AND COALESCE(is_tombstone, false) = false
            ORDER BY version_no DESC
            LIMIT 1
            "#,
        )
        .bind(member_canonical_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(ApiError::sql)?;
        let Some(member_version_id) = member_version_id else {
            continue;
        };
        sqlx::query(
            r#"
            INSERT INTO memory_relations (id, src_memory_id, relation_type, dst_memory_id)
            VALUES (gen_random_uuid(), $1, 'summarizes', $2)
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(meta_id)
        .bind(member_version_id)
        .execute(&mut **tx)
        .await
        .map_err(ApiError::sql)?;
    }
    Ok(meta_id)
}

async fn insert_memory_update_from_proposal(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    proposal: &LockedMemoryProposal,
) -> Result<Uuid, ApiError> {
    let target_id = proposal.record.target_memory_id.ok_or_else(|| {
        ApiError::validation(ValidationError::new("target_memory_id is required"))
    })?;
    let row = sqlx::query(
        r#"
        SELECT latest.id, latest.canonical_id, latest.version_no, latest.canonical_text,
               latest.summary, latest.memory_type, latest.scope, latest.importance,
               latest.confidence, latest.status
        FROM memory_entries target
        JOIN LATERAL (
            SELECT m.*
            FROM memory_entries m
            WHERE m.canonical_id = target.canonical_id
            ORDER BY m.version_no DESC
            LIMIT 1
        ) latest ON TRUE
        WHERE target.id = $1
        "#,
    )
    .bind(target_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(ApiError::sql)?
    .ok_or_else(|| ApiError::not_found("target memory not found"))?;

    insert_memory_from_proposal(tx, proposal, Some(row)).await
}

async fn insert_memory_from_proposal(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    proposal: &LockedMemoryProposal,
    previous: Option<sqlx::postgres::PgRow>,
) -> Result<Uuid, ApiError> {
    let candidate = &proposal.record.candidate;
    let canonical_text = candidate_text(candidate, &["canonical_text", "text", "content"])
        .or_else(|| {
            previous
                .as_ref()
                .and_then(|row| row.try_get("canonical_text").ok())
        })
        .ok_or_else(|| {
            ApiError::validation(ValidationError::new(
                "candidate canonical_text or text is required",
            ))
        })?;
    let summary = candidate_text(candidate, &["summary", "title"])
        .or_else(|| {
            previous
                .as_ref()
                .and_then(|row| row.try_get("summary").ok())
        })
        .unwrap_or_else(|| truncate_for_summary(&canonical_text));
    let memory_type = candidate_text(candidate, &["memory_type"])
        .or_else(|| {
            previous
                .as_ref()
                .and_then(|row| row.try_get::<String, _>("memory_type").ok())
        })
        .unwrap_or_else(|| "implementation".to_string());
    let scope = candidate_text(candidate, &["scope"])
        .or_else(|| previous.as_ref().and_then(|row| row.try_get("scope").ok()))
        .unwrap_or_else(|| "project".to_string());
    let importance = candidate_i32(candidate, "importance")
        .or_else(|| {
            previous
                .as_ref()
                .and_then(|row| row.try_get("importance").ok())
        })
        .unwrap_or(3);
    let confidence = candidate_f32(candidate, "confidence")
        .or_else(|| {
            previous
                .as_ref()
                .and_then(|row| row.try_get::<f32, _>("confidence").ok())
        })
        .unwrap_or(proposal.record.confidence);
    let memory_id = Uuid::new_v4();
    let (canonical_id, version_no) = if let Some(row) = &previous {
        (
            row.try_get("canonical_id").map_err(ApiError::sql)?,
            row.try_get::<i32, _>("version_no").map_err(ApiError::sql)? + 1,
        )
    } else {
        (memory_id, 1)
    };

    sqlx::query(
        r#"
        INSERT INTO memory_entries
            (id, project_id, canonical_id, version_no, is_tombstone,
             canonical_text, summary, memory_type, scope, importance, confidence,
             status, created_at, updated_at, archived_at, search_document)
        VALUES
            ($1, $2, $3, $4, FALSE, $5, $6, $7, $8, $9, $10,
             'active', now(), now(), NULL, to_tsvector('english', $5 || ' ' || $6))
        "#,
    )
    .bind(memory_id)
    .bind(proposal.project_id)
    .bind(canonical_id)
    .bind(version_no)
    .bind(&canonical_text)
    .bind(&summary)
    .bind(parse_memory_type(&memory_type).to_string())
    .bind(&scope)
    .bind(importance)
    .bind(confidence)
    .execute(&mut **tx)
    .await
    .map_err(ApiError::sql)?;

    insert_memory_tags_from_candidate(tx, memory_id, candidate).await?;
    insert_memory_sources_from_proposal(tx, memory_id, proposal).await?;
    // Durable producer link so later citations of this memory can reward the
    // loop that created it (procedural utility), without parsing note strings.
    sqlx::query(
        r#"
        INSERT INTO loop_produced_memory (canonical_id, project_id, loop_id, run_id)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (canonical_id) DO NOTHING
        "#,
    )
    .bind(canonical_id)
    .bind(proposal.project_id)
    .bind(&proposal.record.loop_id)
    .bind(proposal.record.run_id)
    .execute(&mut **tx)
    .await
    .map_err(ApiError::sql)?;
    Ok(memory_id)
}

async fn archive_memory_from_proposal(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    proposal: &LockedMemoryProposal,
) -> Result<Uuid, ApiError> {
    let target_id = proposal.record.target_memory_id.ok_or_else(|| {
        ApiError::validation(ValidationError::new("target_memory_id is required"))
    })?;
    let row = sqlx::query(
        r#"
        WITH target AS (
            SELECT canonical_id FROM memory_entries WHERE id = $1
        ),
        latest AS (
            SELECT m.id
            FROM memory_entries m
            JOIN target ON target.canonical_id = m.canonical_id
            ORDER BY m.version_no DESC
            LIMIT 1
        )
        UPDATE memory_entries
        SET status = 'archived',
            archived_at = now(),
            updated_at = now()
        WHERE id = (SELECT id FROM latest)
        RETURNING id
        "#,
    )
    .bind(target_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(ApiError::sql)?
    .ok_or_else(|| ApiError::not_found("target memory not found"))?;
    row.try_get("id").map_err(ApiError::sql)
}

async fn link_memories_from_proposal(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    proposal: &LockedMemoryProposal,
) -> Result<Uuid, ApiError> {
    let src_memory_id = proposal.record.target_memory_id.ok_or_else(|| {
        ApiError::validation(ValidationError::new("target_memory_id is required"))
    })?;
    let dst_memory_id = candidate_uuid(
        &proposal.record.candidate,
        &["related_memory_id", "dst_memory_id", "memory_id"],
    )
    .ok_or_else(|| {
        ApiError::validation(ValidationError::new(
            "candidate related_memory_id is required",
        ))
    })?;
    let relation_type = candidate_text(&proposal.record.candidate, &["relation_type"])
        .unwrap_or_else(|| {
            if proposal.record.proposal_type == "merge" {
                "duplicates".to_string()
            } else {
                "related_to".to_string()
            }
        });
    sqlx::query(
        r#"
        INSERT INTO memory_relations (id, src_memory_id, relation_type, dst_memory_id)
        VALUES (gen_random_uuid(), $1, $2, $3)
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(src_memory_id)
    .bind(parse_relation_type(&relation_type).to_string())
    .bind(dst_memory_id)
    .execute(&mut **tx)
    .await
    .map_err(ApiError::sql)?;
    Ok(src_memory_id)
}

async fn insert_memory_tags_from_candidate(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    memory_id: Uuid,
    candidate: &serde_json::Value,
) -> Result<(), ApiError> {
    for tag in candidate
        .get("tags")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
    {
        sqlx::query(
            "INSERT INTO memory_tags (memory_entry_id, tag) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(memory_id)
        .bind(tag)
        .execute(&mut **tx)
        .await
        .map_err(ApiError::sql)?;
    }
    Ok(())
}

async fn insert_memory_sources_from_proposal(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    memory_id: Uuid,
    proposal: &LockedMemoryProposal,
) -> Result<(), ApiError> {
    let note = format!(
        "Accepted loop memory proposal {} from loop {}{}{}.",
        proposal.record.id,
        proposal.record.loop_id,
        proposal
            .record
            .run_id
            .map(|run_id| format!(" run {run_id}"))
            .unwrap_or_default(),
        proposal
            .record
            .risk_notes
            .as_deref()
            .map(|risk| format!(" Risk notes: {risk}"))
            .unwrap_or_default()
    );
    insert_memory_source(tx, memory_id, "note", None, None, Some(&note)).await?;
    for item in evidence_source_items(&proposal.record.evidence) {
        let source_kind = item
            .get("source_kind")
            .or_else(|| item.get("kind"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("note");
        let file_path = item
            .get("file_path")
            .or_else(|| item.get("path"))
            .and_then(serde_json::Value::as_str);
        let git_commit = item.get("git_commit").and_then(serde_json::Value::as_str);
        let excerpt = item
            .get("excerpt")
            .or_else(|| item.get("summary"))
            .or_else(|| item.get("reason"))
            .and_then(serde_json::Value::as_str);
        insert_memory_source(tx, memory_id, source_kind, file_path, git_commit, excerpt).await?;
    }
    Ok(())
}

async fn insert_memory_source(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    memory_id: Uuid,
    source_kind: &str,
    file_path: Option<&str>,
    git_commit: Option<&str>,
    excerpt: Option<&str>,
) -> Result<(), ApiError> {
    sqlx::query(
        r#"
        INSERT INTO memory_sources
            (id, memory_entry_id, task_id, file_path, git_commit, source_kind, excerpt, created_at)
        VALUES (gen_random_uuid(), $1, NULL, $2, $3, $4, $5, now())
        "#,
    )
    .bind(memory_id)
    .bind(file_path)
    .bind(git_commit)
    .bind(source_kind_string(source_kind))
    .bind(excerpt)
    .execute(&mut **tx)
    .await
    .map_err(ApiError::sql)?;
    Ok(())
}

fn evidence_source_items(
    value: &serde_json::Value,
) -> Vec<&serde_json::Map<String, serde_json::Value>> {
    if let Some(array) = value.as_array() {
        return array
            .iter()
            .filter_map(serde_json::Value::as_object)
            .collect();
    }
    if let Some(object) = value.as_object() {
        for key in ["sources", "evidence_refs", "refs"] {
            if let Some(array) = object.get(key).and_then(serde_json::Value::as_array) {
                return array
                    .iter()
                    .filter_map(serde_json::Value::as_object)
                    .collect();
            }
        }
        return vec![object];
    }
    Vec::new()
}

async fn append_memory_proposal_trace(
    pool: &PgPool,
    proposal: &LoopMemoryProposalRecord,
    action: &str,
    reason: Option<&str>,
) -> Result<(), ApiError> {
    if let Some(run_id) = proposal.run_id {
        append_loop_trace(
            pool,
            run_id,
            "memory_proposal",
            &format!("Memory proposal {action}"),
            json!({
                "proposal_id": proposal.id,
                "proposal_type": proposal.proposal_type,
                "status": proposal.status,
                "target_memory_id": proposal.target_memory_id,
                "candidate": proposal.candidate,
                "evidence": proposal.evidence,
                "confidence": proposal.confidence,
                "risk_notes": proposal.risk_notes,
                "reason": reason,
            }),
            false,
        )
        .await?;
    }
    Ok(())
}

fn validate_memory_proposal_status(value: &str) -> Result<(), ApiError> {
    match value {
        "pending" | "approved" | "rejected" | "edited" => Ok(()),
        _ => Err(ApiError::validation(ValidationError::new(
            "status must be pending, approved, rejected, or edited",
        ))),
    }
}

fn candidate_text(candidate: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| candidate.get(*key).and_then(serde_json::Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn candidate_i32(candidate: &serde_json::Value, key: &str) -> Option<i32> {
    candidate
        .get(key)
        .and_then(serde_json::Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
}

fn candidate_f32(candidate: &serde_json::Value, key: &str) -> Option<f32> {
    candidate
        .get(key)
        .and_then(serde_json::Value::as_f64)
        .map(|value| value as f32)
}

fn candidate_uuid(candidate: &serde_json::Value, keys: &[&str]) -> Option<Uuid> {
    keys.iter()
        .find_map(|key| candidate.get(*key).and_then(serde_json::Value::as_str))
        .and_then(|value| Uuid::parse_str(value).ok())
}

fn truncate_for_summary(text: &str) -> String {
    text.chars().take(120).collect()
}

fn source_kind_string(value: &str) -> &'static str {
    match parse_source_kind(value) {
        SourceKind::TaskPrompt => "task_prompt",
        SourceKind::File => "file",
        SourceKind::GitCommit => "git_commit",
        SourceKind::CommandOutput => "command_output",
        SourceKind::Test => "test",
        SourceKind::Note => "note",
        SourceKind::Memory => "memory",
    }
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

struct ContextRefreshResult {
    summary: String,
    output: serde_json::Value,
}

struct ContextRefreshProposalDraft {
    proposal_type: String,
    target_memory_id: Option<Uuid>,
    candidate: serde_json::Value,
    evidence: serde_json::Value,
    confidence: f32,
    risk_notes: String,
}

async fn emit_context_pack_refresh_proposals(
    pool: &PgPool,
    run_id: Uuid,
    request: &LoopRunRequest,
    loop_id: &str,
    context_pack: &LoopContextPackResponse,
) -> Result<ContextRefreshResult, ApiError> {
    let project = request
        .project
        .as_deref()
        .ok_or_else(|| ApiError::validation(ValidationError::new("project is required")))?;
    let drafts = context_pack_refresh_proposal_drafts(request, context_pack);
    let mut proposal_ids = Vec::new();
    for draft in drafts {
        let create = LoopMemoryProposalCreateRequest {
            project: project.to_string(),
            loop_id: loop_id.to_string(),
            proposal_type: draft.proposal_type,
            run_id: Some(run_id),
            target_memory_id: draft.target_memory_id,
            candidate: draft.candidate,
            evidence: draft.evidence,
            confidence: draft.confidence,
            risk_notes: Some(draft.risk_notes),
        };
        create.validate().map_err(ApiError::validation)?;
        let created = insert_loop_memory_proposal(pool, &create).await?;
        proposal_ids.push(created.proposal.id);
    }
    let stale_count = context_pack
        .pack
        .memories
        .iter()
        .filter(|memory| memory.stale || memory.contradictory)
        .count();
    let summary = format!(
        "Context Pack Refresh produced {} memory proposal(s), {} stale/contradictory warning(s), and a context-pack diff.",
        proposal_ids.len(),
        stale_count + context_pack.pack.warnings.len()
    );
    let output = json!({
        "proposal_ids": proposal_ids,
        "proposal_count": proposal_ids.len(),
        "stale_or_contradictory_memory_count": stale_count,
        "warning_count": context_pack.pack.warnings.len(),
        "context_pack_id": context_pack.pack.id,
        "context_diff": context_pack.diff,
    });
    append_loop_trace(
        pool,
        run_id,
        "context_refresh",
        "Context Pack Refresh proposals",
        output.clone(),
        false,
    )
    .await?;
    Ok(ContextRefreshResult { summary, output })
}

fn context_pack_refresh_proposal_drafts(
    request: &LoopRunRequest,
    context_pack: &LoopContextPackResponse,
) -> Vec<ContextRefreshProposalDraft> {
    let mut drafts = vec![
        architecture_summary_draft(request, context_pack),
        command_list_draft(request),
        conventions_draft(request),
        module_map_draft(request),
    ];
    if let Some(stale) = stale_memory_warnings_draft(context_pack) {
        drafts.push(stale);
    }
    drafts
}

fn architecture_summary_draft(
    request: &LoopRunRequest,
    context_pack: &LoopContextPackResponse,
) -> ContextRefreshProposalDraft {
    let overview = repo_file_excerpt(
        request.repo_root.as_deref(),
        "docs/developer/architecture/overview.md",
    )
    .unwrap_or_else(|| "Architecture overview file was not available.".to_string());
    let code_map = repo_file_excerpt(
        request.repo_root.as_deref(),
        "docs/developer/architecture/code-map.md",
    )
    .unwrap_or_else(|| "Code map file was not available.".to_string());
    let selected = context_pack
        .pack
        .memories
        .iter()
        .take(8)
        .map(|memory| format!("- [{}] {}", memory.memory_type, memory.summary))
        .collect::<Vec<_>>()
        .join("\n");
    ContextRefreshProposalDraft {
        proposal_type: "add".to_string(),
        candidate: json!({
            "canonical_text": format!(
                "# Context Pack Architecture Summary\n\n## Architecture Overview\n{}\n\n## Code Map\n{}\n\n## Selected Memory Context\n{}",
                overview, code_map, if selected.is_empty() { "- No memories selected.".to_string() } else { selected }
            ),
            "summary": "Context Pack Refresh architecture summary",
            "memory_type": "architecture",
            "tags": ["loop-engineering", "context-pack-refresh", "architecture"]
        }),
        target_memory_id: None,
        evidence: evidence_refs(&[
            ("docs/developer/architecture/overview.md", &overview),
            ("docs/developer/architecture/code-map.md", &code_map),
        ]),
        confidence: 0.78,
        risk_notes:
            "Generated architecture summary should be reviewed before becoming durable memory."
                .to_string(),
    }
}

fn command_list_draft(request: &LoopRunRequest) -> ContextRefreshProposalDraft {
    let contributing = repo_file_excerpt(request.repo_root.as_deref(), "CONTRIBUTING.md")
        .unwrap_or_else(|| "CONTRIBUTING.md was not available.".to_string());
    let package_json = repo_file_excerpt(request.repo_root.as_deref(), "web/package.json")
        .unwrap_or_else(|| "web/package.json was not available.".to_string());
    let cargo = repo_file_excerpt(request.repo_root.as_deref(), "Cargo.toml")
        .unwrap_or_else(|| "Cargo.toml was not available.".to_string());
    ContextRefreshProposalDraft {
        proposal_type: "add".to_string(),
        candidate: json!({
            "canonical_text": format!(
                "# Context Pack Command List\n\n## Validation Guidance\n{}\n\n## Rust Workspace\n{}\n\n## Web Scripts\n{}",
                contributing, cargo, package_json
            ),
            "summary": "Context Pack Refresh command list",
            "memory_type": "documentation",
            "tags": ["loop-engineering", "context-pack-refresh", "commands"]
        }),
        target_memory_id: None,
        evidence: evidence_refs(&[
            ("CONTRIBUTING.md", &contributing),
            ("Cargo.toml", &cargo),
            ("web/package.json", &package_json),
        ]),
        confidence: 0.76,
        risk_notes: "Generated command list should be checked for obsolete or environment-specific commands.".to_string(),
    }
}

fn conventions_draft(request: &LoopRunRequest) -> ContextRefreshProposalDraft {
    let agents = repo_file_excerpt(request.repo_root.as_deref(), "AGENTS.md")
        .unwrap_or_else(|| "AGENTS.md was not available.".to_string());
    let contributing = repo_file_excerpt(request.repo_root.as_deref(), "CONTRIBUTING.md")
        .unwrap_or_else(|| "CONTRIBUTING.md was not available.".to_string());
    ContextRefreshProposalDraft {
        proposal_type: "add".to_string(),
        candidate: json!({
            "canonical_text": format!(
                "# Context Pack Conventions\n\n## Agent Instructions\n{}\n\n## Contribution Expectations\n{}",
                agents, contributing
            ),
            "summary": "Context Pack Refresh conventions",
            "memory_type": "convention",
            "tags": ["loop-engineering", "context-pack-refresh", "conventions"]
        }),
        target_memory_id: None,
        evidence: evidence_refs(&[("AGENTS.md", &agents), ("CONTRIBUTING.md", &contributing)]),
        confidence: 0.80,
        risk_notes: "Generated conventions should be reviewed because instruction docs can contain broad guidance.".to_string(),
    }
}

fn module_map_draft(request: &LoopRunRequest) -> ContextRefreshProposalDraft {
    let code_map = repo_file_excerpt(
        request.repo_root.as_deref(),
        "docs/developer/architecture/code-map.md",
    )
    .unwrap_or_else(|| "Code map file was not available.".to_string());
    let cargo = repo_file_excerpt(request.repo_root.as_deref(), "Cargo.toml")
        .unwrap_or_else(|| "Cargo.toml was not available.".to_string());
    ContextRefreshProposalDraft {
        proposal_type: "add".to_string(),
        candidate: json!({
            "canonical_text": format!(
                "# Context Pack Module Map\n\n## Code Map\n{}\n\n## Workspace Manifest\n{}",
                code_map, cargo
            ),
            "summary": "Context Pack Refresh module map",
            "memory_type": "architecture",
            "tags": ["loop-engineering", "context-pack-refresh", "module-map"]
        }),
        target_memory_id: None,
        evidence: evidence_refs(&[
            ("docs/developer/architecture/code-map.md", &code_map),
            ("Cargo.toml", &cargo),
        ]),
        confidence: 0.78,
        risk_notes: "Generated module map should be reviewed before replacing or augmenting durable architecture memory.".to_string(),
    }
}

fn stale_memory_warnings_draft(
    context_pack: &LoopContextPackResponse,
) -> Option<ContextRefreshProposalDraft> {
    let flagged = context_pack
        .pack
        .memories
        .iter()
        .filter(|memory| memory.stale || memory.contradictory)
        .map(|memory| {
            format!(
                "- {} [{}] stale={} contradictory={} freshness={}",
                memory.summary,
                memory.memory_id,
                memory.stale,
                memory.contradictory,
                memory.freshness
            )
        })
        .collect::<Vec<_>>();
    if flagged.is_empty() && context_pack.pack.warnings.is_empty() {
        return None;
    }
    let warnings = context_pack
        .pack
        .warnings
        .iter()
        .map(|warning| format!("- {warning}"))
        .collect::<Vec<_>>()
        .join("\n");
    Some(ContextRefreshProposalDraft {
        proposal_type: "add".to_string(),
        candidate: json!({
            "canonical_text": format!(
                "# Context Pack Stale Memory Warnings\n\n## Flagged Memories\n{}\n\n## Pack Warnings\n{}",
                if flagged.is_empty() { "- No stale or contradictory memories flagged.".to_string() } else { flagged.join("\n") },
                if warnings.is_empty() { "- No pack warnings.".to_string() } else { warnings }
            ),
            "summary": "Context Pack Refresh stale memory warnings",
            "memory_type": "debugging",
            "tags": ["loop-engineering", "context-pack-refresh", "stale-memory"]
        }),
        target_memory_id: None,
        evidence: json!([{
            "source_kind": "note",
            "excerpt": "Context-pack builder flagged stale, contradictory, excluded, or warning-producing context."
        }]),
        confidence: 0.72,
        risk_notes: "Generated stale-memory warning should be checked before durable cleanup work."
            .to_string(),
    })
}

fn repo_file_excerpt(repo_root: Option<&str>, relative: &str) -> Option<String> {
    let path = FsPath::new(repo_root?).join(relative);
    let contents = fs::read_to_string(path).ok()?;
    let excerpt = contents.lines().take(80).collect::<Vec<_>>().join("\n");
    Some(excerpt.chars().take(3_000).collect())
}

fn evidence_refs(items: &[(&str, &str)]) -> serde_json::Value {
    serde_json::Value::Array(
        items
            .iter()
            .filter(|(_, excerpt)| !excerpt.is_empty())
            .map(|(path, excerpt)| {
                json!({
                    "source_kind": "file",
                    "file_path": path,
                    "excerpt": excerpt.chars().take(600).collect::<String>()
                })
            })
            .collect(),
    )
}

struct MemoryHygieneResult {
    summary: String,
    output: serde_json::Value,
}

async fn emit_memory_hygiene_proposals(
    pool: &PgPool,
    run_id: Uuid,
    request: &LoopRunRequest,
    loop_id: &str,
) -> Result<MemoryHygieneResult, ApiError> {
    let project = request
        .project
        .as_deref()
        .ok_or_else(|| ApiError::validation(ValidationError::new("project is required")))?;
    let memories = crate::repository::fetch_project_memories(pool, project, Some("active"), 200, 0)
        .await
        .map_err(ApiError::sql)?
        .items;
    let drafts = memory_hygiene_proposal_drafts(&memories);
    let mut proposal_ids = Vec::new();
    for draft in drafts {
        let create = LoopMemoryProposalCreateRequest {
            project: project.to_string(),
            loop_id: loop_id.to_string(),
            proposal_type: draft.proposal_type,
            run_id: Some(run_id),
            target_memory_id: draft.target_memory_id,
            candidate: draft.candidate,
            evidence: draft.evidence,
            confidence: draft.confidence,
            risk_notes: Some(draft.risk_notes),
        };
        create.validate().map_err(ApiError::validation)?;
        let created = insert_loop_memory_proposal(pool, &create).await?;
        proposal_ids.push(created.proposal.id);
    }
    let summary = if proposal_ids.is_empty() {
        "Memory Hygiene found no duplicate, stale, contradictory, or low-confidence candidates in the inspected sample."
            .to_string()
    } else {
        format!(
            "Memory Hygiene produced {} reviewable cleanup proposal(s).",
            proposal_ids.len()
        )
    };
    let output = json!({
        "proposal_ids": proposal_ids,
        "proposal_count": proposal_ids.len(),
        "inspected_memory_count": memories.len(),
        "sensitive_content_policy": "summaries_and_ids_only"
    });
    append_loop_trace(
        pool,
        run_id,
        "memory_hygiene",
        "Memory Hygiene proposals",
        output.clone(),
        false,
    )
    .await?;
    Ok(MemoryHygieneResult { summary, output })
}

fn memory_hygiene_proposal_drafts(
    memories: &[ProjectMemoryListItem],
) -> Vec<ContextRefreshProposalDraft> {
    let mut drafts = Vec::new();
    drafts.extend(duplicate_memory_drafts(memories));
    drafts.extend(stale_or_low_confidence_drafts(memories));
    drafts.extend(related_memory_link_drafts(memories));
    drafts.truncate(12);
    drafts
}

fn duplicate_memory_drafts(memories: &[ProjectMemoryListItem]) -> Vec<ContextRefreshProposalDraft> {
    let mut by_summary: BTreeMap<String, Vec<&ProjectMemoryListItem>> = BTreeMap::new();
    for memory in memories {
        let key = normalize_hygiene_text(&memory.summary);
        if key.len() >= 12 {
            by_summary.entry(key).or_default().push(memory);
        }
    }
    by_summary
        .values()
        .filter(|group| group.len() > 1)
        .take(4)
        .filter_map(|group| {
            let mut ordered = group.clone();
            ordered.sort_by(|a, b| {
                b.confidence
                    .partial_cmp(&a.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(b.importance.cmp(&a.importance))
            });
            let keep = ordered.first()?;
            let duplicate = ordered.get(1)?;
            Some(ContextRefreshProposalDraft {
                proposal_type: "merge".to_string(),
                candidate: json!({
                    "related_memory_id": keep.id,
                    "relation_type": "duplicates",
                    "summary": format!("Merge likely duplicate memory into {}", keep.id),
                    "evidence_summary": format!(
                        "Duplicate summaries: target `{}` and related `{}`.",
                        duplicate.summary, keep.summary
                    )
                }),
                evidence: json!([{
                    "source_kind": "note",
                    "excerpt": format!(
                        "Likely duplicate summaries. Target: {} (confidence {:.2}, importance {}). Related: {} (confidence {:.2}, importance {}).",
                        duplicate.id, duplicate.confidence, duplicate.importance,
                        keep.id, keep.confidence, keep.importance
                    )
                }]),
                confidence: 0.74,
                risk_notes: "Duplicate detection uses summary normalization only; review before approving merge relation.".to_string(),
                target_memory_id: Some(duplicate.id),
            })
        })
        .collect()
}

fn stale_or_low_confidence_drafts(
    memories: &[ProjectMemoryListItem],
) -> Vec<ContextRefreshProposalDraft> {
    let stale_before = chrono::Utc::now() - chrono::Duration::days(180);
    memories
        .iter()
        .filter(|memory| {
            memory.confidence < 0.45
                || (memory.updated_at < stale_before && memory.importance <= 2)
        })
        .take(5)
        .map(|memory| {
            let stale = memory.updated_at < stale_before;
            ContextRefreshProposalDraft {
                proposal_type: "deprecate".to_string(),
                candidate: json!({
                    "summary": format!("Deprecate low-signal memory {}", memory.id),
                    "reason": if stale { "stale_low_importance" } else { "low_confidence" },
                    "memory_summary": memory.summary,
                    "confidence": memory.confidence,
                    "importance": memory.importance,
                    "updated_at": memory.updated_at
                }),
                evidence: json!([{
                    "source_kind": "note",
                    "excerpt": format!(
                        "Memory {} summary `{}` has confidence {:.2}, importance {}, updated at {}.",
                        memory.id, memory.summary, memory.confidence, memory.importance, memory.updated_at
                    )
                }]),
                confidence: if stale { 0.68 } else { 0.64 },
                risk_notes: "Deprecation proposal uses metadata and summary only; inspect full memory before approval.".to_string(),
                target_memory_id: Some(memory.id),
            }
        })
        .collect()
}

fn related_memory_link_drafts(
    memories: &[ProjectMemoryListItem],
) -> Vec<ContextRefreshProposalDraft> {
    let mut drafts = Vec::new();
    for (index, left) in memories.iter().enumerate() {
        if drafts.len() >= 3 {
            break;
        }
        let Some(shared_tag) = left.tags.first() else {
            continue;
        };
        let Some(right) = memories.iter().skip(index + 1).find(|candidate| {
            candidate.memory_type == left.memory_type
                && candidate.tags.iter().any(|tag| tag == shared_tag)
                && normalize_hygiene_text(&candidate.summary)
                    != normalize_hygiene_text(&left.summary)
        }) else {
            continue;
        };
        drafts.push(ContextRefreshProposalDraft {
            proposal_type: "link".to_string(),
            candidate: json!({
                "related_memory_id": right.id,
                "relation_type": "related_to",
                "summary": format!("Link related memories sharing tag `{shared_tag}`"),
                "shared_tag": shared_tag,
                "left_summary": left.summary,
                "right_summary": right.summary
            }),
            evidence: json!([{
                "source_kind": "note",
                "excerpt": format!(
                    "Memories {} and {} share type {} and tag `{}`.",
                    left.id, right.id, left.memory_type, shared_tag
                )
            }]),
            confidence: 0.60,
            risk_notes:
                "Related-memory link is low-risk but should still be reviewed for usefulness."
                    .to_string(),
            target_memory_id: Some(left.id),
        });
    }
    drafts
}

fn normalize_hygiene_text(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_alphanumeric() || ch.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

struct CiTriageResult {
    summary: String,
    output: serde_json::Value,
}

async fn emit_ci_failure_triage_report(
    pool: &PgPool,
    run_id: Uuid,
    request: &LoopRunRequest,
    loop_id: &str,
    mode: &LoopMode,
    context_pack: &LoopContextPackResponse,
) -> Result<CiTriageResult, ApiError> {
    let project = request
        .project
        .as_deref()
        .ok_or_else(|| ApiError::validation(ValidationError::new("project is required")))?;
    let ci_text = ci_payload_text(request.trigger_payload.as_ref());
    let classification = classify_ci_failure(&ci_text);
    let memory_evidence = context_pack
        .pack
        .memories
        .iter()
        .take(5)
        .map(|memory| {
            json!({
                "memory_id": memory.memory_id,
                "summary": memory.summary,
                "memory_type": memory.memory_type,
                "confidence": memory.confidence,
                "freshness": memory.freshness
            })
        })
        .collect::<Vec<_>>();
    let diagnosis = ci_triage_diagnosis(&classification, &ci_text, &memory_evidence);
    let mut proposal_id = None;
    if *mode == LoopMode::SuggestOnly && classification.follow_up_suitable {
        let create = LoopMemoryProposalCreateRequest {
            project: project.to_string(),
            loop_id: loop_id.to_string(),
            proposal_type: "add".to_string(),
            run_id: Some(run_id),
            target_memory_id: None,
            candidate: json!({
                "canonical_text": format!(
                    "# CI Failure Follow-up Task\n\nClassification: {}\nConfidence: {:.2}\n\n{}\n\nNo code changes were made by the triage loop.",
                    classification.kind, classification.confidence, diagnosis
                ),
                "summary": format!("CI triage follow-up: {}", classification.kind),
                "memory_type": "task",
                "tags": ["loop-engineering", "ci-triage", "follow-up-task"]
            }),
            evidence: json!([{
                "source_kind": "command_output",
                "excerpt": ci_text.chars().take(1_200).collect::<String>()
            }]),
            confidence: classification.confidence,
            risk_notes: Some(
                "Follow-up task proposal from CI triage; review before routing to Draft PR."
                    .to_string(),
            ),
        };
        create.validate().map_err(ApiError::validation)?;
        let created = insert_loop_memory_proposal(pool, &create).await?;
        proposal_id = Some(created.proposal.id);
    }
    let summary = format!(
        "CI Failure Triage classified failure as {} with confidence {:.2}.",
        classification.kind, classification.confidence
    );
    let output = json!({
        "classification": classification.kind,
        "confidence": classification.confidence,
        "diagnosis": diagnosis,
        "evidence_excerpt": ci_text.chars().take(1_200).collect::<String>(),
        "memory_evidence": memory_evidence,
        "follow_up_proposal_id": proposal_id,
        "mode": mode.as_str(),
        "code_written": false
    });
    append_loop_trace(
        pool,
        run_id,
        "ci_triage",
        "CI Failure Triage report",
        output.clone(),
        false,
    )
    .await?;
    Ok(CiTriageResult { summary, output })
}

struct CiFailureClassification {
    kind: &'static str,
    confidence: f32,
    follow_up_suitable: bool,
}

fn classify_ci_failure(text: &str) -> CiFailureClassification {
    let lower = text.to_lowercase();
    if contains_any(
        &lower,
        &[
            "timeout",
            "timed out",
            "connection reset",
            "network",
            "rate limit",
        ],
    ) {
        return CiFailureClassification {
            kind: "environmental",
            confidence: 0.74,
            follow_up_suitable: false,
        };
    }
    if contains_any(
        &lower,
        &[
            "lockfile",
            "dependency",
            "version conflict",
            "unresolved import",
            "package not found",
        ],
    ) {
        return CiFailureClassification {
            kind: "dependency-related",
            confidence: 0.72,
            follow_up_suitable: true,
        };
    }
    if contains_any(
        &lower,
        &["flaky", "intermittent", "rerun", "race condition"],
    ) {
        return CiFailureClassification {
            kind: "flaky",
            confidence: 0.68,
            follow_up_suitable: false,
        };
    }
    if contains_any(
        &lower,
        &[
            "assertion",
            "test failed",
            "expected",
            "panic",
            "compile error",
            "type error",
        ],
    ) {
        return CiFailureClassification {
            kind: "likely regression",
            confidence: 0.76,
            follow_up_suitable: true,
        };
    }
    CiFailureClassification {
        kind: "unknown",
        confidence: 0.45,
        follow_up_suitable: false,
    }
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn ci_payload_text(payload: Option<&serde_json::Value>) -> String {
    let Some(payload) = payload else {
        return "No CI payload was attached to the loop run.".to_string();
    };
    let mut parts = Vec::new();
    collect_ci_payload_strings(payload, &mut parts);
    if parts.is_empty() {
        serde_json::to_string_pretty(payload)
            .unwrap_or_else(|_| "Unprintable CI payload.".to_string())
    } else {
        parts.join("\n")
    }
}

fn collect_ci_payload_strings(value: &serde_json::Value, parts: &mut Vec<String>) {
    match value {
        serde_json::Value::String(value) => {
            if value.len() > 3 {
                parts.push(value.chars().take(2_000).collect());
            }
        }
        serde_json::Value::Array(items) => {
            for item in items.iter().take(20) {
                collect_ci_payload_strings(item, parts);
            }
        }
        serde_json::Value::Object(object) => {
            for key in [
                "workflow",
                "job",
                "step",
                "status",
                "conclusion",
                "error",
                "message",
                "log",
                "logs",
                "stderr",
                "stdout",
            ] {
                if let Some(value) = object.get(key) {
                    collect_ci_payload_strings(value, parts);
                }
            }
        }
        _ => {}
    }
}

fn ci_triage_diagnosis(
    classification: &CiFailureClassification,
    ci_text: &str,
    memory_evidence: &[serde_json::Value],
) -> String {
    let memory_lines = memory_evidence
        .iter()
        .filter_map(|value| value.get("summary").and_then(serde_json::Value::as_str))
        .take(5)
        .map(|summary| format!("- {summary}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "The CI evidence matches `{}` signals. Review the attached log excerpt first, then compare against the relevant memories below.\n\nRelevant memories:\n{}",
        classification.kind,
        if memory_lines.is_empty() {
            "- No relevant memories were selected.".to_string()
        } else {
            memory_lines
        }
    ) + &format!(
        "\n\nLog excerpt:\n{}",
        ci_text.chars().take(800).collect::<String>()
    )
}

struct IssueTriageResult {
    summary: String,
    output: serde_json::Value,
}

async fn emit_agent_ready_issue_triage_report(
    pool: &PgPool,
    run_id: Uuid,
    request: &LoopRunRequest,
    loop_id: &str,
    mode: &LoopMode,
    context_pack: &LoopContextPackResponse,
) -> Result<IssueTriageResult, ApiError> {
    let project = request
        .project
        .as_deref()
        .ok_or_else(|| ApiError::validation(ValidationError::new("project is required")))?;
    let issue = issue_payload_text(request.trigger_payload.as_ref());
    let classification = classify_agent_ready_issue(&issue);
    let likely_files = likely_issue_files(&issue);
    let test_strategy = issue_test_strategy(&issue);
    let memory_evidence = context_pack
        .pack
        .memories
        .iter()
        .take(5)
        .map(|memory| {
            json!({
                "memory_id": memory.memory_id,
                "summary": memory.summary,
                "memory_type": memory.memory_type,
                "confidence": memory.confidence
            })
        })
        .collect::<Vec<_>>();
    let mut proposal_id = None;
    if *mode == LoopMode::SuggestOnly && classification.agent_ready {
        let create = LoopMemoryProposalCreateRequest {
            project: project.to_string(),
            loop_id: loop_id.to_string(),
            proposal_type: "add".to_string(),
            run_id: Some(run_id),
            target_memory_id: None,
            candidate: json!({
                "canonical_text": format!(
                    "# Agent-Ready Task Pack\n\nIssue:\n{}\n\nLikely files:\n{}\n\nTest strategy:\n{}\n\nSuggested labels: {}",
                    issue,
                    likely_files.join("\n"),
                    test_strategy,
                    classification.labels.join(", ")
                ),
                "summary": "Agent-ready issue task pack",
                "memory_type": "task",
                "tags": ["loop-engineering", "agent-ready", "task-pack"]
            }),
            evidence: json!([{
                "source_kind": "note",
                "excerpt": issue.chars().take(1_200).collect::<String>()
            }]),
            confidence: classification.confidence,
            risk_notes: Some(
                "Task pack proposal from issue triage; review before routing to Draft PR."
                    .to_string(),
            ),
        };
        create.validate().map_err(ApiError::validation)?;
        let created = insert_loop_memory_proposal(pool, &create).await?;
        proposal_id = Some(created.proposal.id);
    }
    let summary = format!(
        "Agent-Ready Issue Triage suggests {} with risk {} and ambiguity {}.",
        classification.labels.join(", "),
        classification.risk,
        classification.ambiguity
    );
    let output = json!({
        "ambiguity": classification.ambiguity,
        "implementation_risk": classification.risk,
        "suggested_labels": classification.labels,
        "missing_context": classification.missing_context,
        "likely_files": likely_files,
        "test_strategy": test_strategy,
        "task_pack_proposal_id": proposal_id,
        "memory_evidence": memory_evidence,
        "mode": mode.as_str()
    });
    append_loop_trace(
        pool,
        run_id,
        "issue_triage",
        "Agent-Ready Issue Triage report",
        output.clone(),
        false,
    )
    .await?;
    Ok(IssueTriageResult { summary, output })
}

struct IssueClassification {
    ambiguity: &'static str,
    risk: &'static str,
    labels: Vec<&'static str>,
    missing_context: Vec<&'static str>,
    confidence: f32,
    agent_ready: bool,
}

fn classify_agent_ready_issue(issue: &str) -> IssueClassification {
    let lower = issue.to_lowercase();
    let mut missing = Vec::new();
    if !contains_any(
        &lower,
        &["expected", "actual", "reproduce", "acceptance", "should"],
    ) {
        missing.push("acceptance_criteria");
    }
    if contains_any(
        &lower,
        &[
            "auth",
            "billing",
            "security",
            "migration",
            "secret",
            "delete",
        ],
    ) {
        return IssueClassification {
            ambiguity: if missing.is_empty() { "medium" } else { "high" },
            risk: "high",
            labels: vec!["needs-design", "needs-human-clarification"],
            missing_context: missing,
            confidence: 0.70,
            agent_ready: false,
        };
    }
    if !missing.is_empty() || issue.trim().len() < 80 {
        return IssueClassification {
            ambiguity: "high",
            risk: "medium",
            labels: vec!["needs-human-clarification"],
            missing_context: missing,
            confidence: 0.68,
            agent_ready: false,
        };
    }
    IssueClassification {
        ambiguity: "low",
        risk: "low",
        labels: vec!["agent-ready"],
        missing_context: Vec::new(),
        confidence: 0.78,
        agent_ready: true,
    }
}

fn issue_payload_text(payload: Option<&serde_json::Value>) -> String {
    let Some(payload) = payload else {
        return "No issue payload was attached to the loop run.".to_string();
    };
    let mut parts = Vec::new();
    collect_issue_payload_strings(payload, &mut parts);
    if parts.is_empty() {
        serde_json::to_string_pretty(payload)
            .unwrap_or_else(|_| "Unprintable issue payload.".to_string())
    } else {
        parts.join("\n\n")
    }
}

fn collect_issue_payload_strings(value: &serde_json::Value, parts: &mut Vec<String>) {
    match value {
        serde_json::Value::String(value) => {
            if value.len() > 3 {
                parts.push(value.chars().take(2_000).collect());
            }
        }
        serde_json::Value::Array(items) => {
            for item in items.iter().take(20) {
                collect_issue_payload_strings(item, parts);
            }
        }
        serde_json::Value::Object(object) => {
            for key in [
                "identifier",
                "title",
                "description",
                "body",
                "labels",
                "comments",
            ] {
                if let Some(value) = object.get(key) {
                    collect_issue_payload_strings(value, parts);
                }
            }
        }
        _ => {}
    }
}

fn likely_issue_files(issue: &str) -> Vec<String> {
    let lower = issue.to_lowercase();
    let mut files = Vec::new();
    if contains_any(&lower, &["web", "browser", "ui", "tab"]) {
        files.push("web/src/".to_string());
    }
    if contains_any(&lower, &["cli", "command", "flag"]) {
        files.push("crates/mem-cli/src/commands/".to_string());
    }
    if contains_any(&lower, &["service", "api", "route", "database"]) {
        files.push("crates/mem-service/src/".to_string());
    }
    if contains_any(&lower, &["memory", "query", "search", "retrieval"]) {
        files.push("crates/mem-search/src/".to_string());
    }
    if files.is_empty() {
        files.push("Inspect repo map before implementation.".to_string());
    }
    files
}

fn issue_test_strategy(issue: &str) -> String {
    let lower = issue.to_lowercase();
    if contains_any(&lower, &["web", "ui", "browser"]) {
        "Add or update Vitest component coverage and run `npm --prefix web run test && npm --prefix web run build`.".to_string()
    } else if contains_any(&lower, &["service", "api", "database", "route"]) {
        "Add service/repository tests and run focused cargo tests for mem-service.".to_string()
    } else {
        "Run focused cargo tests for touched crates and add regression coverage for changed behavior.".to_string()
    }
}

struct DraftPrResult {
    status: LoopRunStatus,
    summary: String,
    output: serde_json::Value,
    blocked_reason: Option<String>,
}

async fn emit_draft_pr_loop_report(
    pool: &PgPool,
    run_id: Uuid,
    request: &LoopRunRequest,
    loop_id: &str,
    mode: &LoopMode,
    context_pack: &LoopContextPackResponse,
) -> Result<DraftPrResult, ApiError> {
    let project = request
        .project
        .as_deref()
        .ok_or_else(|| ApiError::validation(ValidationError::new("project is required")))?;
    let project_id = upsert_project_slug(pool, project)
        .await
        .map_err(ApiError::sql)?;
    let issue = issue_payload_text(request.trigger_payload.as_ref());
    let lower = issue.to_lowercase();
    let sensitive = contains_any(
        &lower,
        &[
            "auth",
            "billing",
            "security",
            "migration",
            "infrastructure",
            "secret",
            "delete",
        ],
    );
    let approved = payload_truthy(
        request.trigger_payload.as_ref(),
        &["approved", "loop_approved", "explicit_user_approval"],
    );
    let agent_ready = contains_any(&lower, &["agent-ready", "agent_ready"]);
    if sensitive || !approved || !agent_ready {
        let reason = if sensitive {
            "sensitive_area_requires_approval"
        } else if !agent_ready {
            "missing_agent_ready_label"
        } else {
            "missing_explicit_issue_approval"
        };
        let approval = create_loop_run_approval(
            pool,
            Some(project_id),
            run_id,
            loop_id,
            LoopActionKind::WriteRepo,
            json!({
                "draft_pr": true,
                "issue": issue,
                "required_label": "agent-ready",
                "requires_explicit_issue_approval": true,
                "auto_merge": false
            }),
            "Draft PR loop can write repo changes and requires approved low-risk issue scope.",
            request.reason.as_deref(),
        )
        .await?;
        let output = json!({
            "gate": {
                "agent_ready": agent_ready,
                "approved": approved,
                "sensitive": sensitive,
                "blocked_reason": reason,
                "approval_id": approval.id
            },
            "draft_pr": null,
            "auto_merge": false
        });
        append_loop_trace(
            pool,
            run_id,
            "draft_pr_gate",
            "Draft PR gate blocked",
            output.clone(),
            false,
        )
        .await?;
        return Ok(DraftPrResult {
            status: LoopRunStatus::Blocked,
            summary: "Draft PR loop is waiting for approval before writing code.".to_string(),
            output,
            blocked_reason: Some(reason.to_string()),
        });
    }

    let repo_root = request
        .repo_root
        .as_deref()
        .ok_or_else(|| ApiError::validation(ValidationError::new("repo_root is required")))?;
    let manager = WorktreeSandboxManager::default();
    let workspace = manager
        .create_workspace(&mem_loops::SandboxWorkspaceSpec {
            project: project.to_string(),
            repo_root: PathBuf::from(repo_root),
            run_id,
            base_ref: None,
        })
        .map_err(|error| ApiError::io(anyhow::anyhow!("create draft PR worktree: {error}")))?;
    let task_pack = RunnerTaskPack {
        title: draft_pr_issue_title(request.trigger_payload.as_ref()),
        prompt: issue.clone(),
        acceptance_criteria: draft_pr_acceptance_criteria(request.trigger_payload.as_ref()),
        metadata: json!({ "expected_changed_file": "draft-pr-plan.md" }),
    };
    let runner_result = invoke_runner_with_policy(
        &MockLoopRunner::success("mock-draft-pr"),
        RunnerInvocation {
            runner_id: "mock-draft-pr".to_string(),
            task_pack,
            context_pack: context_pack.pack.clone(),
            capability_profile: RunnerCapabilityProfile {
                can_read_repo: true,
                can_write_repo: true,
                can_run_commands: true,
                can_propose_memory: true,
                allowed_commands: draft_pr_allowed_commands(request.trigger_payload.as_ref()),
            },
            workspace: RunnerWorkspaceRef {
                repo_root: repo_root.to_string(),
                worktree_path: Some(workspace.worktree_path.display().to_string()),
                branch: Some(workspace.branch.clone()),
            },
            budget: RunnerBudget {
                max_seconds: 600,
                max_tokens: 20_000,
                max_cost_usd: 2.0,
            },
            mode: mode.clone(),
        },
    );
    let checks = run_draft_pr_checks(&manager, &workspace, request.trigger_payload.as_ref())?;
    let output = json!({
        "gate": {
            "agent_ready": true,
            "approved": true,
            "sensitive": false
        },
        "workspace": workspace.runner_workspace_ref(),
        "runner": runner_result,
        "checks": checks,
        "draft_pr": {
            "mode": "draft_only",
            "branch": workspace.branch,
            "worktree_path": workspace.worktree_path,
            "url": null,
            "auto_merge": false,
            "open_pr_requires_external_git_host_adapter": true
        }
    });
    append_loop_trace(
        pool,
        run_id,
        "draft_pr",
        "Draft PR gated execution",
        output.clone(),
        false,
    )
    .await?;
    Ok(DraftPrResult {
        status: LoopRunStatus::Succeeded,
        summary: "Draft PR loop prepared an isolated branch and draft-only PR payload.".to_string(),
        output,
        blocked_reason: None,
    })
}

fn payload_truthy(payload: Option<&serde_json::Value>, keys: &[&str]) -> bool {
    let Some(payload) = payload else {
        return false;
    };
    match payload {
        serde_json::Value::Bool(value) => *value,
        serde_json::Value::Array(items) => {
            items.iter().any(|item| payload_truthy(Some(item), keys))
        }
        serde_json::Value::Object(object) => object.iter().any(|(key, value)| {
            (keys.iter().any(|candidate| key == candidate)
                && matches!(value, serde_json::Value::Bool(true)))
                || payload_truthy(Some(value), keys)
        }),
        _ => false,
    }
}

fn draft_pr_issue_title(payload: Option<&serde_json::Value>) -> String {
    payload
        .and_then(|payload| payload.get("title"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Draft PR loop task")
        .to_string()
}

fn draft_pr_acceptance_criteria(payload: Option<&serde_json::Value>) -> Vec<String> {
    payload
        .and_then(|payload| payload.get("acceptance_criteria"))
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .filter(|items: &Vec<String>| !items.is_empty())
        .unwrap_or_else(|| vec!["Open a draft-only PR for human review.".to_string()])
}

fn draft_pr_allowed_commands(payload: Option<&serde_json::Value>) -> Vec<String> {
    payload
        .and_then(|payload| payload.get("allowed_commands"))
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .filter(|items: &Vec<String>| !items.is_empty())
        .unwrap_or_else(|| vec!["cargo".to_string(), "npm".to_string(), "sh".to_string()])
}

fn run_draft_pr_checks(
    manager: &WorktreeSandboxManager,
    workspace: &mem_loops::SandboxWorkspace,
    payload: Option<&serde_json::Value>,
) -> Result<Vec<serde_json::Value>, ApiError> {
    if !payload_truthy(payload, &["run_checks"]) {
        return Ok(vec![json!({
            "status": "planned",
            "reason": "run_checks was not enabled in the trigger payload"
        })]);
    }
    let checks = payload
        .and_then(|payload| payload.get("checks"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let limits = mem_loops::SandboxLimits {
        allowed_commands: draft_pr_allowed_commands(payload),
        ..mem_loops::SandboxLimits::default()
    };
    checks
        .iter()
        .map(|check| {
            let request = sandbox_command_from_check(check)?;
            let log = manager
                .run_command(workspace, &request, &limits)
                .map_err(|error| ApiError::io(anyhow::anyhow!("run draft PR check: {error}")))?;
            Ok(json!({
                "command": log.command,
                "exit_code": log.exit_code,
                "timed_out": log.timed_out,
                "limit_violations": log.limit_violations,
                "status": if log.exit_code == 0 && !log.timed_out { "passed" } else { "failed" }
            }))
        })
        .collect()
}

fn sandbox_command_from_check(
    value: &serde_json::Value,
) -> Result<mem_loops::SandboxCommandRequest, ApiError> {
    if let Some(command) = value.as_str() {
        let mut parts = command.split_whitespace();
        let Some(program) = parts.next() else {
            return Err(ApiError::validation(ValidationError::new(
                "check command must be non-empty",
            )));
        };
        return Ok(mem_loops::SandboxCommandRequest::new(program, parts));
    }
    let program = value
        .get("program")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| ApiError::validation(ValidationError::new("check.program is required")))?;
    let args = value
        .get("args")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(mem_loops::SandboxCommandRequest::new(program, args))
}

struct ReviewerDriftResult {
    summary: String,
    output: serde_json::Value,
}

async fn emit_reviewer_drift_report(
    pool: &PgPool,
    run_id: Uuid,
    request: &LoopRunRequest,
    loop_id: &str,
    context_pack: &LoopContextPackResponse,
) -> Result<ReviewerDriftResult, ApiError> {
    let project = request
        .project
        .as_deref()
        .ok_or_else(|| ApiError::validation(ValidationError::new("project is required")))?;
    let changed_files = payload_string_array(
        request.trigger_payload.as_ref(),
        &["changed_files", "files", "paths"],
    );
    let expected_paths =
        payload_string_array(request.trigger_payload.as_ref(), &["expected_paths"]);
    let diff = payload_string_field(
        request.trigger_payload.as_ref(),
        &["diff", "patch", "summary"],
    );
    let diff_lower = diff.to_lowercase();
    let relevant_memories = context_pack
        .pack
        .memories
        .iter()
        .filter(|memory| {
            matches!(
                memory.memory_type,
                mem_api::MemoryType::Architecture | mem_api::MemoryType::Convention
            ) || memory
                .source_refs
                .iter()
                .filter_map(|source| source.file_path.as_deref())
                .any(|path| {
                    changed_files
                        .iter()
                        .any(|changed| changed.starts_with(path))
                })
        })
        .take(8)
        .map(|memory| {
            json!({
                "memory_id": memory.memory_id,
                "summary": memory.summary,
                "memory_type": memory.memory_type,
                "confidence": memory.confidence
            })
        })
        .collect::<Vec<_>>();
    let mut findings = Vec::new();
    if !expected_paths.is_empty()
        && changed_files.iter().any(|path| {
            !expected_paths
                .iter()
                .any(|expected| path.starts_with(expected))
        })
    {
        findings.push(json!({
            "kind": "unrelated_changes",
            "severity": "medium",
            "message": "Changed files include paths outside the expected scope."
        }));
    }
    if !changed_files
        .iter()
        .any(|path| path.contains("test") || path.contains("spec"))
        && !diff_lower.contains("test")
    {
        findings.push(json!({
            "kind": "missing_tests",
            "severity": "medium",
            "message": "No test file or test evidence was found in the changed files/diff."
        }));
    }
    if contains_any(
        &diff_lower,
        &["todo", "unwrap()", "panic!", "temporary", "hack"],
    ) {
        findings.push(json!({
            "kind": "hidden_behavior_change",
            "severity": "medium",
            "message": "Diff contains markers that may hide behavior changes or unfinished handling."
        }));
    }
    if contains_any(
        &diff_lower,
        &[
            "auth",
            "billing",
            "security",
            "secret",
            "token",
            "credential",
        ],
    ) {
        findings.push(json!({
            "kind": "security_risk",
            "severity": "high",
            "message": "Diff touches security-sensitive concepts."
        }));
    }
    let architecture_drift = changed_files.iter().any(|path| {
        path.contains("architecture") || path.starts_with("crates/mem-service/src/routes")
    }) || contains_any(
        &diff_lower,
        &["architecture", "public api", "schema", "protocol"],
    );
    if architecture_drift {
        findings.push(json!({
            "kind": "architecture_drift",
            "severity": "medium",
            "message": "Change appears to alter architecture or an externally visible contract."
        }));
    }
    let mut proposal_id = None;
    if architecture_drift
        && payload_truthy(request.trigger_payload.as_ref(), &["architecture_changed"])
    {
        let create = LoopMemoryProposalCreateRequest {
            project: project.to_string(),
            loop_id: loop_id.to_string(),
            proposal_type: "add".to_string(),
            run_id: Some(run_id),
            target_memory_id: None,
            candidate: json!({
                "canonical_text": format!(
                    "# Architecture Drift Update\n\nChanged files:\n{}\n\nDiff summary:\n{}",
                    changed_files.join("\n"),
                    diff.chars().take(1_500).collect::<String>()
                ),
                "summary": "Architecture drift update proposal",
                "memory_type": "architecture",
                "tags": ["loop-engineering", "reviewer-drift"]
            }),
            evidence: json!([{
                "source_kind": "note",
                "excerpt": diff.chars().take(1_000).collect::<String>()
            }]),
            confidence: 0.76,
            risk_notes: Some("Review proposed architecture memory before approval.".to_string()),
        };
        create.validate().map_err(ApiError::validation)?;
        let created = insert_loop_memory_proposal(pool, &create).await?;
        proposal_id = Some(created.proposal.id);
    }
    let output = json!({
        "changed_files": changed_files,
        "expected_paths": expected_paths,
        "relevant_memories": relevant_memories,
        "findings": findings,
        "architecture_memory_proposal_id": proposal_id,
        "code_written": false
    });
    append_loop_trace(
        pool,
        run_id,
        "reviewer_drift",
        "Reviewer / Drift Detection report",
        output.clone(),
        false,
    )
    .await?;
    Ok(ReviewerDriftResult {
        summary: format!(
            "Reviewer / Drift Detection produced {} finding(s).",
            output["findings"].as_array().map_or(0, Vec::len)
        ),
        output,
    })
}

fn payload_string_array(payload: Option<&serde_json::Value>, keys: &[&str]) -> Vec<String> {
    let Some(serde_json::Value::Object(object)) = payload else {
        return Vec::new();
    };
    keys.iter()
        .find_map(|key| object.get(*key))
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn payload_string_field(payload: Option<&serde_json::Value>, keys: &[&str]) -> String {
    let Some(serde_json::Value::Object(object)) = payload else {
        return String::new();
    };
    keys.iter()
        .find_map(|key| object.get(*key))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string()
}

struct SkillMiningResult {
    summary: String,
    output: serde_json::Value,
}

async fn emit_skill_mining_report(
    pool: &PgPool,
    run_id: Uuid,
    request: &LoopRunRequest,
    loop_id: &str,
) -> Result<SkillMiningResult, ApiError> {
    let project = request
        .project
        .as_deref()
        .ok_or_else(|| ApiError::validation(ValidationError::new("project is required")))?;
    let payload = request.trigger_payload.as_ref();
    let successful = payload_truthy(
        payload,
        &["successful", "accepted_pr", "merged", "run_succeeded"],
    );
    let commands = payload_string_array(payload, &["commands", "validation_commands"]);
    let validation = payload_string_array(payload, &["validation", "validation_evidence"]);
    let recipe = payload_string_field(payload, &["recipe", "summary", "description"]);
    let source_run = payload_string_field(payload, &["source_run", "run_id", "pr_url", "url"]);
    let suitable = successful && (!recipe.trim().is_empty() || !commands.is_empty());
    let mut proposal_id = None;
    if suitable {
        let title = payload_string_field(payload, &["title", "skill_title"]);
        let title = if title.trim().is_empty() {
            "Learned development skill".to_string()
        } else {
            title
        };
        let applicability = payload_string_array(payload, &["applicability", "conditions"]);
        let canonical_text = format!(
            "# {title}\n\nApplicability:\n{}\n\nRecipe:\n{}\n\nCommands:\n{}\n\nValidation evidence:\n{}\n\nSource:\n{}",
            bullet_lines(&applicability),
            if recipe.trim().is_empty() {
                "Use the commands and source evidence as the initial recipe."
            } else {
                recipe.as_str()
            },
            bullet_lines(&commands),
            bullet_lines(&validation),
            source_run
        );
        let create = LoopMemoryProposalCreateRequest {
            project: project.to_string(),
            loop_id: loop_id.to_string(),
            proposal_type: "add".to_string(),
            run_id: Some(run_id),
            target_memory_id: None,
            candidate: json!({
                "canonical_text": canonical_text,
                "summary": title,
                "memory_type": "reference",
                "tags": ["loop-engineering", "learned-skill"]
            }),
            evidence: json!([{
                "source_kind": "note",
                "excerpt": source_run
            }]),
            confidence: 0.78,
            risk_notes: Some(
                "Learned skill proposal requires approval before durable use.".to_string(),
            ),
        };
        create.validate().map_err(ApiError::validation)?;
        let created = insert_loop_memory_proposal(pool, &create).await?;
        proposal_id = Some(created.proposal.id);
    }
    let output = json!({
        "suitable": suitable,
        "successful": successful,
        "commands": commands,
        "validation_evidence": validation,
        "source_run": source_run,
        "skill_proposal_id": proposal_id,
        "requires_approval": proposal_id.is_some()
    });
    append_loop_trace(
        pool,
        run_id,
        "skill_mining",
        "Skill Mining report",
        output.clone(),
        false,
    )
    .await?;
    Ok(SkillMiningResult {
        summary: if suitable {
            "Skill Mining created a pending learned-skill proposal.".to_string()
        } else {
            "Skill Mining did not find a suitable reusable recipe.".to_string()
        },
        output,
    })
}

fn bullet_lines(items: &[String]) -> String {
    if items.is_empty() {
        return "- n/a".to_string();
    }
    items
        .iter()
        .map(|item| format!("- {item}"))
        .collect::<Vec<_>>()
        .join("\n")
}

struct MemoryEvalResult {
    summary: String,
    output: serde_json::Value,
}

async fn emit_memory_eval_report(
    pool: &PgPool,
    run_id: Uuid,
    request: &LoopRunRequest,
    context_pack: &LoopContextPackResponse,
) -> Result<MemoryEvalResult, ApiError> {
    let project = request
        .project
        .as_deref()
        .ok_or_else(|| ApiError::validation(ValidationError::new("project is required")))?;
    let included_ids = context_pack
        .pack
        .memories
        .iter()
        .map(|memory| memory.memory_id.to_string())
        .collect::<std::collections::BTreeSet<_>>();
    let expected_ids = golden_expected_memory_ids(request.trigger_payload.as_ref());
    let expected_found = expected_ids
        .iter()
        .filter(|id| included_ids.contains(id.as_str()))
        .count();
    let precision_proxy = if included_ids.is_empty() {
        0.0
    } else {
        expected_found as f64 / included_ids.len() as f64
    };
    let recall_proxy = if expected_ids.is_empty() {
        0.0
    } else {
        expected_found as f64 / expected_ids.len() as f64
    };
    let memory_count = context_pack.pack.memories.len().max(1) as f64;
    let stale_rate = context_pack
        .pack
        .memories
        .iter()
        .filter(|memory| memory.stale)
        .count() as f64
        / memory_count;
    let contradiction_rate = context_pack
        .pack
        .memories
        .iter()
        .filter(|memory| memory.contradictory)
        .count() as f64
        / memory_count;
    let (proposal_total, proposal_approved) = proposal_acceptance_counts(pool, project).await?;
    let accepted_memory_proposal_rate = if proposal_total == 0 {
        0.0
    } else {
        proposal_approved as f64 / proposal_total as f64
    };
    let (run_total, useful_runs, total_cost) = useful_run_counts(pool, project).await?;
    let useful_run_rate = if run_total == 0 {
        0.0
    } else {
        useful_runs as f64 / run_total as f64
    };
    let cost_per_useful_run = if useful_runs == 0 {
        0.0
    } else {
        total_cost / useful_runs as f64
    };
    let baseline = request
        .trigger_payload
        .as_ref()
        .and_then(|payload| payload.get("baseline"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let output = json!({
        "golden_scenarios": request
            .trigger_payload
            .as_ref()
            .and_then(|payload| payload.get("golden_scenarios"))
            .cloned()
            .unwrap_or_else(|| json!([])),
        "metrics": {
            "retrieval_precision_proxy": precision_proxy,
            "retrieval_recall_proxy": recall_proxy,
            "stale_memory_injection_rate": stale_rate,
            "contradiction_rate": contradiction_rate,
            "accepted_memory_proposal_rate": accepted_memory_proposal_rate,
            "useful_run_rate": useful_run_rate,
            "cost_per_useful_run": cost_per_useful_run
        },
        "comparison": {
            "baseline": baseline,
            "context_pack_memory_count": context_pack.pack.memories.len(),
            "context_pack_token_count": context_pack.pack.estimated_tokens,
            "warning_count": context_pack.pack.warnings.len()
        },
        "dashboard": {
            "kind": "internal_run_report",
            "run_id": run_id,
            "project": project
        }
    });
    append_loop_trace(
        pool,
        run_id,
        "memory_eval",
        "Retrieval and Memory Eval report",
        output.clone(),
        false,
    )
    .await?;
    Ok(MemoryEvalResult {
        summary: "Memory Eval produced retrieval/context quality metrics.".to_string(),
        output,
    })
}

fn golden_expected_memory_ids(payload: Option<&serde_json::Value>) -> Vec<String> {
    payload
        .and_then(|payload| payload.get("golden_scenarios"))
        .and_then(serde_json::Value::as_array)
        .map(|scenarios| {
            scenarios
                .iter()
                .flat_map(|scenario| {
                    scenario
                        .get("expected_memory_ids")
                        .and_then(serde_json::Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(serde_json::Value::as_str)
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn proposal_acceptance_counts(pool: &PgPool, project: &str) -> Result<(i64, i64), ApiError> {
    let row = sqlx::query(
        r#"
        SELECT
            COUNT(*)::bigint AS total,
            COUNT(*) FILTER (WHERE mp.status = 'approved')::bigint AS approved
        FROM memory_proposals mp
        JOIN projects p ON p.id = mp.project_id
        WHERE p.slug = $1
        "#,
    )
    .bind(project)
    .fetch_one(pool)
    .await
    .map_err(ApiError::sql)?;
    Ok((
        row.try_get::<i64, _>("total").map_err(ApiError::sql)?,
        row.try_get::<i64, _>("approved").map_err(ApiError::sql)?,
    ))
}

async fn useful_run_counts(pool: &PgPool, project: &str) -> Result<(i64, i64, f64), ApiError> {
    let row = sqlx::query(
        r#"
        SELECT
            COUNT(*)::bigint AS total,
            COUNT(*) FILTER (WHERE lr.status = 'succeeded')::bigint AS useful,
            COALESCE(SUM((lr.cost_json->>'total_usd')::double precision), 0.0) AS cost
        FROM loop_runs lr
        JOIN projects p ON p.id = lr.project_id
        WHERE p.slug = $1
        "#,
    )
    .bind(project)
    .fetch_one(pool)
    .await
    .map_err(ApiError::sql)?;
    Ok((
        row.try_get::<i64, _>("total").map_err(ApiError::sql)?,
        row.try_get::<i64, _>("useful").map_err(ApiError::sql)?,
        row.try_get::<f64, _>("cost").map_err(ApiError::sql)?,
    ))
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
