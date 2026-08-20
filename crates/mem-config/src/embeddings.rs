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

impl EmbeddingsConfig {
    /// Fills in a sensible `name` for every configured embedding backend
    /// that didn't ship one, deduplicating against existing names.
    /// Silently drops the `active` selector if it points at a missing
    /// backend so search falls back to "no embeddings" rather than
    /// crashing the service on startup — doctor/health will still flag it.
    /// If no explicit `active` is set and exactly one backend is
    /// configured, materialize that one into `active` so downstream
    /// code can trust `active` as the persistent source of truth.
    pub fn normalize_backend_names(&mut self) {
        let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
        for backend in &mut self.backends {
            if backend.name.trim().is_empty() {
                backend.name = derive_embedding_backend_name(backend);
            }
            let mut candidate = backend.name.clone();
            let mut suffix = 2;
            while used.contains(&candidate) {
                candidate = format!("{}-{suffix}", backend.name);
                suffix += 1;
            }
            backend.name = candidate.clone();
            used.insert(candidate);
        }
        if let Some(active) = self.active.as_deref()
            && !used.contains(active)
        {
            self.active = None;
        }
        if self.enabled && self.active.is_none() && self.backends.len() == 1 {
            self.active = Some(self.backends[0].name.clone());
        }
    }
}

pub fn derive_embedding_backend_name(backend: &EmbeddingBackendConfig) -> String {
    let provider = backend.provider.trim();
    let model = backend.model.trim();
    let combined = if provider.is_empty() && model.is_empty() {
        "embeddings".to_string()
    } else if provider.is_empty() {
        model.to_string()
    } else if model.is_empty() {
        provider.to_string()
    } else {
        format!("{provider}-{model}")
    };
    combined
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingBackendConfig {
    /// Stable identifier used by CLI/API activation and user-visible
    /// listings. When omitted in config, the loader auto-derives one
    /// from `{provider}-{model}` so users can start without naming.
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_embeddings_provider")]
    pub provider: String,
    #[serde(default = "default_embeddings_base_url")]
    pub base_url: String,
    #[serde(default = "default_embeddings_api_key_env")]
    pub api_key_env: String,
    #[serde(default)]
    pub model: String,
    #[serde(default = "default_embeddings_batch_size")]
    pub batch_size: usize,
    #[serde(default, alias = "dimension", skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<u32>,
    #[serde(default = "default_true")]
    pub create_enabled: bool,
}

impl Default for EmbeddingBackendConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            provider: default_embeddings_provider(),
            base_url: default_embeddings_base_url(),
            api_key_env: default_embeddings_api_key_env(),
            model: String::new(),
            batch_size: default_embeddings_batch_size(),
            dimensions: None,
            create_enabled: true,
        }
    }
}

/// Wraps one or more configured embedding backends plus the name of the
/// one currently used for search. Accepts two TOML shapes:
///
/// 1. Legacy singleton — a flat `[embeddings]` table with `provider`,
///    `model`, etc. Loads as a single backend named after
///    `{provider}-{model}` and marked active.
/// 2. Multi-backend — `[embeddings] active = "<name>"` plus one or
///    more `[[embeddings.backends]]` array-of-tables.
#[derive(Debug, Clone, Serialize)]
pub struct EmbeddingsConfig {
    pub enabled: bool,
    pub create_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
    #[serde(default)]
    pub backends: Vec<EmbeddingBackendConfig>,
}

impl Default for EmbeddingsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            create_enabled: true,
            active: None,
            backends: Vec::new(),
        }
    }
}

impl EmbeddingsConfig {
    /// The currently-active backend according to `active`, falling back
    /// to the sole backend when exactly one is configured and no
    /// explicit `active` was given. Returns `None` when no backends are
    /// configured at all.
    pub fn active_backend(&self) -> Option<&EmbeddingBackendConfig> {
        if !self.enabled {
            return None;
        }
        if let Some(name) = self.active.as_deref() {
            return self.backends.iter().find(|b| b.name == name);
        }
        if self.backends.len() == 1 {
            return self.backends.first();
        }
        None
    }

    /// Lookup a backend by name.
    pub fn backend(&self, name: &str) -> Option<&EmbeddingBackendConfig> {
        self.backends.iter().find(|b| b.name == name)
    }
}

impl<'de> Deserialize<'de> for EmbeddingsConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        let value = serde_json::Value::deserialize(deserializer)?;
        let map = match value {
            serde_json::Value::Null => return Ok(Self::default()),
            serde_json::Value::Object(map) => map,
            other => {
                return Err(D::Error::custom(format!(
                    "expected [embeddings] to be a table, got {other}"
                )));
            }
        };

        // New-form: presence of `backends` array wins.
        if map.contains_key("backends") {
            let enabled = match map.get("enabled") {
                Some(serde_json::Value::Bool(value)) => *value,
                Some(serde_json::Value::Null) | None => true,
                Some(other) => {
                    return Err(D::Error::custom(format!(
                        "embeddings.enabled must be a boolean, got {other}"
                    )));
                }
            };
            let create_enabled = match map.get("create_enabled") {
                Some(serde_json::Value::Bool(value)) => *value,
                Some(serde_json::Value::Null) | None => true,
                Some(other) => {
                    return Err(D::Error::custom(format!(
                        "embeddings.create_enabled must be a boolean, got {other}"
                    )));
                }
            };
            let active = match map.get("active") {
                Some(serde_json::Value::String(s)) => Some(s.clone()),
                Some(serde_json::Value::Null) | None => None,
                Some(other) => {
                    return Err(D::Error::custom(format!(
                        "embeddings.active must be a string, got {other}"
                    )));
                }
            };
            let backends_value = map.get("backends").cloned().unwrap_or_default();
            let backends: Vec<EmbeddingBackendConfig> =
                serde_json::from_value(backends_value).map_err(D::Error::custom)?;
            return Ok(Self {
                enabled,
                create_enabled,
                active,
                backends,
            });
        }

        let enabled = match map.get("enabled") {
            Some(serde_json::Value::Bool(value)) => *value,
            Some(serde_json::Value::Null) | None => true,
            Some(other) => {
                return Err(D::Error::custom(format!(
                    "embeddings.enabled must be a boolean, got {other}"
                )));
            }
        };
        let create_enabled = match map.get("create_enabled") {
            Some(serde_json::Value::Bool(value)) => *value,
            Some(serde_json::Value::Null) | None => true,
            Some(other) => {
                return Err(D::Error::custom(format!(
                    "embeddings.create_enabled must be a boolean, got {other}"
                )));
            }
        };

        // Legacy singleton: if nothing relevant is set, return a wholly
        // empty config so other code paths still see "no backends".
        let is_empty = !map.contains_key("provider")
            && !map.contains_key("model")
            && !map.contains_key("base_url")
            && !map.contains_key("api_key_env")
            && !map.contains_key("batch_size")
            && !map.contains_key("dimensions")
            && !map.contains_key("dimension")
            && !map.contains_key("name")
            && !map.contains_key("enabled")
            && !map.contains_key("create_enabled");
        if is_empty {
            return Ok(Self::default());
        }

        if !map.contains_key("provider")
            && !map.contains_key("model")
            && !map.contains_key("base_url")
            && !map.contains_key("api_key_env")
            && !map.contains_key("batch_size")
            && !map.contains_key("dimensions")
            && !map.contains_key("dimension")
            && !map.contains_key("name")
        {
            return Ok(Self {
                enabled,
                create_enabled,
                active: None,
                backends: Vec::new(),
            });
        }

        let backend: EmbeddingBackendConfig =
            serde_json::from_value(serde_json::Value::Object(map)).map_err(D::Error::custom)?;
        Ok(Self {
            enabled,
            create_enabled,
            active: if backend.name.is_empty() {
                None
            } else {
                Some(backend.name.clone())
            },
            backends: vec![backend],
        })
    }
}

pub fn default_embeddings_provider() -> String {
    "openai".to_string()
}

pub fn default_embeddings_base_url() -> String {
    "https://api.openai.com/v1".to_string()
}

pub fn default_embeddings_api_key_env() -> String {
    "OPENAI_API_KEY".to_string()
}

pub fn default_embeddings_batch_size() -> usize {
    16
}
