// SPDX-License-Identifier: AGPL-3.0-or-later

#[allow(unused_imports)]
use std::{
    collections::HashMap,
    env, fmt,
    path::{Path, PathBuf},
    time::Duration,
};

#[allow(unused_imports)]
use chrono::{DateTime, Utc};
#[allow(unused_imports)]
use config::{Config, ConfigError, Environment, File, FileFormat};
#[allow(unused_imports)]
use mem_platform::discover_existing_global_config_path;
#[allow(unused_imports)]
use mem_record::*;
#[allow(unused_imports)]
use serde::{Deserialize, Serialize};
#[allow(unused_imports)]
use thiserror::Error;
#[allow(unused_imports)]
use uuid::Uuid;

#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcAuthConfig {
    #[serde(default)]
    pub issuer_url: String,
    #[serde(default)]
    pub client_id: String,
    #[serde(default = "default_oidc_client_secret_env")]
    pub client_secret_env: String,
    #[serde(default = "default_oidc_scopes")]
    pub scopes: Vec<String>,
    #[serde(default = "default_oidc_groups_claim")]
    pub groups_claim: String,
}

impl Default for OidcAuthConfig {
    fn default() -> Self {
        Self {
            issuer_url: String::new(),
            client_id: String::new(),
            client_secret_env: default_oidc_client_secret_env(),
            scopes: default_oidc_scopes(),
            groups_claim: default_oidc_groups_claim(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthGroupMappingRule {
    pub group: String,
    pub role: AuthRole,
    #[serde(default)]
    pub global: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthGroupMappingsConfig {
    #[serde(default)]
    pub rules: Vec<AuthGroupMappingRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    #[serde(default)]
    pub mode: AuthMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_base_url: Option<String>,
    #[serde(default = "default_auth_session_ttl")]
    #[serde(with = "humantime_serde")]
    pub session_ttl: Duration,
    #[serde(default)]
    pub multi_user_legacy_token_enabled: bool,
    #[serde(default)]
    pub oidc: OidcAuthConfig,
    #[serde(default)]
    pub group_mappings: AuthGroupMappingsConfig,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            mode: AuthMode::SingleUser,
            public_base_url: None,
            session_ttl: default_auth_session_ttl(),
            multi_user_legacy_token_enabled: false,
            oidc: OidcAuthConfig::default(),
            group_mappings: AuthGroupMappingsConfig::default(),
        }
    }
}

pub fn default_auth_session_ttl() -> Duration {
    Duration::from_secs(12 * 60 * 60)
}

pub fn default_oidc_client_secret_env() -> String {
    "MEMORY_LAYER_OIDC_CLIENT_SECRET".to_string()
}

pub fn default_oidc_scopes() -> Vec<String> {
    vec![
        "openid".to_string(),
        "profile".to_string(),
        "email".to_string(),
    ]
}

pub fn default_oidc_groups_claim() -> String {
    "groups".to_string()
}
