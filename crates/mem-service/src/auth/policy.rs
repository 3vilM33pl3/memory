// SPDX-License-Identifier: AGPL-3.0-or-later

use axum::{
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use mem_record::AuthRole;
use serde_json::Value;
use sqlx::Row;
use uuid::Uuid;

use super::{
    AuthenticatedPrincipal, CSRF_COOKIE_NAME, CredentialSource, cookie_value, hash_secret,
    resolve_request_principal,
};
use crate::{ApiError, AppState};

const MAX_AUTH_INSPECTION_BODY: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProjectScope {
    Public,
    Unscoped,
    Global,
    PathProject,
    BodyProject,
    BodyMemoryResource,
    QueryProjectOrGlobal,
    MemoryResource,
    ValidationResource,
    LoopRunResource,
    LoopApprovalResource,
    LoopProposalResource,
    WorkspaceResource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RoutePolicy {
    pub(crate) role: AuthRole,
    pub(crate) scope: ProjectScope,
}

pub(crate) fn route_policy(method: &Method, path: &str) -> RoutePolicy {
    if matches!(
        path,
        "/healthz"
            | "/v1/openapi.yaml"
            | "/v1/web/auth-token"
            | "/v1/auth/login"
            | "/v1/auth/callback"
    ) {
        return RoutePolicy {
            role: AuthRole::Reader,
            scope: ProjectScope::Public,
        };
    }
    if path == "/v1/auth/me" || path == "/v1/auth/logout" {
        return unscoped(AuthRole::Reader);
    }
    if path.starts_with("/v1/auth/") || path == "/v1/admin/shutdown" {
        return global(AuthRole::Admin);
    }
    if path.starts_with("/v1/projects/") {
        let role = if path.contains("/bundle/import")
            || path.contains("/replacement-proposals/")
            || path.ends_with("/graph/extract")
            || (path.ends_with("/replacement-policy") && *method != Method::GET)
        {
            AuthRole::Operator
        } else {
            AuthRole::Reader
        };
        return RoutePolicy {
            role,
            scope: ProjectScope::PathProject,
        };
    }
    if path == "/v1/query/global" {
        return unscoped(AuthRole::Reader);
    }
    if path == "/v1/query" {
        return body(AuthRole::Reader);
    }
    if path == "/v1/memory" && *method == Method::DELETE {
        // DeleteMemoryRequest carries no project field; authorize against the
        // project that owns the referenced memory, not "any admin grant".
        return RoutePolicy {
            role: AuthRole::Admin,
            scope: ProjectScope::BodyMemoryResource,
        };
    }
    if path.starts_with("/v1/memory/") {
        return RoutePolicy {
            role: if path.ends_with("/history") || *method == Method::GET {
                AuthRole::Reader
            } else {
                AuthRole::Operator
            },
            scope: ProjectScope::MemoryResource,
        };
    }
    if path.starts_with("/v1/validation-runs/") {
        return RoutePolicy {
            role: AuthRole::Operator,
            scope: ProjectScope::ValidationResource,
        };
    }
    if path == "/v1/agents" || path == "/v1/agents/workspaces" || path == "/v1/skills/repair" {
        return global(AuthRole::Admin);
    }
    if path == "/v1/runtime/status" {
        // Returns per-project watcher and workspace state for ?project=…;
        // without the parameter it reports service-wide state.
        return RoutePolicy {
            role: AuthRole::Reader,
            scope: ProjectScope::QueryProjectOrGlobal,
        };
    }
    if path.starts_with("/v1/skills")
        || path == "/ws"
        || path == "/v1/embeddings/backends"
        || (path == "/v1/config/llm-audit" && *method == Method::GET)
    {
        return unscoped(AuthRole::Reader);
    }
    if path == "/v1/loops/runs"
        || path == "/v1/loops/approvals"
        || path == "/v1/loops/memory-proposals"
    {
        return RoutePolicy {
            role: if *method == Method::GET {
                AuthRole::Reader
            } else {
                AuthRole::Operator
            },
            scope: ProjectScope::QueryProjectOrGlobal,
        };
    }
    if path.starts_with("/v1/loops/runs/") {
        return RoutePolicy {
            role: if *method == Method::GET {
                AuthRole::Reader
            } else {
                AuthRole::Operator
            },
            scope: ProjectScope::LoopRunResource,
        };
    }
    if path.starts_with("/v1/loops/approvals/") {
        return RoutePolicy {
            role: AuthRole::Operator,
            scope: ProjectScope::LoopApprovalResource,
        };
    }
    if path.starts_with("/v1/loops/memory-proposals/") {
        return RoutePolicy {
            role: AuthRole::Operator,
            scope: ProjectScope::LoopProposalResource,
        };
    }
    if path == "/v1/loops/global-kill-switch" {
        return if *method == Method::GET {
            unscoped(AuthRole::Reader)
        } else {
            global(AuthRole::Admin)
        };
    }
    if path.starts_with("/v1/loops/") {
        if *method == Method::GET {
            return RoutePolicy {
                role: AuthRole::Reader,
                scope: ProjectScope::QueryProjectOrGlobal,
            };
        }
        if path.ends_with("/enable")
            || path.ends_with("/disable")
            || path.ends_with("/pause")
            || path.ends_with("/snooze")
        {
            return body(AuthRole::Admin);
        }
        return body(AuthRole::Operator);
    }
    if path == "/v1/loops" {
        return unscoped(AuthRole::Reader);
    }
    if path.starts_with("/v1/agents/workspaces/") {
        return RoutePolicy {
            role: AuthRole::Writer,
            scope: ProjectScope::WorkspaceResource,
        };
    }
    if path == "/v1/embeddings/activate"
        || path == "/v1/embeddings/deactivate"
        || path == "/v1/embeddings/create-enabled"
        || (path == "/v1/config/llm-audit" && *method != Method::GET)
    {
        return global(AuthRole::Admin);
    }
    if matches!(
        path,
        "/v1/curate"
            | "/v1/provenance/verify"
            | "/v1/reindex"
            | "/v1/reembed"
            | "/v1/prune-embeddings"
            | "/v1/prune-history"
            | "/v1/archive"
    ) {
        return body(AuthRole::Operator);
    }
    if matches!(
        path,
        "/v1/checkpoint/activity"
            | "/v1/plan/activity"
            | "/v1/scan/activity"
            | "/v1/graph/activity"
            | "/v1/commits/sync"
            | "/v1/capture/task"
            | "/v1/watchers/heartbeat"
            | "/v1/watchers/unregister"
            | "/v1/watchers/restart-local"
            | "/v1/agents/workspaces/start"
    ) {
        return body(AuthRole::Writer);
    }

    if *method == Method::GET || *method == Method::HEAD {
        unscoped(AuthRole::Reader)
    } else {
        global(AuthRole::Admin)
    }
}

const fn unscoped(role: AuthRole) -> RoutePolicy {
    RoutePolicy {
        role,
        scope: ProjectScope::Unscoped,
    }
}

const fn global(role: AuthRole) -> RoutePolicy {
    RoutePolicy {
        role,
        scope: ProjectScope::Global,
    }
}

const fn body(role: AuthRole) -> RoutePolicy {
    RoutePolicy {
        role,
        scope: ProjectScope::BodyProject,
    }
}

#[axum::debug_middleware]
pub(crate) async fn authorization_guard(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    if !state.is_primary() {
        if request.uri().path() == "/ws" {
            return next.run(request).await;
        }
        if request.uri().path().starts_with("/v1/") {
            return match proxy_relay_request(&state, request).await {
                Ok(response) => response,
                Err(error) => error.into_response(),
            };
        }
    }

    let policy = route_policy(request.method(), request.uri().path());
    if policy.scope == ProjectScope::Public {
        return next.run(request).await;
    }

    let principal = match resolve_request_principal(&state, request.headers(), true).await {
        Ok(Some(principal)) => principal,
        Ok(None) => {
            return ApiError::unauthorized("authentication required").into_response();
        }
        Err(error) => return error.into_response(),
    };

    if principal.credential_source == CredentialSource::BrowserSession
        && is_mutating(request.method())
        && let Err(error) = validate_browser_mutation(&state, request.headers(), &principal)
    {
        return error.into_response();
    }

    let project = match resolve_request_project(&state, &mut request, policy.scope).await {
        Ok(project) => project,
        Err(error) => return error.into_response(),
    };
    let authorized = is_authorized(&principal, policy, project.as_deref());
    if !authorized {
        return ApiError::forbidden("principal does not have the required role for this resource")
            .into_response();
    }

    request.extensions_mut().insert(principal);
    next.run(request).await
}

/// Pure authorization decision for a resolved (policy, project) pair.
/// Project-scoped policies whose project could not be resolved require a
/// GLOBAL grant — never "any grant anywhere", which would let a principal
/// scoped to one project act on resources it cannot even name.
fn is_authorized(
    principal: &AuthenticatedPrincipal,
    policy: RoutePolicy,
    project: Option<&str>,
) -> bool {
    match policy.scope {
        ProjectScope::Public => true,
        ProjectScope::Global => principal.has_global_role(policy.role),
        ProjectScope::Unscoped => principal.has_any_role(policy.role),
        _ => match project {
            Some(project) => principal
                .role_for_project(project)
                .is_some_and(|role| role >= policy.role),
            None => principal.has_global_role(policy.role),
        },
    }
}

pub(crate) async fn proxy_relay_request(
    state: &AppState,
    request: Request,
) -> Result<Response, ApiError> {
    let peer = crate::selected_primary_peer(state).ok_or_else(|| {
        ApiError::service_unavailable("no primary memory service available on the local network")
    })?;
    let (parts, body) = request.into_parts();
    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or_else(|| parts.uri.path());
    let body = to_bytes(body, MAX_AUTH_INSPECTION_BODY)
        .await
        .map_err(|error| ApiError::io(error.into()))?;
    let mut headers = parts.headers;
    for name in [
        header::HOST,
        header::CONTENT_LENGTH,
        header::CONNECTION,
        header::TRANSFER_ENCODING,
        header::ACCEPT_ENCODING,
    ] {
        headers.remove(name);
    }
    let upstream = state
        .http_client
        .request(
            parts.method,
            format!("http://{}{}", peer.advertise_addr, path_and_query),
        )
        .headers(headers)
        .body(body)
        .send()
        .await
        .map_err(|error| ApiError::io(error.into()))?;
    let status = upstream.status();
    let upstream_headers = upstream.headers().clone();
    let mut response = Response::new(Body::from_stream(upstream.bytes_stream()));
    *response.status_mut() = status;
    for (name, value) in &upstream_headers {
        if !matches!(
            name.as_str(),
            "content-length" | "transfer-encoding" | "content-encoding" | "connection"
        ) {
            response.headers_mut().append(name, value.clone());
        }
    }
    Ok(response)
}

fn is_mutating(method: &Method) -> bool {
    !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

fn validate_browser_mutation(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    principal: &AuthenticatedPrincipal,
) -> Result<(), ApiError> {
    let public_base_url = state
        .config
        .auth
        .public_base_url
        .as_deref()
        .ok_or_else(|| {
            ApiError::forbidden("auth.public_base_url is required for browser writes")
        })?;
    let expected_origin = reqwest::Url::parse(public_base_url)
        .map_err(|_| ApiError::internal("auth.public_base_url is invalid"))?
        .origin()
        .ascii_serialization();
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::forbidden("browser write is missing Origin"))?;
    if origin != expected_origin {
        return Err(ApiError::forbidden("browser write Origin is not allowed"));
    }
    let csrf_header = headers
        .get("x-csrf-token")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::forbidden("browser write is missing x-csrf-token"))?;
    let csrf_cookie = cookie_value(headers, CSRF_COOKIE_NAME)
        .ok_or_else(|| ApiError::forbidden("browser write is missing CSRF cookie"))?;
    if csrf_header != csrf_cookie
        || principal.session_csrf_hash.as_deref() != Some(hash_secret(csrf_header).as_slice())
    {
        return Err(ApiError::forbidden("invalid browser CSRF token"));
    }
    Ok(())
}

async fn resolve_request_project(
    state: &AppState,
    request: &mut Request,
    scope: ProjectScope,
) -> Result<Option<String>, ApiError> {
    match scope {
        ProjectScope::Public | ProjectScope::Unscoped | ProjectScope::Global => Ok(None),
        ProjectScope::PathProject => Ok(request
            .uri()
            .path()
            .split('/')
            .nth(3)
            .and_then(|value| urlencoding::decode(value).ok())
            .map(|value| value.into_owned())),
        ProjectScope::BodyProject => project_from_body(request).await,
        ProjectScope::BodyMemoryResource => {
            let memory_id = memory_id_from_body(request).await?;
            resource_project(state, memory_id, ResourceKind::Memory).await
        }
        ProjectScope::QueryProjectOrGlobal => Ok(query_value(request, "project")),
        ProjectScope::MemoryResource => {
            resource_project(state, resource_id(request), ResourceKind::Memory).await
        }
        ProjectScope::ValidationResource => {
            resource_project(state, resource_id(request), ResourceKind::Validation).await
        }
        ProjectScope::LoopRunResource => {
            resource_project(state, resource_id(request), ResourceKind::LoopRun).await
        }
        ProjectScope::LoopApprovalResource => {
            resource_project(state, resource_id(request), ResourceKind::LoopApproval).await
        }
        ProjectScope::LoopProposalResource => {
            resource_project(state, resource_id(request), ResourceKind::LoopProposal).await
        }
        ProjectScope::WorkspaceResource => {
            resource_project(state, resource_id(request), ResourceKind::Workspace).await
        }
    }
}

async fn project_from_body(request: &mut Request) -> Result<Option<String>, ApiError> {
    body_string_field(request, "project").await
}

async fn memory_id_from_body(request: &mut Request) -> Result<Option<Uuid>, ApiError> {
    Ok(body_string_field(request, "memory_id")
        .await?
        .and_then(|value| Uuid::parse_str(&value).ok()))
}

async fn body_string_field(request: &mut Request, field: &str) -> Result<Option<String>, ApiError> {
    let body = std::mem::replace(request.body_mut(), Body::empty());
    let bytes = to_bytes(body, MAX_AUTH_INSPECTION_BODY)
        .await
        .map_err(|_| {
            ApiError::status_message(StatusCode::PAYLOAD_TOO_LARGE, "request body is too large")
        })?;
    let value = if bytes.is_empty() {
        None
    } else {
        serde_json::from_slice::<Value>(&bytes)
            .ok()
            .and_then(|value| value.get(field).and_then(Value::as_str).map(str::to_owned))
    };
    *request.body_mut() = Body::from(bytes);
    Ok(value)
}

fn query_value(request: &Request, name: &str) -> Option<String> {
    request.uri().query().and_then(|query| {
        query.split('&').find_map(|part| {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            (key == name)
                .then(|| {
                    urlencoding::decode(value)
                        .ok()
                        .map(|value| value.into_owned())
                })
                .flatten()
        })
    })
}

#[derive(Debug, Clone, Copy)]
enum ResourceKind {
    Memory,
    Validation,
    LoopRun,
    LoopApproval,
    LoopProposal,
    Workspace,
}

async fn resource_project(
    state: &AppState,
    id: Option<Uuid>,
    kind: ResourceKind,
) -> Result<Option<String>, ApiError> {
    let Some(id) = id else {
        return Ok(None);
    };
    let pool = state.pool()?;
    let sql = match kind {
        ResourceKind::Memory => {
            "SELECT p.slug FROM memory_entries r JOIN projects p ON p.id = r.project_id WHERE r.id = $1 OR r.canonical_id = $1 LIMIT 1"
        }
        ResourceKind::Validation => {
            "SELECT p.slug FROM memory_validation_runs r JOIN projects p ON p.id = r.project_id WHERE r.id = $1"
        }
        ResourceKind::LoopRun => {
            "SELECT p.slug FROM loop_runs r JOIN projects p ON p.id = r.project_id WHERE r.id = $1"
        }
        ResourceKind::LoopApproval => {
            "SELECT p.slug FROM approval_requests r JOIN projects p ON p.id = r.project_id WHERE r.id = $1"
        }
        ResourceKind::LoopProposal => {
            "SELECT p.slug FROM memory_proposals r JOIN projects p ON p.id = r.project_id WHERE r.id = $1"
        }
        ResourceKind::Workspace => {
            "SELECT p.slug FROM agent_workspaces r JOIN projects p ON p.id = r.project_id WHERE r.id = $1"
        }
    };
    let row = sqlx::query(sql)
        .bind(id)
        .fetch_optional(&pool)
        .await
        .map_err(ApiError::sql)?;
    row.map(|row| row.try_get("slug").map_err(ApiError::sql))
        .transpose()
}

fn resource_id(request: &Request) -> Option<Uuid> {
    request
        .uri()
        .path()
        .split('/')
        .find_map(|part| Uuid::parse_str(part).ok())
}

/// Whether a request is allowed while the service runs in read-only mode.
/// Queries and briefing/export POST routes are semantic reads.
pub(crate) fn read_only_request_allowed(method: &Method, path: &str) -> bool {
    if matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return true;
    }
    if *method != Method::POST {
        return false;
    }
    match path {
        "/v1/query" | "/v1/query/global" => true,
        _ => {
            path.starts_with("/v1/projects/")
                && (path.ends_with("/resume")
                    || path.ends_with("/up-to-speed")
                    || path.ends_with("/bundle/export")
                    || path.ends_with("/bundle/export/preview"))
        }
    }
}

pub(crate) async fn read_only_guard(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if state.config.service.read_only
        && !read_only_request_allowed(request.method(), request.uri().path())
    {
        return ApiError::status_message(
            axum::http::StatusCode::FORBIDDEN,
            "this Memory Layer runs in read-only (student) mode; writes are disabled",
        )
        .into_response();
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use axum::http::Method;
    use mem_record::{AuthPrincipalKind, AuthRole};
    use uuid::Uuid;

    use super::{
        AuthenticatedPrincipal, CredentialSource, ProjectScope, RoutePolicy, is_authorized,
        read_only_request_allowed, route_policy,
    };
    use crate::auth::ProjectRoleGrant;

    fn project_admin(project: &str) -> AuthenticatedPrincipal {
        let mut project_roles = BTreeMap::new();
        project_roles.insert(
            project.to_string(),
            ProjectRoleGrant {
                role: AuthRole::Admin,
                source: "test".to_string(),
            },
        );
        AuthenticatedPrincipal {
            id: Uuid::new_v4(),
            kind: AuthPrincipalKind::HumanOidc,
            display_name: "Project Admin".to_string(),
            email: None,
            groups: Vec::new(),
            global_role: None,
            project_roles,
            credential_source: CredentialSource::BrowserSession,
            token_id: None,
            session_id: None,
            session_csrf_hash: None,
        }
    }

    #[test]
    fn delete_memory_resolves_the_owning_project() {
        assert_eq!(
            route_policy(&Method::DELETE, "/v1/memory").scope,
            ProjectScope::BodyMemoryResource
        );
    }

    #[test]
    fn project_scoped_admin_cannot_act_across_projects() {
        let principal = project_admin("project-a");
        let delete_policy = RoutePolicy {
            role: AuthRole::Admin,
            scope: ProjectScope::BodyMemoryResource,
        };
        // Memory resolved to another project: denied.
        assert!(!is_authorized(&principal, delete_policy, Some("project-b")));
        // Memory in the granted project: allowed.
        assert!(is_authorized(&principal, delete_policy, Some("project-a")));
        // Unresolvable project (unknown memory id, or a body without a
        // project on a BodyProject route) requires a GLOBAL grant.
        assert!(!is_authorized(&principal, delete_policy, None));
        let prune_policy = RoutePolicy {
            role: AuthRole::Operator,
            scope: ProjectScope::BodyProject,
        };
        assert!(!is_authorized(&principal, prune_policy, None));
        assert!(is_authorized(&principal, prune_policy, Some("project-a")));
    }

    #[test]
    fn global_admin_still_authorized_without_project() {
        let mut principal = project_admin("project-a");
        principal.global_role = Some(AuthRole::Admin);
        let delete_policy = RoutePolicy {
            role: AuthRole::Admin,
            scope: ProjectScope::BodyMemoryResource,
        };
        assert!(is_authorized(&principal, delete_policy, None));
        assert!(is_authorized(&principal, delete_policy, Some("project-b")));
    }

    #[test]
    fn route_roles_cover_role_boundaries() {
        assert_eq!(
            route_policy(&Method::POST, "/v1/capture/task").role,
            AuthRole::Writer
        );
        assert_eq!(
            route_policy(&Method::POST, "/v1/curate").role,
            AuthRole::Operator
        );
        assert_eq!(
            route_policy(&Method::DELETE, "/v1/memory").role,
            AuthRole::Admin
        );
        assert_eq!(
            route_policy(&Method::GET, "/v1/projects/demo/overview").scope,
            ProjectScope::PathProject
        );
        assert_eq!(
            route_policy(&Method::GET, "/healthz").scope,
            ProjectScope::Public
        );
        assert_eq!(
            route_policy(&Method::GET, "/v1/loops/context_pack_refresh/context-pack").scope,
            ProjectScope::QueryProjectOrGlobal
        );
        assert_eq!(
            route_policy(&Method::POST, "/v1/auth/tokens").role,
            AuthRole::Admin
        );
    }

    #[test]
    fn reads_and_query_paths_are_allowed() {
        assert!(read_only_request_allowed(&Method::GET, "/v1/loops"));
        assert!(read_only_request_allowed(
            &Method::GET,
            "/v1/projects/demo/structure"
        ));
        assert!(read_only_request_allowed(&Method::POST, "/v1/query"));
        assert!(read_only_request_allowed(&Method::POST, "/v1/query/global"));
        assert!(read_only_request_allowed(
            &Method::POST,
            "/v1/projects/classroom/resume"
        ));
        assert!(read_only_request_allowed(
            &Method::POST,
            "/v1/projects/classroom/up-to-speed"
        ));
        assert!(read_only_request_allowed(
            &Method::POST,
            "/v1/projects/classroom/bundle/export"
        ));
        assert!(read_only_request_allowed(
            &Method::POST,
            "/v1/projects/classroom/bundle/export/preview"
        ));
    }

    #[test]
    fn mutating_endpoints_are_blocked() {
        assert!(!read_only_request_allowed(&Method::POST, "/v1/curate"));
        assert!(!read_only_request_allowed(
            &Method::POST,
            "/v1/capture/task"
        ));
        assert!(!read_only_request_allowed(
            &Method::POST,
            "/v1/projects/classroom/bundle/import"
        ));
        assert!(!read_only_request_allowed(
            &Method::POST,
            "/v1/projects/classroom/bundle/import/preview"
        ));
        assert!(!read_only_request_allowed(&Method::POST, "/v1/archive"));
        assert!(!read_only_request_allowed(
            &Method::POST,
            "/v1/loops/memory_consolidation/run"
        ));
        assert!(!read_only_request_allowed(&Method::DELETE, "/v1/memory"));
        assert!(!read_only_request_allowed(
            &Method::PUT,
            "/v1/projects/classroom/replacement-policy"
        ));
        assert!(!read_only_request_allowed(
            &Method::POST,
            "/v1/admin/shutdown"
        ));
    }
}
