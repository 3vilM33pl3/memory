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

pub fn project_paths_for_repo(repo_root: &Path) -> Option<mem_platform::ProjectPaths> {
    let slug = project_slug_for_repo(repo_root);
    mem_platform::project_paths(repo_root, &slug)
}

pub fn project_slug_for_repo(repo_root: &Path) -> String {
    read_repo_project_slug(repo_root)
        .or_else(|| {
            repo_root
                .file_name()
                .and_then(|value| value.to_str())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| "project".to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentProjectConfig {
    #[serde(default)]
    pub capture: AgentCaptureConfig,
    #[serde(default)]
    pub analysis: AgentAnalysisConfig,
    #[serde(default)]
    pub retrieval: AgentRetrievalConfig,
    #[serde(default)]
    pub curation: AgentCurationConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentCaptureConfig {
    #[serde(default)]
    pub include_paths: Vec<String>,
    #[serde(default)]
    pub ignore_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAnalysisConfig {
    #[serde(default = "default_agent_analyzers")]
    pub analyzers: Vec<String>,
}

impl Default for AgentAnalysisConfig {
    fn default() -> Self {
        Self {
            analyzers: default_agent_analyzers(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentRetrievalConfig {
    #[serde(default)]
    pub graph_enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentCurationConfig {
    #[serde(default)]
    pub replacement_policy: ReplacementPolicy,
}

pub fn default_agent_analyzers() -> Vec<String> {
    vec![
        "rust".to_string(),
        "typescript".to_string(),
        "python".to_string(),
    ]
}

pub fn repo_agent_settings_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".agents").join("memory-layer.toml")
}

pub fn read_repo_project_slug(repo_root: &Path) -> Option<String> {
    read_project_slug_from_file(&repo_root.join(".mem").join("project.toml"))
        .or_else(|| read_project_slug_from_file(&repo_agent_settings_path(repo_root)))
}

pub fn read_project_slug_from_file(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("slug = ") {
            let slug = value.trim().trim_matches('"').trim();
            if !slug.is_empty() {
                return Some(slug.to_string());
            }
        }
    }
    None
}

pub fn load_repo_agent_settings(repo_root: &Path) -> Result<AgentProjectConfig, ConfigError> {
    let path = repo_agent_settings_path(repo_root);
    if !path.exists() {
        return Ok(AgentProjectConfig::default());
    }

    let config = Config::builder()
        .add_source(File::from(path).format(FileFormat::Toml).required(false))
        .build()?;
    config.try_deserialize()
}

pub fn load_repo_replacement_policy(repo_root: &Path) -> Result<ReplacementPolicy, ConfigError> {
    Ok(load_repo_agent_settings(repo_root)?
        .curation
        .replacement_policy)
}
