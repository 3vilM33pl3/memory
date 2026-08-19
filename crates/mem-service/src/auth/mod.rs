// SPDX-License-Identifier: AGPL-3.0-or-later

mod model;
mod oidc;
mod policy;
mod store;

pub(crate) use model::*;
pub(crate) use oidc::*;
pub(crate) use policy::*;
pub(crate) use store::*;

use axum::http::HeaderMap;

use crate::ApiError;

/// Compatibility check used by handlers that predate centralized
/// authorization. The auth middleware rewrites this internal header only after
/// it has authenticated and authorized the request.
pub(crate) fn require_token(
    headers: &HeaderMap,
    expected: &str,
    _bind_addr: &str,
) -> Result<(), ApiError> {
    require_strict_token(headers, expected)
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
