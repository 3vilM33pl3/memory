use std::sync::Arc;

use axum::{
    Router,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::Response,
    routing::{any, delete, get, post},
};
use mem_api::AppConfig;
use rmcp::transport::{
    StreamableHttpServerConfig, StreamableHttpService,
    streamable_http_server::session::local::LocalSessionManager,
};
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

use super::{
    AppState, activate_embedding_backend, admin_shutdown, agents_snapshot, archive, capture_task,
    checkpoint_activity, curate_memory, deactivate_embedding_backend, delete_memory, get_memory,
    get_memory_history, graph_activity, healthz, list_embedding_backends, llm_audit_status,
    plan_activity, project_activities, project_bundle_export, project_bundle_export_preview,
    project_bundle_import, project_bundle_import_preview, project_commit_detail, project_commits,
    project_memories, project_overview, project_replacement_policy,
    project_replacement_policy_update, project_replacement_proposal_approve,
    project_replacement_proposal_reject, project_replacement_proposals, project_resume,
    project_up_to_speed, prune_embeddings, prune_history, query, reembed, reindex, runtime_status,
    scan_activity, set_embedding_creation_enabled, set_llm_audit_enabled, stats, sync_commits,
    verify_provenance, watcher_heartbeat, watcher_restart_local, watcher_unregister,
    web_auth_token, web_unavailable, websocket,
};

pub(crate) fn build_http_app(state: AppState) -> Router {
    let web_assets = state.web_root.clone();
    let config = state.config.clone();
    let mcp_config = state.config.mcp.clone();
    let mut app = Router::new()
        .route("/healthz", get(healthz))
        .route("/ws", get(websocket))
        .route("/v1/web/auth-token", get(web_auth_token))
        .route("/v1/admin/shutdown", post(admin_shutdown))
        .route("/v1/runtime/status", get(runtime_status))
        .route("/v1/query", post(query))
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
        .route("/v1/memory/{id}", get(get_memory))
        .route("/v1/memory/{id}/history", get(get_memory_history))
        .route("/v1/memory", delete(delete_memory))
        .route("/v1/prune-history", post(prune_history))
        .route("/v1/stats", get(stats))
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
        .route("/v1/projects/{slug}/overview", get(project_overview))
        .route("/v1/projects/{slug}/resume", post(project_resume))
        .route("/v1/projects/{slug}/activities", get(project_activities))
        .route("/v1/projects/{slug}/up-to-speed", post(project_up_to_speed))
        .route("/v1/watchers/heartbeat", post(watcher_heartbeat))
        .route("/v1/watchers/unregister", post(watcher_unregister))
        .route("/v1/watchers/restart-local", post(watcher_restart_local))
        .route("/v1/archive", post(archive))
        .route("/v1/agents", get(agents_snapshot))
        .with_state(state)
        .layer(TraceLayer::new_for_http());

    if mcp_config.enabled && mcp_config.http_enabled {
        app = app.merge(build_mcp_http_router(config));
    }

    if let Some(root) = web_assets {
        let index = root.join("index.html");
        app.fallback_service(ServeDir::new(root).not_found_service(ServeFile::new(index)))
    } else {
        app.fallback(any(web_unavailable))
    }
}

#[derive(Clone)]
struct McpHttpAuth {
    require_token: bool,
    api_token: String,
    bind_addr: String,
}

fn build_mcp_http_router(config: AppConfig) -> Router {
    let service_config = config.service.clone();
    let mcp_config = config.mcp.clone();
    let server_config = StreamableHttpServerConfig::default()
        .with_allowed_hosts(mcp_allowed_hosts(&service_config.bind_addr))
        .with_allowed_origins(mcp_allowed_origins(&service_config.bind_addr));
    let service = StreamableHttpService::new(
        move || Ok(mem_mcp::MemoryMcpServer::http(config.clone())),
        Arc::new(LocalSessionManager::default()),
        server_config,
    );
    let path = normalize_mcp_path(&mcp_config.http_path);
    Router::new()
        .nest_service(&path, service)
        .route_layer(middleware::from_fn_with_state(
            McpHttpAuth {
                require_token: mcp_config.require_token,
                api_token: service_config.api_token,
                bind_addr: service_config.bind_addr,
            },
            mcp_http_auth_middleware,
        ))
}

async fn mcp_http_auth_middleware(
    State(auth): State<McpHttpAuth>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: Next,
) -> Result<Response, StatusCode> {
    validate_mcp_origin(&headers, &auth.bind_addr)?;
    if auth.require_token && !mcp_token_matches(&headers, &auth.api_token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(next.run(request).await)
}

pub(crate) fn mcp_token_matches(headers: &HeaderMap, expected: &str) -> bool {
    headers
        .get("x-api-token")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == expected)
        || headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .is_some_and(|value| value == expected)
}

pub(crate) fn validate_mcp_origin(headers: &HeaderMap, bind_addr: &str) -> Result<(), StatusCode> {
    let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        return Ok(());
    };
    if mcp_allowed_origins(bind_addr)
        .iter()
        .any(|allowed| origin == allowed)
    {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

fn normalize_mcp_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        "/mcp".to_string()
    } else if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

fn mcp_allowed_hosts(bind_addr: &str) -> Vec<String> {
    let host = bind_addr
        .rsplit_once(':')
        .map(|(host, _)| host.trim_matches('[').trim_matches(']'))
        .unwrap_or(bind_addr);
    let mut hosts = vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "::1".to_string(),
    ];
    if !host.is_empty() && !hosts.iter().any(|value| value == host) {
        hosts.push(host.to_string());
    }
    hosts
}

fn mcp_allowed_origins(bind_addr: &str) -> Vec<String> {
    let host = bind_addr
        .rsplit_once(':')
        .map(|(host, _)| host.trim_matches('[').trim_matches(']'))
        .unwrap_or(bind_addr);
    let mut origins = vec![
        "http://127.0.0.1".to_string(),
        "http://localhost".to_string(),
        "http://[::1]".to_string(),
    ];
    if !host.is_empty() && !matches!(host, "127.0.0.1" | "localhost" | "::1") {
        origins.push(format!("http://{host}"));
    }
    origins
}
