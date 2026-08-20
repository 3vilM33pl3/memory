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
pub struct LlmConfig {
    #[serde(default = "default_llm_provider")]
    pub provider: String,
    #[serde(default = "default_llm_base_url")]
    pub base_url: String,
    #[serde(default = "default_llm_api_key_env")]
    pub api_key_env: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub temperature: f32,
    #[serde(default = "default_llm_max_input_bytes")]
    pub max_input_bytes: usize,
    #[serde(default = "default_llm_max_output_tokens")]
    pub max_output_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmAuditConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub redact: bool,
    #[serde(default = "default_llm_audit_max_message_chars")]
    pub max_message_chars: usize,
    #[serde(default = "default_llm_audit_max_total_chars")]
    pub max_total_chars: usize,
}

impl Default for LlmAuditConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            redact: true,
            max_message_chars: default_llm_audit_max_message_chars(),
            max_total_chars: default_llm_audit_max_total_chars(),
        }
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: default_llm_provider(),
            base_url: default_llm_base_url(),
            api_key_env: default_llm_api_key_env(),
            model: String::new(),
            temperature: 0.0,
            max_input_bytes: default_llm_max_input_bytes(),
            max_output_tokens: default_llm_max_output_tokens(),
        }
    }
}

pub const OPENAI_COMPATIBLE_PROVIDER: &str = "openai_compatible";

pub const OLLAMA_PROVIDER: &str = "ollama";

pub const OPENAI_COMPATIBLE_BASE_URL: &str = "https://api.openai.com/v1";

pub const OLLAMA_BASE_URL: &str = "http://127.0.0.1:11434/v1";

pub fn is_ollama_provider(provider: &str) -> bool {
    provider.trim().eq_ignore_ascii_case(OLLAMA_PROVIDER)
}

pub fn is_supported_llm_provider(provider: &str) -> bool {
    matches!(
        provider.trim(),
        OPENAI_COMPATIBLE_PROVIDER | OLLAMA_PROVIDER
    )
}

pub fn effective_llm_base_url(config: &LlmConfig) -> String {
    effective_llm_base_url_for(&config.provider, &config.base_url)
}

pub fn effective_llm_base_url_for(provider: &str, configured: &str) -> String {
    let configured = configured.trim().trim_end_matches('/');
    if is_ollama_provider(provider)
        && (configured.is_empty() || configured == OPENAI_COMPATIBLE_BASE_URL)
    {
        OLLAMA_BASE_URL.to_string()
    } else if configured.is_empty() {
        OPENAI_COMPATIBLE_BASE_URL.to_string()
    } else {
        configured.to_string()
    }
}

pub fn llm_max_output_tokens_field(provider: &str) -> &'static str {
    if is_ollama_provider(provider) {
        "max_tokens"
    } else {
        "max_completion_tokens"
    }
}

pub fn resolve_llm_api_key(config: &LlmConfig) -> Option<String> {
    let key = config.llm_api_key_env_for_resolution()?;
    resolve_secret_value(key).filter(|value| !value.trim().is_empty())
}

pub fn llm_requires_api_key(config: &LlmConfig) -> bool {
    if is_ollama_provider(&config.provider) {
        let key = config.api_key_env.trim();
        !key.is_empty() && key != default_llm_api_key_env()
    } else {
        true
    }
}

impl LlmConfig {
    fn llm_api_key_env_for_resolution(&self) -> Option<&str> {
        let key = self.api_key_env.trim();
        if key.is_empty() {
            return None;
        }
        if is_ollama_provider(&self.provider) && key == default_llm_api_key_env() {
            return None;
        }
        Some(key)
    }
}

pub fn default_llm_provider() -> String {
    "openai_compatible".to_string()
}

pub fn default_llm_base_url() -> String {
    "https://api.openai.com/v1".to_string()
}

pub fn default_llm_api_key_env() -> String {
    "OPENAI_API_KEY".to_string()
}

pub fn default_llm_max_input_bytes() -> usize {
    120_000
}

pub fn default_llm_max_output_tokens() -> u32 {
    3_000
}

pub fn default_llm_audit_max_message_chars() -> usize {
    8_000
}

pub fn default_llm_audit_max_total_chars() -> usize {
    32_000
}
