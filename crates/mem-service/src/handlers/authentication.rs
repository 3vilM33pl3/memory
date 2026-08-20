// SPDX-License-Identifier: AGPL-3.0-or-later

use std::net::SocketAddr;

use axum::{
    Extension, Json,
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use mem_record::AuthMode;
use mem_record::{
    AuthMeResponse, AuthMembershipGrantRequest, AuthMembershipResponse, AuthPrincipalResponse,
    AuthServiceTokenCreateRequest, AuthServiceTokenResponse,
};
use uuid::Uuid;

use crate::{
    ApiError, AppState, AuthenticatedPrincipal, CSRF_COOKIE_NAME, SESSION_COOKIE_NAME,
    create_service_token, grant_membership, list_memberships, list_principals, list_service_tokens,
    revoke_membership, revoke_service_token,
};

pub(crate) async fn auth_me(
    State(state): State<AppState>,
    Extension(principal): Extension<AuthenticatedPrincipal>,
) -> Result<Json<AuthMeResponse>, ApiError> {
    Ok(Json(AuthMeResponse {
        authenticated: true,
        mode: state.config.auth.mode,
        read_only: state.config.service.read_only,
        principal: principal.to_response(),
    }))
}

/// Mints a browser session for the machine-local owner. Single-user mode
/// only, loopback connections only: this replaces the old /v1/web/auth-token
/// endpoint that handed the raw installation token to the browser.
pub(crate) async fn session_bootstrap(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> Result<Response, ApiError> {
    if state.config.auth.mode != AuthMode::SingleUser {
        return Err(ApiError::status_message(
            StatusCode::NOT_FOUND,
            "session bootstrap exists only in single-user mode; use the OIDC login flow",
        ));
    }
    if !peer.ip().is_loopback() {
        return Err(ApiError::forbidden(
            "session bootstrap requires a loopback connection",
        ));
    }
    let pool = state.pool()?;
    crate::auth::ensure_local_owner_principal(&pool)
        .await
        .map_err(ApiError::sql)?;
    let (session_secret, csrf_secret) =
        crate::auth::mint_web_session(&state, &pool, crate::auth::local_owner_id()).await?;
    let secure = false; // loopback-only by construction
    let max_age = i64::try_from(state.config.auth.session_ttl.as_secs())
        .map_err(|_| ApiError::internal("auth session ttl is too large"))?;
    let mut response = Json(serde_json::json!({ "authenticated": true })).into_response();
    crate::auth::append_cookie(
        response.headers_mut(),
        SESSION_COOKIE_NAME,
        &session_secret,
        max_age,
        true,
        secure,
    )?;
    crate::auth::append_cookie(
        response.headers_mut(),
        CSRF_COOKIE_NAME,
        &csrf_secret,
        max_age,
        false,
        secure,
    )?;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

pub(crate) async fn auth_logout(
    State(state): State<AppState>,
    Extension(principal): Extension<AuthenticatedPrincipal>,
) -> Result<Response, ApiError> {
    if let Some(session_id) = principal.session_id {
        sqlx::query("UPDATE auth_web_sessions SET revoked_at = now() WHERE id = $1")
            .bind(session_id)
            .execute(&state.pool()?)
            .await
            .map_err(ApiError::sql)?;
    }
    let secure = state
        .config
        .auth
        .public_base_url
        .as_deref()
        .is_some_and(|url| url.starts_with("https://"));
    let mut response = Json(serde_json::json!({"logged_out": true})).into_response();
    append_expired_cookie(response.headers_mut(), SESSION_COOKIE_NAME, true, secure)?;
    append_expired_cookie(response.headers_mut(), CSRF_COOKIE_NAME, false, secure)?;
    Ok(response)
}

pub(crate) async fn auth_list_tokens(
    State(state): State<AppState>,
) -> Result<Json<Vec<AuthServiceTokenResponse>>, ApiError> {
    Ok(Json(list_service_tokens(&state.pool()?).await?))
}

pub(crate) async fn auth_create_token(
    State(state): State<AppState>,
    Extension(principal): Extension<AuthenticatedPrincipal>,
    Json(request): Json<AuthServiceTokenCreateRequest>,
) -> Result<(axum::http::StatusCode, Json<AuthServiceTokenResponse>), ApiError> {
    let response = create_service_token(&state.pool()?, &principal, request).await?;
    Ok((axum::http::StatusCode::CREATED, Json(response)))
}

pub(crate) async fn auth_revoke_token(
    State(state): State<AppState>,
    Extension(principal): Extension<AuthenticatedPrincipal>,
    Path(selector): Path<String>,
) -> Result<Json<AuthServiceTokenResponse>, ApiError> {
    Ok(Json(
        revoke_service_token(&state.pool()?, &principal, &selector).await?,
    ))
}

pub(crate) async fn auth_list_principals(
    State(state): State<AppState>,
) -> Result<Json<Vec<AuthPrincipalResponse>>, ApiError> {
    Ok(Json(list_principals(&state.pool()?, &state).await?))
}

pub(crate) async fn auth_list_memberships(
    State(state): State<AppState>,
) -> Result<Json<Vec<AuthMembershipResponse>>, ApiError> {
    Ok(Json(list_memberships(&state.pool()?).await?))
}

pub(crate) async fn auth_grant_membership(
    State(state): State<AppState>,
    Extension(principal): Extension<AuthenticatedPrincipal>,
    Json(request): Json<AuthMembershipGrantRequest>,
) -> Result<Json<AuthMembershipResponse>, ApiError> {
    Ok(Json(
        grant_membership(&state.pool()?, &principal, request).await?,
    ))
}

pub(crate) async fn auth_revoke_membership(
    State(state): State<AppState>,
    Extension(principal): Extension<AuthenticatedPrincipal>,
    Path(id): Path<Uuid>,
) -> Result<Json<AuthMembershipResponse>, ApiError> {
    Ok(Json(
        revoke_membership(&state.pool()?, &principal, id).await?,
    ))
}

fn append_expired_cookie(
    headers: &mut HeaderMap,
    name: &str,
    http_only: bool,
    secure: bool,
) -> Result<(), ApiError> {
    let mut value = format!("{name}=; Path=/; Max-Age=0; SameSite=Lax");
    if http_only {
        value.push_str("; HttpOnly");
    }
    if secure {
        value.push_str("; Secure");
    }
    headers.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&value)
            .map_err(|_| ApiError::internal("failed to build logout cookie"))?,
    );
    Ok(())
}
