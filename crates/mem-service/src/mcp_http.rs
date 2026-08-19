// SPDX-License-Identifier: AGPL-3.0-or-later

use std::sync::Arc;

use axum::{
    Router,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::Response,
};
use mem_api::{AuthMode, AuthRole};
use rmcp::transport::{
    StreamableHttpServerConfig, StreamableHttpService,
    streamable_http_server::session::local::LocalSessionManager,
};

pub(crate) fn build_mcp_http_router(state: crate::AppState) -> Router {
    let config = state.config.clone();
    let service_config = state.config.service.clone();
    let mcp_config = state.config.mcp.clone();
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
            state,
            mcp_http_auth_middleware,
        ))
}

async fn mcp_http_auth_middleware(
    State(state): State<crate::AppState>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if !state.is_primary() {
        return crate::auth::proxy_relay_request(&state, request)
            .await
            .map_err(|error| error.status);
    }
    validate_mcp_origin(&headers, &state.config.service.bind_addr)?;
    let principal = crate::auth::resolve_request_principal(&state, &headers, false)
        .await
        .map_err(|error| error.status)?;
    let token_required =
        state.config.mcp.require_token || state.config.auth.mode == AuthMode::MultiUser;
    let Some(principal) = principal else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    if token_required
        && principal.credential_source == crate::auth::CredentialSource::AnonymousSingleUser
    {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if !principal.has_any_role(AuthRole::Reader) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(next.run(request).await)
}

#[cfg(test)]
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
