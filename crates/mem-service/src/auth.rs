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
