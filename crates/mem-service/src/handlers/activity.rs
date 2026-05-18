use crate::prelude::*;
use crate::*;

pub(crate) async fn capture_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CaptureTaskRequest>,
) -> Result<Json<mem_api::CaptureTaskResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/capture/task", &request, true).await?,
        ));
    }
    let task_title = request.task_title.clone();
    let project = request.project.clone();
    let response = if request.dry_run {
        preview_capture(state.pool()?, &request)
            .await
            .map_err(ApiError::sql)?
    } else {
        store_capture(state.pool()?, &request)
            .await
            .map_err(ApiError::sql)?
    };
    if request.dry_run {
        return Ok(Json(response));
    }
    notify_project_changed(
        &state,
        project,
        None,
        ActivityKind::CaptureTask,
        format!("Captured task: {task_title}"),
        Some(ActivityDetails::CaptureTask {
            session_id: response.session_id,
            task_id: response.task_id,
            raw_capture_id: response.raw_capture_id,
            idempotency_key: response.idempotency_key.clone(),
            task_title: Some(task_title.clone()),
            writer_id: request.writer_id.clone(),
        }),
    );
    Ok(Json(response))
}

pub(crate) async fn scan_activity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ScanActivityRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/scan/activity", &request, true).await?,
        ));
    }

    let summary = if request.dry_run {
        format!(
            "Scanned repository in dry-run mode and accepted {} candidate memory entry/entries.",
            request.candidate_count
        )
    } else {
        format!(
            "Scanned repository and accepted {} candidate memory entry/entries.",
            request.candidate_count
        )
    };
    notify_project_changed(
        &state,
        request.project.clone(),
        None,
        ActivityKind::Scan,
        summary,
        Some(ActivityDetails::Scan {
            dry_run: request.dry_run,
            candidate_count: request.candidate_count,
            files_considered: request.files_considered,
            commits_considered: request.commits_considered,
            index_reused: request.index_reused,
            report_path: request.report_path.clone(),
            capture_id: request.capture_id.clone(),
            curate_run_id: request.curate_run_id.clone(),
        }),
    );
    Ok(Json(serde_json::json!({ "logged": true })))
}

pub(crate) async fn graph_activity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<GraphActivityRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/graph/activity", &request, true).await?,
        ));
    }

    notify_project_changed(
        &state,
        request.project.clone(),
        None,
        ActivityKind::GraphExtract,
        graph_activity_summary(&request),
        Some(graph_activity_details(&request)),
    );
    Ok(Json(serde_json::json!({ "logged": true })))
}

pub(crate) fn graph_activity_summary(request: &GraphActivityRequest) -> String {
    let verb = if request.reused_existing_run {
        "Reused code graph extraction"
    } else if request.dry_run {
        "Previewed code graph extraction"
    } else {
        "Extracted code graph"
    };
    format!(
        "{verb}: {} symbols, {} references, {} graph edge(s).",
        request.symbol_count, request.reference_count, request.graph_edge_count
    )
}

pub(crate) fn graph_activity_details(request: &GraphActivityRequest) -> ActivityDetails {
    ActivityDetails::GraphExtract {
        repo_root: request.repo_root.clone(),
        git_head: request.git_head.clone(),
        since: request.since.clone(),
        extraction_run_id: request.extraction_run_id,
        dry_run: request.dry_run,
        reused_existing_run: request.reused_existing_run,
        index_reused: request.index_reused,
        analyzer_version: request.analyzer_version.clone(),
        strategy_version: request.strategy_version.clone(),
        symbol_count: request.symbol_count,
        reference_count: request.reference_count,
        resolved_reference_count: request.resolved_reference_count,
        unresolved_reference_count: request.unresolved_reference_count,
        ambiguous_reference_count: request.ambiguous_reference_count,
        graph_node_count: request.graph_node_count,
        graph_edge_count: request.graph_edge_count,
        evidence_count: request.evidence_count,
    }
}

pub(crate) async fn checkpoint_activity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CheckpointActivityRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/checkpoint/activity", &request, true).await?,
        ));
    }

    let summary = if let Some(note) = request.checkpoint.note.as_deref() {
        format!("Saved checkpoint for project {} ({note})", request.project)
    } else {
        format!("Saved checkpoint for project {}", request.project)
    };
    notify_project_changed(
        &state,
        request.project.clone(),
        None,
        ActivityKind::Checkpoint,
        summary,
        Some(ActivityDetails::Checkpoint {
            repo_root: request.checkpoint.repo_root.clone(),
            marked_at: request.checkpoint.marked_at,
            note: request.checkpoint.note.clone(),
            git_branch: request.checkpoint.git_branch.clone(),
            git_head: request.checkpoint.git_head.clone(),
        }),
    );
    Ok(Json(serde_json::json!({ "logged": true })))
}

pub(crate) async fn plan_activity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<PlanActivityRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/plan/activity", &request, true).await?,
        ));
    }

    let remaining_count = request.remaining_items.len();
    let verified_complete = matches!(request.action, PlanActivityAction::FinishVerified);
    let summary = match &request.action {
        PlanActivityAction::Started => {
            format!("Recorded approved plan for execution: {}", request.title)
        }
        PlanActivityAction::Synced => {
            format!("Synced approved plan state: {}", request.title)
        }
        PlanActivityAction::FinishBlocked => format!(
            "Plan completion blocked: {} ({} remaining item(s))",
            request.title, remaining_count
        ),
        PlanActivityAction::FinishVerified => {
            format!("Verified approved plan complete: {}", request.title)
        }
    };
    notify_project_changed(
        &state,
        request.project.clone(),
        None,
        ActivityKind::Plan,
        summary,
        Some(ActivityDetails::Plan {
            action: request.action.clone(),
            title: request.title.clone(),
            thread_key: request.thread_key.clone(),
            total_items: request.total_items,
            completed_items: request.completed_items,
            remaining_items: request.remaining_items.clone(),
            source_path: request.source_path.clone(),
            verified_complete,
        }),
    );
    Ok(Json(serde_json::json!({ "logged": true })))
}
