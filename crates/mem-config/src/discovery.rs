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

pub fn normalize_legacy_config_keys(value: &mut serde_json::Value) {
    let Some(root) = value.as_object_mut() else {
        return;
    };
    let Some(automation) = root
        .get_mut("automation")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return;
    };

    if automation.contains_key("capture_idle_threshold") {
        automation.remove("idle_threshold");
    } else if let Some(legacy) = automation.remove("idle_threshold") {
        automation.insert("capture_idle_threshold".to_string(), legacy);
    }
}

pub fn env_file_source(
    path: &Path,
) -> Result<Option<config::File<config::FileSourceString, FileFormat>>, ConfigError> {
    let values = memory_layer_env_file_values(path)?;
    if values.is_empty() {
        return Ok(None);
    }

    let mut lines = values
        .into_iter()
        .map(|(key, value)| {
            let quoted = serde_json::to_string(&value)
                .expect("serializing an owned string to JSON cannot fail");
            format!("{key} = {quoted}")
        })
        .collect::<Vec<_>>();
    lines.sort();
    Ok(Some(File::from_str(&lines.join("\n"), FileFormat::Toml)))
}

pub fn memory_layer_env_file_values(path: &Path) -> Result<HashMap<String, String>, ConfigError> {
    let mut values = HashMap::new();
    let Ok(content) = std::fs::read_to_string(path) else {
        return Ok(values);
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((name, value)) = trimmed.split_once('=') else {
            continue;
        };
        let key = name.trim();
        if !key.starts_with("MEMORY_LAYER__") {
            continue;
        }
        let config_key = key["MEMORY_LAYER__".len()..]
            .split("__")
            .map(|segment| segment.trim().to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join(".");
        values.insert(config_key, value.trim().to_string());
    }

    Ok(values)
}

pub fn exact_env_file_value(path: &Path, key: &str) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    content.lines().find_map(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return None;
        }
        let (name, value) = trimmed.split_once('=')?;
        if name.trim() != key || value.trim().is_empty() {
            return None;
        }
        Some(value.trim().to_string())
    })
}

pub fn env_path_for_config(path: &Path) -> PathBuf {
    path.parent()
        .unwrap_or_else(|| Path::new("."))
        .join("memory-layer.env")
}

pub fn discover_repo_config_path() -> Option<PathBuf> {
    let cwd = env::current_dir().ok()?;
    find_repo_config_path(&cwd)
}

pub fn dev_overlay_path_for_base(base: &Path) -> Option<PathBuf> {
    base.parent().map(|parent| parent.join("config.dev.toml"))
}

pub fn discover_repo_dev_config_path() -> Option<PathBuf> {
    let cwd = env::current_dir().ok()?;
    if let Some(repo_root) = mem_platform::discover_project_root(&cwd)
        && let Some(paths) = project_paths_for_repo(&repo_root)
        && paths.dev_config_path().is_file()
    {
        return Some(paths.dev_config_path());
    }
    for directory in cwd.ancestors() {
        let candidate = directory.join(".mem").join("config.dev.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub fn dev_overlay_missing_message(base: Option<&Path>) -> String {
    let hint = match base.and_then(Path::parent) {
        Some(dir) => format!("{}/config.dev.toml", dir.display()),
        None => "<project config>/config.dev.toml".to_string(),
    };
    format!(
        "dev profile active but {hint} is missing. Run `memory dev init` to \
         scaffold it, or set MEMORY_LAYER_PROFILE=prod to opt out."
    )
}

pub fn discover_repo_env_path() -> Option<PathBuf> {
    let config_path = discover_repo_config_path()?;
    Some(env_path_for_config(&config_path))
}

pub fn discover_global_env_path() -> Option<PathBuf> {
    let config_path = discover_global_config_path()?;
    Some(env_path_for_config(&config_path))
}

pub fn discover_global_config_path() -> Option<PathBuf> {
    discover_existing_global_config_path()
}

pub fn find_repo_config_path(start: &Path) -> Option<PathBuf> {
    if let Some(repo_root) = mem_platform::discover_project_root(start)
        && let Some(paths) = project_paths_for_repo(&repo_root)
        && paths.config_path().is_file()
    {
        return Some(paths.config_path());
    }
    for directory in start.ancestors() {
        let candidate = directory.join(".mem").join("config.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub fn resolve_secret_value(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .or_else(|| discover_repo_env_path().and_then(|path| env_lookup(&path, key)))
        .or_else(|| discover_global_env_path().and_then(|path| env_lookup(&path, key)))
}

pub fn env_lookup(path: &Path, key: &str) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((name, value)) = trimmed.split_once('=')
            && name.trim() == key
        {
            return Some(value.trim().to_string());
        }
    }
    None
}

/// Bool that also accepts the string forms the env config source produces
/// (`MEMORY_LAYER__…=true` arrives as the string "true", not a boolean).
pub fn deserialize_env_bool<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum FlexibleBool {
        Bool(bool),
        Int(i64),
        Text(String),
    }
    match FlexibleBool::deserialize(deserializer)? {
        FlexibleBool::Bool(value) => Ok(value),
        FlexibleBool::Int(value) => Ok(value != 0),
        FlexibleBool::Text(value) => match value.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "on" => Ok(true),
            "false" | "0" | "no" | "off" | "" => Ok(false),
            other => Err(serde::de::Error::custom(format!(
                "invalid boolean value: {other:?}"
            ))),
        },
    }
}
