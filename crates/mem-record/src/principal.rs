// SPDX-License-Identifier: AGPL-3.0-or-later

#[allow(unused_imports)]
use std::{
    collections::HashMap,
    fmt,
    path::{Path, PathBuf},
    time::Duration,
};

#[allow(unused_imports)]
use chrono::{DateTime, Utc};
#[allow(unused_imports)]
use serde::{Deserialize, Serialize};
#[allow(unused_imports)]
use thiserror::Error;
#[allow(unused_imports)]
use uuid::Uuid;

#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    #[default]
    SingleUser,
    MultiUser,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthRole {
    Reader,
    Writer,
    Operator,
    Admin,
}

impl AuthRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reader => "reader",
            Self::Writer => "writer",
            Self::Operator => "operator",
            Self::Admin => "admin",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthPrincipalKind {
    HumanOidc,
    ServiceToken,
    LegacyServiceToken,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthProjectAccess {
    pub project: String,
    pub role: AuthRole,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthPrincipalResponse {
    pub id: Uuid,
    pub kind: AuthPrincipalKind,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global_role: Option<AuthRole>,
    #[serde(default)]
    pub projects: Vec<AuthProjectAccess>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthMeResponse {
    pub authenticated: bool,
    pub mode: AuthMode,
    pub read_only: bool,
    pub principal: AuthPrincipalResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthServiceTokenCreateRequest {
    pub name: String,
    pub project: String,
    pub role: AuthRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_in_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthServiceTokenResponse {
    pub id: Uuid,
    pub principal_id: Uuid,
    pub name: String,
    pub token_prefix: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(default)]
    pub projects: Vec<AuthProjectAccess>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthMembershipGrantRequest {
    pub principal_id: Uuid,
    pub project: String,
    pub role: AuthRole,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthMembershipResponse {
    pub id: Uuid,
    pub principal_id: Uuid,
    pub project: String,
    pub role: AuthRole,
    pub source: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}
