use crate::prelude::*;
use crate::*;

pub(crate) fn require_token(
    headers: &HeaderMap,
    expected: &str,
    _bind_addr: &str,
) -> Result<(), ApiError> {
    if let Some(provided) = headers
        .get("x-api-token")
        .and_then(|value| value.to_str().ok())
    {
        if provided != expected {
            return Err(ApiError::unauthorized("invalid api token"));
        }
        return Ok(());
    }

    Err(ApiError::unauthorized("missing x-api-token header"))
}

pub(crate) fn require_strict_token(headers: &HeaderMap, expected: &str) -> Result<(), ApiError> {
    let Some(provided) = headers
        .get("x-api-token")
        .and_then(|value| value.to_str().ok())
    else {
        return Err(ApiError::unauthorized("missing x-api-token header"));
    };
    if provided != expected {
        return Err(ApiError::unauthorized("invalid api token"));
    }
    Ok(())
}

/// Whether a request is allowed while the service runs in read-only
/// (student) mode. Reads are always allowed; the POST allowlist covers
/// endpoints that are semantically reads (query, briefings, bundle export)
/// even though they use POST bodies. Queries still reinforce activation —
/// that internal dynamic is the point of student mode; only content writes
/// are blocked.
pub(crate) fn read_only_request_allowed(method: &axum::http::Method, path: &str) -> bool {
    if matches!(
        *method,
        axum::http::Method::GET | axum::http::Method::HEAD | axum::http::Method::OPTIONS
    ) {
        return true;
    }
    if *method != axum::http::Method::POST {
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

/// Axum middleware enforcing read-only (student) mode when
/// `service.read_only` is set.
pub(crate) async fn read_only_guard(
    axum::extract::State(state): axum::extract::State<crate::state::AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    if state.config.service.read_only
        && !read_only_request_allowed(request.method(), request.uri().path())
    {
        return axum::response::IntoResponse::into_response(ApiError::status_message(
            axum::http::StatusCode::FORBIDDEN,
            "this Memory Layer runs in read-only (student) mode; writes are disabled",
        ));
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use axum::http::Method;

    use super::read_only_request_allowed;

    #[test]
    fn reads_and_query_paths_are_allowed() {
        assert!(read_only_request_allowed(&Method::GET, "/v1/stats"));
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
