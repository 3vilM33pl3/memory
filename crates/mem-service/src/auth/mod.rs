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
