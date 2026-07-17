use axum::{
    Router,
    routing::{any, delete, get, post},
};
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

use super::{
    AppState, activate_embedding_backend, admin_shutdown, agents_snapshot, approve_loop_approval,
    approve_loop_memory_proposal, archive, archive_memory, build_loop_context_pack,
    cancel_loop_run, capture_task, checkpoint_activity, create_loop_memory_proposal, curate_memory,
    deactivate_embedding_backend, delete_memory, disable_loop, edit_loop_approval,
    edit_loop_memory_proposal, enable_loop, finish_agent_workspace, get_loop_definition,
    get_loop_global_state, get_loop_run, get_loop_run_context_pack, get_memory, get_memory_history,
    graph_activity, healthz, heartbeat_agent_workspace, list_agent_workspaces,
    list_embedding_backends, list_loop_approvals, list_loop_definitions,
    list_loop_memory_proposals, list_loop_runs, llm_audit_status, memory_scores, offline_pending,
    pause_loop, plan_activity, project_activities, project_bundle_export,
    project_bundle_export_preview, project_bundle_import, project_bundle_import_preview,
    project_commit_detail, project_commits, project_graph, project_graph_status, project_memories,
    project_memory_graph, project_overview, project_replacement_policy,
    project_replacement_policy_update, project_replacement_proposal_approve,
    project_replacement_proposal_reject, project_replacement_proposals, project_resume,
    project_structure, project_up_to_speed, prune_embeddings, prune_history, query, query_global,
    read_skill, reembed, reindex, reject_loop_approval, reject_loop_memory_proposal, repair_skills,
    review_validation_run, route_loop_trigger, run_loop, runtime_status, scan_activity,
    set_embedding_creation_enabled, set_llm_audit_enabled, skills, snooze_loop,
    start_agent_workspace, stats, submit_loop_feedback, sync_commits, update_loop_global_state,
    validate_memory, validation_runs, verify_provenance, watcher_heartbeat, watcher_restart_local,
    watcher_unregister, web_auth_token, web_unavailable, websocket,
};

/// The API specification, embedded at compile time so the running service
/// always describes exactly the surface it was built with. A contract test
/// keeps the spec's path inventory in sync with this router.
async fn openapi_spec() -> impl axum::response::IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "application/yaml")],
        include_str!("../../../docs/api/openapi.yaml"),
    )
}

pub(crate) fn build_http_app(state: AppState) -> Router {
    let web_assets = state.web_root.clone();
    let config = state.config.clone();
    let mcp_config = state.config.mcp.clone();
    let mut app = Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/openapi.yaml", get(openapi_spec))
        .route("/ws", get(websocket))
        .route("/v1/web/auth-token", get(web_auth_token))
        .route("/v1/admin/shutdown", post(admin_shutdown))
        .route("/v1/runtime/status", get(runtime_status))
        .route("/v1/offline/pending", get(offline_pending))
        .route("/v1/skills", get(skills))
        .route("/v1/skills/repair", post(repair_skills))
        .route("/v1/skills/{skill_name}", get(read_skill))
        .route("/v1/query", post(query))
        .route("/v1/query/global", post(query_global))
        .route("/v1/checkpoint/activity", post(checkpoint_activity))
        .route("/v1/plan/activity", post(plan_activity))
        .route("/v1/scan/activity", post(scan_activity))
        .route("/v1/graph/activity", post(graph_activity))
        .route("/v1/commits/sync", post(sync_commits))
        .route("/v1/capture/task", post(capture_task))
        .route("/v1/curate", post(curate_memory))
        .route("/v1/provenance/verify", post(verify_provenance))
        .route("/v1/reindex", post(reindex))
        .route("/v1/reembed", post(reembed))
        .route("/v1/prune-embeddings", post(prune_embeddings))
        .route("/v1/embeddings/backends", get(list_embedding_backends))
        .route("/v1/embeddings/activate", post(activate_embedding_backend))
        .route(
            "/v1/embeddings/deactivate",
            post(deactivate_embedding_backend),
        )
        .route(
            "/v1/embeddings/create-enabled",
            post(set_embedding_creation_enabled),
        )
        .route(
            "/v1/config/llm-audit",
            get(llm_audit_status).post(set_llm_audit_enabled),
        )
        .route("/v1/loops", get(list_loop_definitions))
        .route(
            "/v1/loops/global-kill-switch",
            get(get_loop_global_state).post(update_loop_global_state),
        )
        .route("/v1/loops/runs", get(list_loop_runs))
        .route("/v1/loops/runs/{run_id}", get(get_loop_run))
        .route(
            "/v1/loops/runs/{run_id}/context-pack",
            get(get_loop_run_context_pack),
        )
        .route("/v1/loops/runs/{run_id}/cancel", post(cancel_loop_run))
        .route(
            "/v1/loops/runs/{run_id}/feedback",
            post(submit_loop_feedback),
        )
        .route("/v1/loops/approvals", get(list_loop_approvals))
        .route(
            "/v1/loops/memory-proposals",
            get(list_loop_memory_proposals).post(create_loop_memory_proposal),
        )
        .route("/v1/loops/triggers/route", post(route_loop_trigger))
        .route(
            "/v1/loops/approvals/{approval_id}/approve",
            post(approve_loop_approval),
        )
        .route(
            "/v1/loops/approvals/{approval_id}/reject",
            post(reject_loop_approval),
        )
        .route(
            "/v1/loops/approvals/{approval_id}/edit",
            post(edit_loop_approval),
        )
        .route(
            "/v1/loops/memory-proposals/{proposal_id}/approve",
            post(approve_loop_memory_proposal),
        )
        .route(
            "/v1/loops/memory-proposals/{proposal_id}/reject",
            post(reject_loop_memory_proposal),
        )
        .route(
            "/v1/loops/memory-proposals/{proposal_id}/edit",
            post(edit_loop_memory_proposal),
        )
        .route("/v1/loops/{loop_id}", get(get_loop_definition))
        .route("/v1/loops/{loop_id}/enable", post(enable_loop))
        .route("/v1/loops/{loop_id}/disable", post(disable_loop))
        .route("/v1/loops/{loop_id}/pause", post(pause_loop))
        .route("/v1/loops/{loop_id}/snooze", post(snooze_loop))
        .route("/v1/loops/{loop_id}/run", post(run_loop))
        .route(
            "/v1/loops/{loop_id}/context-pack",
            get(build_loop_context_pack),
        )
        .route("/v1/memory/{id}", get(get_memory))
        .route("/v1/memory/{id}/validate", post(validate_memory))
        .route(
            "/v1/validation-runs/{id}/review",
            post(review_validation_run),
        )
        .route("/v1/memory/{id}/archive", post(archive_memory))
        .route("/v1/memory/{id}/history", get(get_memory_history))
        .route("/v1/memory", delete(delete_memory))
        .route("/v1/prune-history", post(prune_history))
        .route("/v1/stats", get(stats))
        .route("/v1/projects/{project}/memory-scores", get(memory_scores))
        .route("/v1/projects/{project}/structure", get(project_structure))
        .route(
            "/v1/projects/{project}/validation-runs",
            get(validation_runs),
        )
        .route("/v1/projects/{slug}/commits", get(project_commits))
        .route(
            "/v1/projects/{slug}/commits/{hash}",
            get(project_commit_detail),
        )
        .route(
            "/v1/projects/{slug}/bundle/export/preview",
            post(project_bundle_export_preview),
        )
        .route(
            "/v1/projects/{slug}/bundle/export",
            post(project_bundle_export),
        )
        .route(
            "/v1/projects/{slug}/bundle/import/preview",
            post(project_bundle_import_preview),
        )
        .route(
            "/v1/projects/{slug}/bundle/import",
            post(project_bundle_import),
        )
        .route(
            "/v1/projects/{slug}/replacement-proposals",
            get(project_replacement_proposals),
        )
        .route(
            "/v1/projects/{slug}/replacement-proposals/{proposal_id}/approve",
            post(project_replacement_proposal_approve),
        )
        .route(
            "/v1/projects/{slug}/replacement-proposals/{proposal_id}/reject",
            post(project_replacement_proposal_reject),
        )
        .route(
            "/v1/projects/{slug}/replacement-policy",
            get(project_replacement_policy)
                .put(project_replacement_policy_update)
                .post(project_replacement_policy_update),
        )
        .route("/v1/projects/{slug}/memories", get(project_memories))
        .route(
            "/v1/projects/{slug}/memory-graph",
            get(project_memory_graph),
        )
        .route("/v1/projects/{slug}/overview", get(project_overview))
        .route(
            "/v1/projects/{slug}/graph/status",
            get(project_graph_status),
        )
        .route("/v1/projects/{slug}/graph", get(project_graph))
        .route("/v1/projects/{slug}/resume", post(project_resume))
        .route("/v1/projects/{slug}/activities", get(project_activities))
        .route("/v1/projects/{slug}/up-to-speed", post(project_up_to_speed))
        .route("/v1/watchers/heartbeat", post(watcher_heartbeat))
        .route("/v1/watchers/unregister", post(watcher_unregister))
        .route("/v1/watchers/restart-local", post(watcher_restart_local))
        .route("/v1/archive", post(archive))
        .route("/v1/agents", get(agents_snapshot))
        .route("/v1/agents/workspaces", get(list_agent_workspaces))
        .route("/v1/agents/workspaces/start", post(start_agent_workspace))
        .route(
            "/v1/agents/workspaces/{workspace_id}/heartbeat",
            post(heartbeat_agent_workspace),
        )
        .route(
            "/v1/agents/workspaces/{workspace_id}/finish",
            post(finish_agent_workspace),
        )
        .with_state(state)
        .layer(TraceLayer::new_for_http());

    if mcp_config.enabled && mcp_config.http_enabled {
        app = app.merge(crate::mcp_http::build_mcp_http_router(config));
    }

    if let Some(root) = web_assets {
        let index = root.join("index.html");
        app.fallback_service(ServeDir::new(root).not_found_service(ServeFile::new(index)))
    } else {
        app.fallback(any(web_unavailable))
    }
}
