// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::BTreeSet;

use axum::{
    extract::{Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{Duration, Utc};
use mem_record::AuthMode;
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet, EndpointNotSet,
    EndpointSet, IssuerUrl, Nonce, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope,
    TokenResponse,
    core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata},
};
use serde::Deserialize;
use sqlx::Row;
use uuid::Uuid;

use super::{CSRF_COOKIE_NAME, SESSION_COOKIE_NAME, hash_secret, random_secret};
use crate::{ApiError, AppState};

const OIDC_FLOW_TTL: Duration = Duration::minutes(10);

type DiscoveredCoreClient = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

#[derive(Debug, Default, Deserialize)]
pub(crate) struct AuthLoginQuery {
    return_to: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AuthCallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

pub(crate) async fn auth_login(
    State(state): State<AppState>,
    Query(query): Query<AuthLoginQuery>,
) -> Result<Response, ApiError> {
    require_multiuser_oidc(&state)?;
    let return_to = validate_return_to(query.return_to.as_deref())?;
    let client = discover_client(&state).await?;
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let mut authorization = client
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        .set_pkce_challenge(pkce_challenge);
    for scope in additional_oidc_scopes(&state.config.auth.oidc.scopes) {
        authorization = authorization.add_scope(Scope::new(scope));
    }
    let (authorization_url, csrf_state, nonce) = authorization.url();
    let pool = state.pool()?;
    sqlx::query("DELETE FROM auth_oidc_flows WHERE expires_at <= now()")
        .execute(&pool)
        .await
        .map_err(ApiError::sql)?;
    sqlx::query(
        r#"
        INSERT INTO auth_oidc_flows
            (id, state_hash, nonce, pkce_verifier, return_to, created_at, expires_at)
        VALUES ($1, $2, $3, $4, $5, now(), $6)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(hash_secret(csrf_state.secret()))
    .bind(nonce.secret())
    .bind(pkce_verifier.secret())
    .bind(return_to)
    .bind(Utc::now() + OIDC_FLOW_TTL)
    .execute(&pool)
    .await
    .map_err(ApiError::sql)?;

    Ok(Redirect::temporary(authorization_url.as_str()).into_response())
}

pub(crate) async fn auth_callback(
    State(state): State<AppState>,
    Query(query): Query<AuthCallbackQuery>,
) -> Result<Response, ApiError> {
    require_multiuser_oidc(&state)?;
    if let Some(error) = query.error.as_deref() {
        let description = query
            .error_description
            .as_deref()
            .unwrap_or("no description");
        return Err(ApiError::status_message(
            StatusCode::BAD_REQUEST,
            format!("OIDC provider rejected login: {error}: {description}"),
        ));
    }
    let code = query
        .code
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::status_message(StatusCode::BAD_REQUEST, "missing OIDC code"))?;
    let returned_state = query
        .state
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::status_message(StatusCode::BAD_REQUEST, "missing OIDC state"))?;
    let pool = state.pool()?;
    let flow = sqlx::query(
        r#"
        DELETE FROM auth_oidc_flows
        WHERE state_hash = $1 AND expires_at > now()
        RETURNING nonce, pkce_verifier, return_to
        "#,
    )
    .bind(hash_secret(&returned_state))
    .fetch_optional(&pool)
    .await
    .map_err(ApiError::sql)?
    .ok_or_else(|| {
        ApiError::status_message(StatusCode::BAD_REQUEST, "invalid or expired OIDC state")
    })?;
    let nonce: String = flow.try_get("nonce").map_err(ApiError::sql)?;
    let pkce_verifier: String = flow.try_get("pkce_verifier").map_err(ApiError::sql)?;
    let return_to: Option<String> = flow.try_get("return_to").map_err(ApiError::sql)?;

    let client = discover_client(&state).await?;
    let http_client = oidc_http_client()?;
    let token_response = client
        .exchange_code(AuthorizationCode::new(code))
        .map_err(|error| {
            ApiError::status_message(
                StatusCode::BAD_GATEWAY,
                format!("OIDC provider does not support code exchange: {error}"),
            )
        })?
        .set_pkce_verifier(PkceCodeVerifier::new(pkce_verifier))
        .request_async(&http_client)
        .await
        .map_err(|error| {
            ApiError::status_message(
                StatusCode::BAD_GATEWAY,
                format!("OIDC code exchange failed: {error}"),
            )
        })?;
    let id_token = token_response.id_token().ok_or_else(|| {
        ApiError::status_message(
            StatusCode::BAD_GATEWAY,
            "OIDC response did not contain an ID token",
        )
    })?;
    let claims = id_token
        .claims(&client.id_token_verifier(), &Nonce::new(nonce))
        .map_err(|error| {
            ApiError::status_message(
                StatusCode::UNAUTHORIZED,
                format!("OIDC ID token validation failed: {error}"),
            )
        })?;
    let subject = claims.subject().as_str().to_string();
    let email = claims.email().map(|email| email.as_str().to_string());
    let display_name = claims
        .preferred_username()
        .map(|name| name.as_str().to_string())
        .or_else(|| email.clone())
        .unwrap_or_else(|| subject.clone());
    let groups = groups_from_id_token(&id_token.to_string(), &state.config.auth.oidc.groups_claim)?;
    let issuer = state.config.auth.oidc.issuer_url.trim_end_matches('/');
    let principal_id = upsert_human_principal(
        &pool,
        issuer,
        &subject,
        &display_name,
        email.as_deref(),
        &groups,
    )
    .await?;

    let (session_secret, csrf_secret) = mint_web_session(&state, &pool, principal_id).await?;
    sqlx::query(
        r#"
        INSERT INTO auth_audit_events
            (id, actor_principal_id, event_type, outcome, target_principal_id,
             metadata_json, recorded_at)
        VALUES ($1, $2, 'oidc.login', 'success', $2, '{}'::jsonb, now())
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(principal_id)
    .execute(&pool)
    .await
    .map_err(ApiError::sql)?;

    let target = validate_return_to(return_to.as_deref())?;
    let secure = state
        .config
        .auth
        .public_base_url
        .as_deref()
        .is_some_and(|url| url.starts_with("https://"));
    let max_age = i64::try_from(state.config.auth.session_ttl.as_secs())
        .map_err(|_| ApiError::internal("auth session ttl is too large"))?;
    let mut response = Redirect::to(&target).into_response();
    append_cookie(
        response.headers_mut(),
        SESSION_COOKIE_NAME,
        &session_secret,
        max_age,
        true,
        secure,
    )?;
    append_cookie(
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

/// Creates an auth_web_sessions row for the principal and returns the
/// (session, csrf) secrets. Shared by the OIDC callback and the single-user
/// session bootstrap.
pub(crate) async fn mint_web_session(
    state: &AppState,
    pool: &sqlx::PgPool,
    principal_id: Uuid,
) -> Result<(String, String), ApiError> {
    let session_secret = format!("mls_{}", random_secret(32));
    let csrf_secret = format!("mlc_{}", random_secret(24));
    let expires_at = Utc::now()
        + Duration::from_std(state.config.auth.session_ttl)
            .map_err(|_| ApiError::internal("auth session ttl is too large"))?;
    sqlx::query(
        r#"
        INSERT INTO auth_web_sessions
            (id, principal_id, session_hash, csrf_hash, created_at, expires_at, last_seen_at)
        VALUES ($1, $2, $3, $4, now(), $5, now())
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(principal_id)
    .bind(hash_secret(&session_secret))
    .bind(hash_secret(&csrf_secret))
    .bind(expires_at)
    .execute(pool)
    .await
    .map_err(ApiError::sql)?;
    Ok((session_secret, csrf_secret))
}

async fn discover_client(state: &AppState) -> Result<DiscoveredCoreClient, ApiError> {
    let oidc = &state.config.auth.oidc;
    let issuer = IssuerUrl::new(oidc.issuer_url.clone()).map_err(|error| {
        ApiError::status_message(
            StatusCode::SERVICE_UNAVAILABLE,
            format!("invalid Authentik issuer URL: {error}"),
        )
    })?;
    let metadata = CoreProviderMetadata::discover_async(issuer, &oidc_http_client()?)
        .await
        .map_err(|error| {
            ApiError::status_message(
                StatusCode::SERVICE_UNAVAILABLE,
                format!("Authentik OIDC discovery failed: {error}"),
            )
        })?;
    let redirect = format!(
        "{}/v1/auth/callback",
        state
            .config
            .auth
            .public_base_url
            .as_deref()
            .expect("validated public base URL")
            .trim_end_matches('/')
    );
    let client_secret = state
        .config
        .credential_env_value(&oidc.client_secret_env)
        .ok_or_else(|| {
            ApiError::service_unavailable(
                "Authentik OIDC client secret is missing from the process environment or adjacent memory-layer.env",
            )
        })?;
    Ok(CoreClient::from_provider_metadata(
        metadata,
        ClientId::new(oidc.client_id.clone()),
        Some(ClientSecret::new(client_secret)),
    )
    .set_redirect_uri(
        RedirectUrl::new(redirect)
            .map_err(|error| ApiError::internal(&format!("invalid OIDC redirect URL: {error}")))?,
    ))
}

fn oidc_http_client() -> Result<openidconnect::reqwest::Client, ApiError> {
    openidconnect::reqwest::ClientBuilder::new()
        .redirect(openidconnect::reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| ApiError::internal(&format!("build OIDC HTTP client: {error}")))
}

fn require_multiuser_oidc(state: &AppState) -> Result<(), ApiError> {
    if state.config.auth.mode != AuthMode::MultiUser {
        return Err(ApiError::not_found(
            "OIDC login is disabled in single-user mode",
        ));
    }
    let auth = &state.config.auth;
    if auth.public_base_url.as_deref().is_none_or(str::is_empty)
        || auth.oidc.issuer_url.trim().is_empty()
        || auth.oidc.client_id.trim().is_empty()
    {
        return Err(ApiError::service_unavailable(
            "multiuser OIDC requires auth.public_base_url, auth.oidc.issuer_url, and auth.oidc.client_id",
        ));
    }
    Ok(())
}

fn validate_return_to(value: Option<&str>) -> Result<String, ApiError> {
    let value = value.unwrap_or("/");
    if !value.starts_with('/') || value.starts_with("//") || value.contains(['\r', '\n']) {
        return Err(ApiError::status_message(
            StatusCode::BAD_REQUEST,
            "return_to must be a local absolute path",
        ));
    }
    Ok(value.to_string())
}

fn groups_from_id_token(token: &str, claim_name: &str) -> Result<Vec<String>, ApiError> {
    let payload = token.split('.').nth(1).ok_or_else(|| {
        ApiError::status_message(StatusCode::UNAUTHORIZED, "OIDC ID token is malformed")
    })?;
    let decoded = URL_SAFE_NO_PAD.decode(payload).map_err(|_| {
        ApiError::status_message(
            StatusCode::UNAUTHORIZED,
            "OIDC ID token payload is malformed",
        )
    })?;
    let claims: serde_json::Value = serde_json::from_slice(&decoded).map_err(|_| {
        ApiError::status_message(
            StatusCode::UNAUTHORIZED,
            "OIDC ID token claims are malformed",
        )
    })?;
    let mut groups = claims
        .get(claim_name)
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    groups.sort();
    Ok(groups)
}

fn additional_oidc_scopes(configured: &[String]) -> Vec<String> {
    configured
        .iter()
        .map(|scope| scope.trim())
        .filter(|scope| !scope.is_empty() && !scope.eq_ignore_ascii_case("openid"))
        .map(str::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

async fn upsert_human_principal(
    pool: &sqlx::PgPool,
    issuer: &str,
    subject: &str,
    display_name: &str,
    email: Option<&str>,
    groups: &[String],
) -> Result<Uuid, ApiError> {
    let id = Uuid::new_v4();
    let row = sqlx::query(
        r#"
        INSERT INTO auth_principals
            (id, kind, issuer, subject, display_name, email, groups_json,
             global_role, created_at, updated_at)
        VALUES ($1, 'human_oidc', $2, $3, $4, $5, $6, NULL, now(), now())
        ON CONFLICT (issuer, subject) WHERE issuer IS NOT NULL AND subject IS NOT NULL
        DO UPDATE SET display_name = EXCLUDED.display_name, email = EXCLUDED.email,
                      groups_json = EXCLUDED.groups_json,
                      global_role = NULL, updated_at = now()
        RETURNING id
        "#,
    )
    .bind(id)
    .bind(issuer)
    .bind(subject)
    .bind(display_name)
    .bind(email)
    .bind(serde_json::json!(groups))
    .fetch_one(pool)
    .await
    .map_err(ApiError::sql)?;
    row.try_get("id").map_err(ApiError::sql)
}

pub(crate) fn append_cookie(
    headers: &mut axum::http::HeaderMap,
    name: &str,
    value: &str,
    max_age: i64,
    http_only: bool,
    secure: bool,
) -> Result<(), ApiError> {
    let mut cookie = format!("{name}={value}; Path=/; Max-Age={max_age}; SameSite=Lax");
    if http_only {
        cookie.push_str("; HttpOnly");
    }
    if secure {
        cookie.push_str("; Secure");
    }
    headers.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie)
            .map_err(|_| ApiError::internal("failed to build authentication cookie"))?,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, Router, response::Redirect, routing::get};

    #[test]
    fn return_target_rejects_external_redirects() {
        assert_eq!(validate_return_to(Some("/graph")).unwrap(), "/graph");
        assert!(validate_return_to(Some("https://example.test")).is_err());
        assert!(validate_return_to(Some("//example.test/path")).is_err());
    }

    #[test]
    fn additional_scopes_omit_openid_blanks_and_duplicates() {
        let configured = vec![
            "openid".to_string(),
            " profile ".to_string(),
            "email".to_string(),
            "profile".to_string(),
            " ".to_string(),
        ];

        assert_eq!(
            additional_oidc_scopes(&configured),
            vec!["email".to_string(), "profile".to_string()]
        );
    }

    #[test]
    fn groups_are_read_from_configured_claim() {
        let payload = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&serde_json::json!({
                "custom_groups": ["writers", "admins", "writers"]
            }))
            .unwrap(),
        );
        let token = format!("header.{payload}.signature");
        assert_eq!(
            groups_from_id_token(&token, "custom_groups").unwrap(),
            ["admins", "writers"]
        );
    }

    #[tokio::test]
    async fn oidc_http_client_does_not_follow_redirects() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test listener");
        let address = listener.local_addr().expect("test listener address");
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route("/start", get(|| async { Redirect::temporary("/target") })),
            )
            .await
        });

        let response = oidc_http_client()
            .expect("OIDC client")
            .get(format!("http://{address}/start"))
            .send()
            .await
            .expect("redirect response");

        assert_eq!(response.status(), reqwest::StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(response.url().path(), "/start");
        server.abort();
    }

    #[tokio::test]
    async fn oidc_discovery_fetches_provider_metadata_and_jwks() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test listener");
        let address = listener.local_addr().expect("test listener address");
        let issuer = format!("http://{address}");
        let discovery_issuer = issuer.clone();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .route(
                        "/.well-known/openid-configuration",
                        get(move || {
                            let issuer = discovery_issuer.clone();
                            async move {
                                Json(serde_json::json!({
                                    "issuer": issuer,
                                    "authorization_endpoint": format!("{issuer}/authorize"),
                                    "token_endpoint": format!("{issuer}/token"),
                                    "jwks_uri": format!("{issuer}/jwks"),
                                    "response_types_supported": ["code"],
                                    "subject_types_supported": ["public"],
                                    "id_token_signing_alg_values_supported": ["RS256"]
                                }))
                            }
                        }),
                    )
                    .route(
                        "/jwks",
                        get(|| async { Json(serde_json::json!({ "keys": [] })) }),
                    ),
            )
            .await
        });

        let metadata = CoreProviderMetadata::discover_async(
            IssuerUrl::new(issuer.clone()).expect("issuer URL"),
            &oidc_http_client().expect("OIDC client"),
        )
        .await
        .expect("discover provider metadata");

        assert_eq!(metadata.issuer().as_str(), issuer);
        assert!(metadata.jwks().keys().is_empty());
        server.abort();
    }
}
