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

/// Strictly opt-in usage telemetry. Nothing is ever sent unless BOTH
/// `enabled = true` AND an `endpoint` are configured: there is no default
/// collector. Events are counts only (event name, version, OS, an anonymous
/// random instance id) — never project names, queries, or memory content.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TelemetryConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub service: ServiceConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub mcp: McpConfig,
    pub database: DatabaseConfig,
    #[serde(default)]
    pub offline: OfflineConfig,
    #[serde(default)]
    pub features: FeatureFlags,
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub llm_audit: LlmAuditConfig,
    #[serde(default)]
    pub embeddings: EmbeddingsConfig,
    #[serde(default)]
    pub cluster: ClusterConfig,
    #[serde(default, alias = "agent")]
    pub writer: WriterConfig,
    #[serde(default)]
    pub automation: AutomationConfig,
    #[serde(default)]
    pub retention: RetentionConfig,
    #[serde(default)]
    pub provenance: ProvenanceConfig,
    #[serde(default)]
    pub reinforcement: ReinforcementConfig,
    #[serde(default)]
    pub curation: CurationConfig,
    #[serde(default)]
    pub consolidation: ConsolidationConfig,
    #[serde(default)]
    pub procedural: ProceduralConfig,
    #[serde(default)]
    pub telemetry: TelemetryConfig,
    #[serde(skip, default = "default_profile")]
    pub profile: Profile,
    /// Path of the resolved config file (base file in dev mode). Useful when
    /// spawning subprocesses that must reuse the same config.
    #[serde(skip)]
    pub resolved_config_path: Option<PathBuf>,
    /// Path of the dev overlay if one was applied.
    #[serde(skip)]
    pub resolved_dev_overlay_path: Option<PathBuf>,
}

impl AppConfig {
    pub fn load_from_path(path: Option<PathBuf>) -> Result<Self, ConfigError> {
        Self::load_with_profile(path, Profile::detect())
    }

    pub fn load_with_profile(path: Option<PathBuf>, profile: Profile) -> Result<Self, ConfigError> {
        let mut builder = Config::builder();
        let mut env_files = Vec::new();
        let mut resolved_config_path: Option<PathBuf> = None;
        let mut resolved_dev_overlay_path: Option<PathBuf> = None;

        if let Some(path) = path {
            env_files.push(env_path_for_config(&path));
            resolved_config_path = Some(path.clone());
            builder = builder.add_source(File::from(path).required(false));
        } else {
            // Global config is part of the installed/prod stack; the dev
            // stack ignores it so a cargo-run service cannot silently pick
            // up the packaged machine-wide settings. Bootstrap shared
            // values (database URL, LLM endpoints) into the user-local project
            // config.dev.toml via `memory dev init --copy-from-global`.
            if profile == Profile::Prod {
                if let Some(path) = discover_global_config_path() {
                    env_files.push(env_path_for_config(&path));
                    builder = builder.add_source(File::from(path).required(false));
                } else {
                    builder = builder.add_source(File::with_name("memory-layer").required(false));
                }
            }
            if let Some(path) = discover_repo_config_path() {
                env_files.push(
                    path.parent()
                        .unwrap_or_else(|| Path::new("."))
                        .join("memory-layer.env"),
                );
                resolved_config_path = Some(path.clone());
                builder = builder.add_source(File::from(path).required(false));
            }
        }

        if profile == Profile::Dev {
            let overlay_path = resolved_config_path
                .as_deref()
                .and_then(dev_overlay_path_for_base)
                .or_else(discover_repo_dev_config_path);
            match overlay_path {
                Some(path) if path.is_file() => {
                    resolved_dev_overlay_path = Some(path.clone());
                    builder = builder.add_source(File::from(path).required(false));
                }
                _ => {
                    return Err(ConfigError::Message(dev_overlay_missing_message(
                        resolved_config_path.as_deref(),
                    )));
                }
            }
        }

        for env_file in env_files {
            if let Some(source) = env_file_source(&env_file)? {
                builder = builder.add_source(source);
            }
        }

        let config = builder
            .add_source(Environment::with_prefix("MEMORY_LAYER").separator("__"))
            .build()?;
        let mut value: serde_json::Value = config.try_deserialize()?;
        normalize_legacy_config_keys(&mut value);
        let mut config: AppConfig =
            serde_json::from_value(value).map_err(|error| ConfigError::Foreign(Box::new(error)))?;
        config.profile = profile;
        config.resolved_config_path = resolved_config_path;
        config.resolved_dev_overlay_path = resolved_dev_overlay_path;
        config.apply_runtime_defaults();
        Ok(config)
    }

    fn apply_runtime_defaults(&mut self) {
        self.embeddings.normalize_backend_names();
        if self.cluster.service_id.trim().is_empty() {
            self.cluster.service_id = format!(
                "service-{}",
                self.service
                    .bind_addr
                    .chars()
                    .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
                    .collect::<String>()
                    .trim_matches('-')
            );
        }
    }

    /// Resolves a named runtime credential without copying secret values into
    /// the serializable configuration. Process environment values take
    /// precedence over the env file adjacent to the active config.
    pub fn credential_env_value(&self, key: &str) -> Option<String> {
        if let Ok(value) = env::var(key)
            && !value.trim().is_empty()
        {
            return Some(value);
        }

        let env_path = self
            .resolved_dev_overlay_path
            .as_deref()
            .or(self.resolved_config_path.as_deref())
            .map(env_path_for_config)?;
        exact_env_file_value(&env_path, key)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceConfig {
    #[serde(default = "default_bind_addr")]
    pub bind_addr: String,
    #[serde(default)]
    pub web_root: Option<String>,
    #[serde(default = "default_api_token")]
    pub api_token: String,
    #[serde(default = "default_request_timeout")]
    #[serde(with = "humantime_serde")]
    pub request_timeout: Duration,
    /// Student / kiosk mode: reject every mutating HTTP endpoint while
    /// keeping reads, queries, and briefings available. Activation still
    /// evolves with use (reinforcement is internal); only content writes are
    /// blocked. Env override: `MEMORY_LAYER__SERVICE__READ_ONLY=true`.
    #[serde(default, deserialize_with = "deserialize_env_bool")]
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub http_enabled: bool,
    #[serde(default = "default_mcp_http_path")]
    pub http_path: String,
    #[serde(default = "default_true")]
    pub require_token: bool,
    #[serde(default = "default_true")]
    pub read_only: bool,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            http_enabled: true,
            http_path: default_mcp_http_path(),
            require_token: true,
            read_only: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterConfig {
    #[serde(default = "default_cluster_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub service_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advertise_addr: Option<String>,
    #[serde(default = "default_cluster_discovery_multicast_addr")]
    pub discovery_multicast_addr: String,
    #[serde(default = "default_cluster_announce_interval")]
    #[serde(with = "humantime_serde")]
    pub announce_interval: Duration,
    #[serde(default = "default_cluster_peer_ttl")]
    #[serde(with = "humantime_serde")]
    pub peer_ttl: Duration,
    #[serde(default = "default_cluster_priority")]
    pub priority: i32,
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            enabled: default_cluster_enabled(),
            service_id: String::new(),
            advertise_addr: None,
            discovery_multicast_addr: default_cluster_discovery_multicast_addr(),
            announce_interval: default_cluster_announce_interval(),
            peer_ttl: default_cluster_peer_ttl(),
            priority: default_cluster_priority(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WriterConfig {
    #[serde(default)]
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfflineConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    #[serde(default = "default_offline_reconnect_interval")]
    #[serde(with = "humantime_serde")]
    pub reconnect_interval: Duration,
    #[serde(default = "default_offline_sync_batch_size")]
    pub sync_batch_size: usize,
}

impl Default for OfflineConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            path: None,
            reconnect_interval: default_offline_reconnect_interval(),
            sync_batch_size: default_offline_sync_batch_size(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FeatureFlags {
    #[serde(default)]
    pub llm_curation: bool,
}

pub fn default_bind_addr() -> String {
    "127.0.0.1:4040".to_string()
}

pub fn default_cluster_enabled() -> bool {
    true
}

pub fn default_cluster_discovery_multicast_addr() -> String {
    "239.255.42.99:4042".to_string()
}

pub fn default_cluster_announce_interval() -> Duration {
    Duration::from_secs(5)
}

pub fn default_cluster_peer_ttl() -> Duration {
    Duration::from_secs(15)
}

pub fn default_cluster_priority() -> i32 {
    100
}

pub fn default_offline_reconnect_interval() -> Duration {
    Duration::from_secs(15)
}

pub fn default_offline_sync_batch_size() -> usize {
    50
}

pub fn default_api_token() -> String {
    "dev-memory-token".to_string()
}

pub fn default_mcp_http_path() -> String {
    "/mcp".to_string()
}

pub fn default_request_timeout() -> Duration {
    Duration::from_secs(30)
}
