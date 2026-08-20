// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::BTreeMap;

use axum::http::{HeaderMap, header};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{Duration, Utc};
use mem_record::{
    AuthMembershipGrantRequest, AuthMembershipResponse, AuthMode, AuthPrincipalKind,
    AuthPrincipalResponse, AuthProjectAccess, AuthRole, AuthServiceTokenCreateRequest,
    AuthServiceTokenResponse, PermissionSet,
};
use rand::RngCore;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use super::{
    AuthenticatedPrincipal, CredentialSource, ProjectRoleGrant, max_role, parse_principal_kind,
    parse_role,
};
use crate::{ApiError, AppState};

pub(crate) const SESSION_COOKIE_NAME: &str = "memory_session";
pub(crate) const CSRF_COOKIE_NAME: &str = "memory_csrf";

/// One credential presented by a request, independent of the transport that
/// carried it (HTTP headers, websocket message, future federation channel).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Credential {
    None,
    ApiToken(String),
    BrowserSession(String),
}

/// Whether a transport adapter may read browser-session cookies. HTTP allows
/// them; machine transports (MCP, streams) ignore them by design.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CookiePolicy {
    Allow,
    Ignore,
}

pub(crate) fn hash_secret(secret: &str) -> Vec<u8> {
    Sha256::digest(secret.as_bytes()).to_vec()
}

/// Transport adapter: extract the single presented credential from HTTP
/// headers, rejecting ambiguous combinations here so `resolve_principal`
/// never sees them.
pub(crate) fn credential_from_headers(
    headers: &HeaderMap,
    cookies: CookiePolicy,
) -> Result<Credential, ApiError> {
    let x_api_token = headers
        .get("x-api-token")
        .map(|value| {
            value
                .to_str()
                .map(str::to_owned)
                .map_err(|_| ApiError::unauthorized("invalid x-api-token header"))
        })
        .transpose()?;
    let bearer = headers
        .get(header::AUTHORIZATION)
        .map(|value| {
            let value = value
                .to_str()
                .map_err(|_| ApiError::unauthorized("invalid authorization header"))?;
            value
                .strip_prefix("Bearer ")
                .filter(|token| !token.trim().is_empty())
                .map(str::to_owned)
                .ok_or_else(|| {
                    ApiError::unauthorized("authorization header must use Bearer authentication")
                })
        })
        .transpose()?;

    let api_token = match (x_api_token, bearer) {
        (Some(left), Some(right)) if left != right => {
            return Err(ApiError::unauthorized(
                "conflicting x-api-token and bearer credentials",
            ));
        }
        (Some(token), _) | (_, Some(token)) => Some(token),
        (None, None) => None,
    };

    let session_token = match cookies {
        CookiePolicy::Allow => cookie_value(headers, SESSION_COOKIE_NAME),
        CookiePolicy::Ignore => None,
    };

    match (api_token, session_token) {
        (Some(_), Some(_)) => Err(ApiError::unauthorized(
            "conflicting api-token and browser-session credentials",
        )),
        (Some(token), None) => Ok(Credential::ApiToken(token)),
        (None, Some(session)) => Ok(Credential::BrowserSession(session)),
        (None, None) => Ok(Credential::None),
    }
}

/// Resolve one presented credential to a principal, independent of transport.
pub(crate) async fn resolve_principal(
    state: &AppState,
    credential: Credential,
) -> Result<Option<AuthenticatedPrincipal>, ApiError> {
    match state.config.auth.mode {
        AuthMode::SingleUser => match credential {
            Credential::BrowserSession(token) => {
                // Single-user browser sessions exist for the bundled web UI;
                // they resolve exactly like multi-user sessions.
                let pool = state.pool()?;
                load_session_principal(&pool, state, &token).await.map(Some)
            }
            Credential::ApiToken(token) if token == state.api_token => {
                Ok(Some(local_owner_admin()))
            }
            Credential::ApiToken(_) => Err(ApiError::unauthorized("invalid api token")),
            Credential::None => Ok(Some(single_user_reader())),
        },
        AuthMode::MultiUser => match credential {
            Credential::ApiToken(token) => {
                if state.config.auth.multi_user_legacy_token_enabled && token == state.api_token {
                    return Ok(Some(legacy_principal()));
                }
                let pool = state.pool()?;
                load_service_token_principal(&pool, state, &token)
                    .await
                    .map(Some)
            }
            Credential::BrowserSession(token) => {
                let pool = state.pool()?;
                load_session_principal(&pool, state, &token).await.map(Some)
            }
            Credential::None => Ok(None),
        },
    }
}

/// HTTP convenience wrapper: adapter + resolution in one call.
pub(crate) async fn resolve_request_principal(
    state: &AppState,
    headers: &HeaderMap,
    cookies: CookiePolicy,
) -> Result<Option<AuthenticatedPrincipal>, ApiError> {
    let credential = credential_from_headers(headers, cookies)?;
    resolve_principal(state, credential).await
}

pub(crate) fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|part| part.trim().split_once('='))
        .find(|(candidate, _)| *candidate == name)
        .map(|(_, value)| value.to_string())
}

/// Stable identity of the machine-local owner in single-user mode. The row
/// is bootstrapped at startup so single-user actions are attributable.
pub(crate) fn local_owner_id() -> Uuid {
    Uuid::new_v5(&Uuid::NAMESPACE_OID, b"memory-layer-local-owner")
}

/// Ensures the local-owner principal row exists (idempotent).
pub(crate) async fn ensure_local_owner_principal(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO auth_principals (id, kind, display_name, groups_json, global_role)
        VALUES ($1, 'internal', 'local owner', '[]'::jsonb, 'admin')
        ON CONFLICT (id) DO NOTHING
        "#,
    )
    .bind(local_owner_id())
    .execute(pool)
    .await
    .map(drop)
}

fn single_user_reader() -> AuthenticatedPrincipal {
    AuthenticatedPrincipal {
        id: local_owner_id(),
        kind: AuthPrincipalKind::Internal,
        display_name: "local owner".to_string(),
        email: None,
        issuer: None,
        subject: None,
        groups: Vec::new(),
        global_role: Some(AuthRole::Reader),
        global: AuthRole::Reader.permissions(),
        project_roles: BTreeMap::new(),
        credential_source: CredentialSource::AnonymousSingleUser,
        token_id: None,
        session_id: None,
        session_csrf_hash: None,
    }
}

fn legacy_principal() -> AuthenticatedPrincipal {
    AuthenticatedPrincipal {
        id: Uuid::new_v5(&Uuid::NAMESPACE_OID, b"memory-layer-legacy-service-token"),
        kind: AuthPrincipalKind::LegacyServiceToken,
        display_name: "legacy service token".to_string(),
        email: None,
        issuer: None,
        subject: None,
        groups: Vec::new(),
        global_role: Some(AuthRole::Admin),
        global: AuthRole::Admin.permissions(),
        project_roles: BTreeMap::new(),
        credential_source: CredentialSource::LegacyServiceToken,
        token_id: None,
        session_id: None,
        session_csrf_hash: None,
    }
}

fn local_owner_admin() -> AuthenticatedPrincipal {
    AuthenticatedPrincipal {
        id: local_owner_id(),
        kind: AuthPrincipalKind::Internal,
        display_name: "local owner".to_string(),
        email: None,
        issuer: None,
        subject: None,
        groups: Vec::new(),
        global_role: Some(AuthRole::Admin),
        global: AuthRole::Admin.permissions(),
        project_roles: BTreeMap::new(),
        credential_source: CredentialSource::LocalApiToken,
        token_id: None,
        session_id: None,
        session_csrf_hash: None,
    }
}

/// Debounces last_used_at/last_seen_at writes: touching these rows on every
/// authenticated request doubles write traffic for zero audit value at
/// sub-minute granularity.
fn should_touch_last_used(id: Uuid) -> bool {
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    static LAST_TOUCH: Mutex<Option<HashMap<Uuid, Instant>>> = Mutex::new(None);
    let mut guard = LAST_TOUCH.lock().expect("last-touch cache lock poisoned");
    let cache = guard.get_or_insert_with(HashMap::new);
    let now = Instant::now();
    match cache.get(&id) {
        Some(last) if now.duration_since(*last) < Duration::from_secs(60) => false,
        _ => {
            if cache.len() > 4096 {
                cache.clear();
            }
            cache.insert(id, now);
            true
        }
    }
}

async fn load_service_token_principal(
    pool: &PgPool,
    state: &AppState,
    token: &str,
) -> Result<AuthenticatedPrincipal, ApiError> {
    let row = sqlx::query(
        r#"
        SELECT p.id, p.kind, p.display_name, p.email, p.issuer, p.subject, p.groups_json, p.global_role, p.global_permissions_json,
               t.id AS token_id
        FROM auth_service_tokens t
        JOIN auth_principals p ON p.id = t.principal_id
        WHERE t.token_hash = $1
          AND t.revoked_at IS NULL
          AND (t.expires_at IS NULL OR t.expires_at > now())
          AND p.disabled_at IS NULL
        "#,
    )
    .bind(hash_secret(token))
    .fetch_optional(pool)
    .await
    .map_err(ApiError::sql)?
    .ok_or_else(|| ApiError::unauthorized("invalid or expired service token"))?;
    let token_id: Uuid = row.try_get("token_id").map_err(ApiError::sql)?;
    let mut principal = principal_from_row(pool, state, &row).await?;
    principal.credential_source = CredentialSource::ServiceToken;
    principal.token_id = Some(token_id);
    if should_touch_last_used(token_id) {
        sqlx::query("UPDATE auth_service_tokens SET last_used_at = now() WHERE id = $1")
            .bind(token_id)
            .execute(pool)
            .await
            .map_err(ApiError::sql)?;
    }
    Ok(principal)
}

async fn load_session_principal(
    pool: &PgPool,
    state: &AppState,
    token: &str,
) -> Result<AuthenticatedPrincipal, ApiError> {
    let row = sqlx::query(
        r#"
        SELECT p.id, p.kind, p.display_name, p.email, p.issuer, p.subject, p.groups_json, p.global_role, p.global_permissions_json,
               s.id AS session_id, s.csrf_hash
        FROM auth_web_sessions s
        JOIN auth_principals p ON p.id = s.principal_id
        WHERE s.session_hash = $1
          AND s.revoked_at IS NULL
          AND s.expires_at > now()
          AND p.disabled_at IS NULL
        "#,
    )
    .bind(hash_secret(token))
    .fetch_optional(pool)
    .await
    .map_err(ApiError::sql)?
    .ok_or_else(|| ApiError::unauthorized("invalid or expired browser session"))?;
    let session_id: Uuid = row.try_get("session_id").map_err(ApiError::sql)?;
    let csrf_hash: Vec<u8> = row.try_get("csrf_hash").map_err(ApiError::sql)?;
    let mut principal = principal_from_row(pool, state, &row).await?;
    principal.credential_source = CredentialSource::BrowserSession;
    principal.session_id = Some(session_id);
    principal.session_csrf_hash = Some(csrf_hash);
    if should_touch_last_used(session_id) {
        sqlx::query("UPDATE auth_web_sessions SET last_seen_at = now() WHERE id = $1")
            .bind(session_id)
            .execute(pool)
            .await
            .map_err(ApiError::sql)?;
    }
    Ok(principal)
}

async fn principal_from_row(
    pool: &PgPool,
    state: &AppState,
    row: &sqlx::postgres::PgRow,
) -> Result<AuthenticatedPrincipal, ApiError> {
    let id: Uuid = row.try_get("id").map_err(ApiError::sql)?;
    let display_name: String = row.try_get("display_name").map_err(ApiError::sql)?;
    let email: Option<String> = row.try_get("email").map_err(ApiError::sql)?;
    let issuer: Option<String> = row.try_get("issuer").map_err(ApiError::sql)?;
    let subject: Option<String> = row.try_get("subject").map_err(ApiError::sql)?;
    let kind = parse_principal_kind(
        row.try_get::<String, _>("kind")
            .map_err(ApiError::sql)?
            .as_str(),
    )
    .ok_or_else(|| ApiError::internal("invalid principal kind in database"))?;
    let groups_value: serde_json::Value = row.try_get("groups_json").map_err(ApiError::sql)?;
    let groups = serde_json::from_value::<Vec<String>>(groups_value)
        .map_err(|_| ApiError::internal("invalid principal groups in database"))?;
    let stored_global_role = row
        .try_get::<Option<String>, _>("global_role")
        .map_err(ApiError::sql)?
        .as_deref()
        .and_then(parse_role);
    let mut global_role = if kind == AuthPrincipalKind::HumanOidc {
        None
    } else {
        stored_global_role
    };
    let stored_global_permissions = permission_set_override(
        row.try_get::<Option<serde_json::Value>, _>("global_permissions_json")
            .map_err(ApiError::sql)?,
    )?;
    let mut project_roles = load_memberships(pool, id).await?;
    let mut group_global_roles: Vec<AuthRole> = Vec::new();

    for rule in &state.config.auth.group_mappings.rules {
        if !groups.iter().any(|group| group == &rule.group) {
            continue;
        }
        if rule.global {
            global_role = max_role(global_role, Some(rule.role));
            group_global_roles.push(rule.role);
        }
        if let Some(project) = rule.project.as_deref() {
            merge_project_grant(
                &mut project_roles,
                project,
                rule.role,
                rule.role.permissions(),
                "oidc_group",
            );
        }
    }

    // Effective global set: explicit permissions override the preset when
    // present; group-mapping rules always union their presets on top.
    let global_permissions = stored_global_permissions
        .unwrap_or_else(|| {
            global_role
                .map(AuthRole::permissions)
                .unwrap_or(PermissionSet::EMPTY)
        })
        .union(
            group_global_roles
                .iter()
                .fold(PermissionSet::EMPTY, |set, role| {
                    set.union(role.permissions())
                }),
        );

    Ok(AuthenticatedPrincipal {
        id,
        kind,
        display_name,
        email,
        issuer,
        subject,
        groups,
        global_role,
        global: global_permissions,
        project_roles,
        credential_source: CredentialSource::ServiceToken,
        token_id: None,
        session_id: None,
        session_csrf_hash: None,
    })
}

async fn load_memberships(
    pool: &PgPool,
    principal_id: Uuid,
) -> Result<BTreeMap<String, ProjectRoleGrant>, ApiError> {
    let rows = sqlx::query(
        r#"
        SELECT p.slug, m.role, m.source, m.permissions_json
        FROM auth_project_memberships m
        JOIN projects p ON p.id = m.project_id
        WHERE m.principal_id = $1
        ORDER BY p.slug
        "#,
    )
    .bind(principal_id)
    .fetch_all(pool)
    .await
    .map_err(ApiError::sql)?;
    let mut roles = BTreeMap::new();
    for row in rows {
        let project: String = row.try_get("slug").map_err(ApiError::sql)?;
        let role_value: String = row.try_get("role").map_err(ApiError::sql)?;
        if role_value == "custom" {
            return Err(ApiError::internal(
                "custom memberships are not supported by this build yet",
            ));
        }
        let role = parse_role(&role_value)
            .ok_or_else(|| ApiError::internal("invalid project role in database"))?;
        let permissions = permission_set_override(
            row.try_get::<Option<serde_json::Value>, _>("permissions_json")
                .map_err(ApiError::sql)?,
        )?
        .unwrap_or_else(|| role.permissions());
        let source: String = row.try_get("source").map_err(ApiError::sql)?;
        merge_project_grant(&mut roles, &project, role, permissions, &source);
    }
    Ok(roles)
}

fn merge_project_grant(
    roles: &mut BTreeMap<String, ProjectRoleGrant>,
    project: &str,
    role: AuthRole,
    permissions: PermissionSet,
    source: &str,
) {
    match roles.get_mut(project) {
        Some(existing) if existing.role < role => {
            existing.role = role;
            existing.permissions = existing.permissions.union(permissions);
            existing.source = source.to_string();
        }
        Some(existing) => {
            existing.permissions = existing.permissions.union(permissions);
        }
        None => {
            roles.insert(
                project.to_string(),
                ProjectRoleGrant {
                    role,
                    permissions,
                    source: source.to_string(),
                },
            );
        }
    }
}

/// Parses an optional permissions_json array into a set; None passes through.
fn permission_set_override(
    value: Option<serde_json::Value>,
) -> Result<Option<PermissionSet>, ApiError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let names: Vec<String> = serde_json::from_value(value)
        .map_err(|_| ApiError::internal("invalid permissions_json in database"))?;
    let mut set = PermissionSet::EMPTY;
    for name in names {
        let permission = mem_record::Permission::parse(&name)
            .ok_or_else(|| ApiError::internal("unknown permission name in permissions_json"))?;
        set = set.with(permission);
    }
    Ok(Some(set))
}

pub(crate) async fn list_principals(
    pool: &PgPool,
    state: &AppState,
) -> Result<Vec<AuthPrincipalResponse>, ApiError> {
    let rows = sqlx::query(
        r#"
        SELECT id, kind, display_name, email, groups_json, global_role
        FROM auth_principals
        WHERE disabled_at IS NULL
        ORDER BY display_name, id
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(ApiError::sql)?;
    let mut principals = Vec::with_capacity(rows.len());
    for row in rows {
        principals.push(principal_from_row(pool, state, &row).await?.to_response());
    }
    Ok(principals)
}

pub(crate) async fn create_service_token(
    pool: &PgPool,
    actor: &AuthenticatedPrincipal,
    request: AuthServiceTokenCreateRequest,
) -> Result<AuthServiceTokenResponse, ApiError> {
    let name = request.name.trim();
    let project = request.project.trim();
    if name.is_empty() {
        return Err(ApiError::validation(mem_record::ValidationError::new(
            "token name must be non-empty",
        )));
    }
    if project.is_empty() {
        return Err(ApiError::validation(mem_record::ValidationError::new(
            "project must be non-empty",
        )));
    }
    if request.role == AuthRole::Admin {
        return Err(ApiError::validation(mem_record::ValidationError::new(
            "service tokens cannot receive the admin role",
        )));
    }
    let expires_at = request
        .expires_in_seconds
        .map(|seconds| {
            if seconds == 0 || seconds > 366 * 24 * 60 * 60 {
                return Err(ApiError::validation(mem_record::ValidationError::new(
                    "token ttl must be between 1 second and 366 days",
                )));
            }
            let seconds = i64::try_from(seconds).map_err(|_| {
                ApiError::validation(mem_record::ValidationError::new("token ttl is too large"))
            })?;
            Ok(Utc::now() + Duration::seconds(seconds))
        })
        .transpose()?;
    let project_id: Uuid = sqlx::query("SELECT id FROM projects WHERE slug = $1")
        .bind(project)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::sql)?
        .ok_or_else(|| ApiError::not_found("project not found"))?
        .try_get("id")
        .map_err(ApiError::sql)?;

    let token = format!("mlt_{}", random_secret(32));
    let token_prefix = token.chars().take(12).collect::<String>();
    let principal_id = Uuid::new_v4();
    let token_id = Uuid::new_v4();
    let membership_id = Uuid::new_v4();
    let actor_id = persisted_actor_id(actor);
    let mut tx = pool.begin().await.map_err(ApiError::sql)?;
    sqlx::query(
        r#"
        INSERT INTO auth_principals
            (id, kind, display_name, groups_json, created_at, updated_at)
        VALUES ($1, 'service_token', $2, '[]'::jsonb, now(), now())
        "#,
    )
    .bind(principal_id)
    .bind(name)
    .execute(&mut *tx)
    .await
    .map_err(ApiError::sql)?;
    sqlx::query(
        r#"
        INSERT INTO auth_service_tokens
            (id, principal_id, name, token_hash, token_prefix, created_by, created_at, expires_at)
        VALUES ($1, $2, $3, $4, $5, $6, now(), $7)
        "#,
    )
    .bind(token_id)
    .bind(principal_id)
    .bind(name)
    .bind(hash_secret(&token))
    .bind(&token_prefix)
    .bind(actor_id)
    .bind(expires_at)
    .execute(&mut *tx)
    .await
    .map_err(ApiError::sql)?;
    sqlx::query(
        r#"
        INSERT INTO auth_project_memberships
            (id, principal_id, project_id, role, source, created_by, created_at, updated_at)
        VALUES ($1, $2, $3, $4, 'bootstrap', $5, now(), now())
        "#,
    )
    .bind(membership_id)
    .bind(principal_id)
    .bind(project_id)
    .bind(request.role.as_str())
    .bind(actor_id)
    .execute(&mut *tx)
    .await
    .map_err(ApiError::sql)?;
    insert_audit_tx(
        &mut tx,
        actor_id,
        "service_token.create",
        Some(project_id),
        Some(principal_id),
        Some(token_id),
        serde_json::json!({"name": name, "role": request.role.as_str()}),
    )
    .await?;
    tx.commit().await.map_err(ApiError::sql)?;

    Ok(AuthServiceTokenResponse {
        id: token_id,
        principal_id,
        name: name.to_string(),
        token_prefix,
        created_at: Utc::now(),
        expires_at,
        revoked_at: None,
        last_used_at: None,
        token: Some(token),
        projects: vec![AuthProjectAccess {
            project: project.to_string(),
            role: request.role,
            permissions: request.role.permissions().names(),
            source: "bootstrap".to_string(),
        }],
    })
}

pub(crate) async fn list_service_tokens(
    pool: &PgPool,
) -> Result<Vec<AuthServiceTokenResponse>, ApiError> {
    let rows = sqlx::query(
        r#"
        SELECT t.id, t.principal_id, t.name, t.token_prefix, t.created_at,
               t.expires_at, t.revoked_at, t.last_used_at
        FROM auth_service_tokens t
        ORDER BY t.created_at DESC
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(ApiError::sql)?;
    let mut tokens = Vec::with_capacity(rows.len());
    for row in rows {
        let principal_id: Uuid = row.try_get("principal_id").map_err(ApiError::sql)?;
        tokens.push(AuthServiceTokenResponse {
            id: row.try_get("id").map_err(ApiError::sql)?,
            principal_id,
            name: row.try_get("name").map_err(ApiError::sql)?,
            token_prefix: row.try_get("token_prefix").map_err(ApiError::sql)?,
            created_at: row.try_get("created_at").map_err(ApiError::sql)?,
            expires_at: row.try_get("expires_at").map_err(ApiError::sql)?,
            revoked_at: row.try_get("revoked_at").map_err(ApiError::sql)?,
            last_used_at: row.try_get("last_used_at").map_err(ApiError::sql)?,
            token: None,
            projects: project_access_for_principal(pool, principal_id).await?,
        });
    }
    Ok(tokens)
}

pub(crate) async fn revoke_service_token(
    pool: &PgPool,
    actor: &AuthenticatedPrincipal,
    selector: &str,
) -> Result<AuthServiceTokenResponse, ApiError> {
    let selector = selector.trim();
    if selector.is_empty() {
        return Err(ApiError::validation(mem_record::ValidationError::new(
            "token id or prefix must be non-empty",
        )));
    }
    let id = Uuid::parse_str(selector).ok();
    let rows = sqlx::query(
        r#"
        SELECT id, principal_id, name, token_prefix, created_at, expires_at,
               revoked_at, last_used_at
        FROM auth_service_tokens
        WHERE ($1::uuid IS NOT NULL AND id = $1)
           OR ($1::uuid IS NULL AND token_prefix LIKE $2 || '%')
        ORDER BY created_at DESC
        LIMIT 2
        "#,
    )
    .bind(id)
    .bind(selector)
    .fetch_all(pool)
    .await
    .map_err(ApiError::sql)?;
    if rows.is_empty() {
        return Err(ApiError::not_found("service token not found"));
    }
    if rows.len() > 1 {
        return Err(ApiError::status_message(
            axum::http::StatusCode::CONFLICT,
            "service token prefix is ambiguous; use the full token id",
        ));
    }
    let row = &rows[0];
    let token_id: Uuid = row.try_get("id").map_err(ApiError::sql)?;
    let principal_id: Uuid = row.try_get("principal_id").map_err(ApiError::sql)?;
    sqlx::query(
        "UPDATE auth_service_tokens SET revoked_at = COALESCE(revoked_at, now()) WHERE id = $1",
    )
    .bind(token_id)
    .execute(pool)
    .await
    .map_err(ApiError::sql)?;
    insert_audit(
        pool,
        persisted_actor_id(actor),
        "service_token.revoke",
        None,
        Some(principal_id),
        Some(token_id),
        serde_json::json!({}),
    )
    .await?;
    Ok(AuthServiceTokenResponse {
        id: token_id,
        principal_id,
        name: row.try_get("name").map_err(ApiError::sql)?,
        token_prefix: row.try_get("token_prefix").map_err(ApiError::sql)?,
        created_at: row.try_get("created_at").map_err(ApiError::sql)?,
        expires_at: row.try_get("expires_at").map_err(ApiError::sql)?,
        revoked_at: Some(Utc::now()),
        last_used_at: row.try_get("last_used_at").map_err(ApiError::sql)?,
        token: None,
        projects: project_access_for_principal(pool, principal_id).await?,
    })
}

pub(crate) async fn grant_membership(
    pool: &PgPool,
    actor: &AuthenticatedPrincipal,
    request: AuthMembershipGrantRequest,
) -> Result<AuthMembershipResponse, ApiError> {
    let project = request.project.trim();
    if project.is_empty() {
        return Err(ApiError::validation(mem_record::ValidationError::new(
            "project must be non-empty",
        )));
    }
    let principal_kind: String = sqlx::query("SELECT kind FROM auth_principals WHERE id = $1")
        .bind(request.principal_id)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::sql)?
        .ok_or_else(|| ApiError::not_found("principal not found"))?
        .try_get("kind")
        .map_err(ApiError::sql)?;
    if principal_kind == "service_token" && request.role == AuthRole::Admin {
        return Err(ApiError::validation(mem_record::ValidationError::new(
            "service principals cannot receive the admin role",
        )));
    }
    let project_id: Uuid = sqlx::query("SELECT id FROM projects WHERE slug = $1")
        .bind(project)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::sql)?
        .ok_or_else(|| ApiError::not_found("project not found"))?
        .try_get("id")
        .map_err(ApiError::sql)?;
    let id = Uuid::new_v4();
    let actor_id = persisted_actor_id(actor);
    let row = sqlx::query(
        r#"
        INSERT INTO auth_project_memberships
            (id, principal_id, project_id, role, source, created_by, created_at, updated_at)
        VALUES ($1, $2, $3, $4, 'direct', $5, now(), now())
        ON CONFLICT (principal_id, project_id) DO UPDATE
        SET role = EXCLUDED.role, source = 'direct', created_by = EXCLUDED.created_by,
            updated_at = now()
        RETURNING id, created_at
        "#,
    )
    .bind(id)
    .bind(request.principal_id)
    .bind(project_id)
    .bind(request.role.as_str())
    .bind(actor_id)
    .fetch_one(pool)
    .await
    .map_err(ApiError::sql)?;
    insert_audit(
        pool,
        actor_id,
        "membership.grant",
        Some(project_id),
        Some(request.principal_id),
        None,
        serde_json::json!({"role": request.role.as_str()}),
    )
    .await?;
    Ok(AuthMembershipResponse {
        id: row.try_get("id").map_err(ApiError::sql)?,
        principal_id: request.principal_id,
        project: project.to_string(),
        role: request.role,
        source: "direct".to_string(),
        created_at: row.try_get("created_at").map_err(ApiError::sql)?,
    })
}

pub(crate) async fn list_memberships(
    pool: &PgPool,
) -> Result<Vec<AuthMembershipResponse>, ApiError> {
    let rows = sqlx::query(
        r#"
        SELECT m.id, m.principal_id, p.slug, m.role, m.source, m.created_at
        FROM auth_project_memberships m
        JOIN projects p ON p.id = m.project_id
        ORDER BY p.slug, m.created_at
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(ApiError::sql)?;
    rows.into_iter()
        .map(|row| {
            let role_value: String = row.try_get("role").map_err(ApiError::sql)?;
            Ok(AuthMembershipResponse {
                id: row.try_get("id").map_err(ApiError::sql)?,
                principal_id: row.try_get("principal_id").map_err(ApiError::sql)?,
                project: row.try_get("slug").map_err(ApiError::sql)?,
                role: parse_role(&role_value)
                    .ok_or_else(|| ApiError::internal("invalid project role in database"))?,
                source: row.try_get("source").map_err(ApiError::sql)?,
                created_at: row.try_get("created_at").map_err(ApiError::sql)?,
            })
        })
        .collect()
}

pub(crate) async fn revoke_membership(
    pool: &PgPool,
    actor: &AuthenticatedPrincipal,
    membership_id: Uuid,
) -> Result<AuthMembershipResponse, ApiError> {
    let row = sqlx::query(
        r#"
        DELETE FROM auth_project_memberships m
        USING projects p
        WHERE m.id = $1 AND p.id = m.project_id
        RETURNING m.id, m.principal_id, p.id AS project_id, p.slug, m.role,
                  m.source, m.created_at
        "#,
    )
    .bind(membership_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::sql)?
    .ok_or_else(|| ApiError::not_found("membership not found"))?;
    let role_value: String = row.try_get("role").map_err(ApiError::sql)?;
    let project_id: Uuid = row.try_get("project_id").map_err(ApiError::sql)?;
    let principal_id: Uuid = row.try_get("principal_id").map_err(ApiError::sql)?;
    insert_audit(
        pool,
        persisted_actor_id(actor),
        "membership.revoke",
        Some(project_id),
        Some(principal_id),
        None,
        serde_json::json!({}),
    )
    .await?;
    Ok(AuthMembershipResponse {
        id: membership_id,
        principal_id,
        project: row.try_get("slug").map_err(ApiError::sql)?,
        role: parse_role(&role_value)
            .ok_or_else(|| ApiError::internal("invalid project role in database"))?,
        source: row.try_get("source").map_err(ApiError::sql)?,
        created_at: row.try_get("created_at").map_err(ApiError::sql)?,
    })
}

async fn project_access_for_principal(
    pool: &PgPool,
    principal_id: Uuid,
) -> Result<Vec<AuthProjectAccess>, ApiError> {
    Ok(load_memberships(pool, principal_id)
        .await?
        .into_iter()
        .map(|(project, grant)| AuthProjectAccess {
            project,
            role: grant.role,
            permissions: grant.permissions.names(),
            source: grant.source,
        })
        .collect())
}

fn persisted_actor_id(actor: &AuthenticatedPrincipal) -> Option<Uuid> {
    // The multi-user legacy token has no principal row; the single-user
    // local owner does (bootstrapped at startup), so it persists.
    (!matches!(
        actor.credential_source,
        CredentialSource::LegacyServiceToken
    ))
    .then_some(actor.id)
}

pub(crate) fn random_secret(bytes: usize) -> String {
    let mut value = vec![0_u8; bytes];
    rand::rng().fill_bytes(&mut value);
    URL_SAFE_NO_PAD.encode(value)
}

async fn insert_audit(
    pool: &PgPool,
    actor_id: Option<Uuid>,
    event_type: &str,
    project_id: Option<Uuid>,
    target_principal_id: Option<Uuid>,
    target_token_id: Option<Uuid>,
    metadata: serde_json::Value,
) -> Result<(), ApiError> {
    sqlx::query(
        r#"
        INSERT INTO auth_audit_events
            (id, actor_principal_id, event_type, outcome, project_id,
             target_principal_id, target_token_id, metadata_json, recorded_at)
        VALUES ($1, $2, $3, 'success', $4, $5, $6, $7, now())
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(actor_id)
    .bind(event_type)
    .bind(project_id)
    .bind(target_principal_id)
    .bind(target_token_id)
    .bind(metadata)
    .execute(pool)
    .await
    .map_err(ApiError::sql)?;
    Ok(())
}

/// Records a denied or errored authorization decision. Fire-and-forget:
/// audit must never turn a denial into a 500.
pub(crate) fn record_denied_audit(
    state: &AppState,
    actor_id: Option<Uuid>,
    outcome: &'static str,
    method: &str,
    path: &str,
    reason: &str,
    request_id: Option<String>,
) {
    let Ok(pool) = state.pool() else {
        return;
    };
    let metadata = serde_json::json!({
        "method": method,
        "path": path,
        "reason": reason,
    });
    let request_id = request_id.clone();
    tokio::spawn(async move {
        let result = sqlx::query(
            r#"
            INSERT INTO auth_audit_events
                (id, actor_principal_id, event_type, outcome, metadata_json, request_id, recorded_at)
            VALUES ($1, $2, 'authorization.denied', $3, $4, $5, now())
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(actor_id)
        .bind(outcome)
        .bind(metadata)
        .bind(request_id)
        .execute(&pool)
        .await;
        if let Err(error) = result {
            tracing::warn!(error = %error, "failed to record denied authorization audit event");
        }
    });
}

async fn insert_audit_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor_id: Option<Uuid>,
    event_type: &str,
    project_id: Option<Uuid>,
    target_principal_id: Option<Uuid>,
    target_token_id: Option<Uuid>,
    metadata: serde_json::Value,
) -> Result<(), ApiError> {
    sqlx::query(
        r#"
        INSERT INTO auth_audit_events
            (id, actor_principal_id, event_type, outcome, project_id,
             target_principal_id, target_token_id, metadata_json, recorded_at)
        VALUES ($1, $2, $3, 'success', $4, $5, $6, $7, now())
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(actor_id)
    .bind(event_type)
    .bind(project_id)
    .bind(target_principal_id)
    .bind(target_token_id)
    .bind(metadata)
    .execute(&mut **tx)
    .await
    .map_err(ApiError::sql)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue};

    use super::*;

    #[test]
    fn credential_parser_accepts_matching_header_forms() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-token", HeaderValue::from_static("token"));
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer token"),
        );
        let credential =
            credential_from_headers(&headers, CookiePolicy::Ignore).expect("credential");
        assert_eq!(credential, Credential::ApiToken("token".to_string()));
    }

    #[test]
    fn machine_authentication_ignores_browser_cookies() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("memory_session=browser-secret"),
        );

        let credential =
            credential_from_headers(&headers, CookiePolicy::Ignore).expect("credential");
        assert_eq!(credential, Credential::None);
        let allowed = credential_from_headers(&headers, CookiePolicy::Allow).expect("credential");
        assert_eq!(
            allowed,
            Credential::BrowserSession("browser-secret".to_string())
        );
    }

    #[tokio::test]
    async fn service_token_admin_role_is_rejected_before_database_access() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgresql://memory:memory@127.0.0.1:1/memory")
            .expect("lazy test pool");
        let actor = legacy_principal();
        let error = create_service_token(
            &pool,
            &actor,
            AuthServiceTokenCreateRequest {
                name: "too-powerful".to_string(),
                project: "memory".to_string(),
                role: AuthRole::Admin,
                expires_in_seconds: Some(60),
            },
        )
        .await
        .expect_err("admin service token must be rejected");

        assert_eq!(error.status, axum::http::StatusCode::BAD_REQUEST);
    }

    #[test]
    fn credential_parser_rejects_conflicting_header_forms() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-token", HeaderValue::from_static("left"));
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer right"),
        );
        assert!(credential_from_headers(&headers, CookiePolicy::Ignore).is_err());
    }

    #[test]
    fn cookie_parser_reads_named_cookie() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("other=1; memory_session=session-value; memory_csrf=csrf"),
        );
        assert_eq!(
            cookie_value(&headers, SESSION_COOKIE_NAME).as_deref(),
            Some("session-value")
        );
    }

    #[test]
    fn generated_service_secrets_are_prefixed_and_nonrepeating() {
        let left = format!("mlt_{}", random_secret(32));
        let right = format!("mlt_{}", random_secret(32));
        assert!(left.starts_with("mlt_"));
        assert!(left.len() >= 46);
        assert_ne!(left, right);
        assert_ne!(hash_secret(&left), hash_secret(&right));
    }

    #[tokio::test]
    async fn service_token_lifecycle_stores_only_a_hash() {
        let Some(pool) = mem_test_support::migrated_pool().await else {
            return;
        };
        let project = mem_test_support::unique_project_slug("auth-token");
        mem_test_support::cleanup_project(&pool, &project)
            .await
            .expect("clean project before auth test");
        crate::repository::handlers::bundle::upsert_project_slug(&pool, &project)
            .await
            .expect("create auth test project");

        let created = create_service_token(
            &pool,
            &legacy_principal(),
            AuthServiceTokenCreateRequest {
                name: format!("test-{project}"),
                project: project.clone(),
                role: AuthRole::Writer,
                expires_in_seconds: Some(3600),
            },
        )
        .await
        .expect("create service token");
        let raw_token = created.token.as_deref().expect("one-time token");
        let stored: Vec<u8> =
            sqlx::query("SELECT token_hash FROM auth_service_tokens WHERE id = $1")
                .bind(created.id)
                .fetch_one(&pool)
                .await
                .expect("read stored token hash")
                .try_get("token_hash")
                .expect("token hash column");
        assert_eq!(stored, hash_secret(raw_token));
        assert_ne!(stored, raw_token.as_bytes());

        let listed = list_service_tokens(&pool)
            .await
            .expect("list service tokens")
            .into_iter()
            .find(|token| token.id == created.id)
            .expect("created token is listed");
        assert!(listed.token.is_none(), "raw token must never be listed");
        assert_eq!(listed.projects[0].project, project);

        let revoked = revoke_service_token(&pool, &legacy_principal(), &created.id.to_string())
            .await
            .expect("revoke service token");
        assert!(revoked.revoked_at.is_some());

        sqlx::query("DELETE FROM auth_principals WHERE id = $1")
            .bind(created.principal_id)
            .execute(&pool)
            .await
            .expect("remove auth test principal");
        mem_test_support::cleanup_project(&pool, &project)
            .await
            .expect("clean project after auth test");
    }
}
