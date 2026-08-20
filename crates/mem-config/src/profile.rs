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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Prod,
    Dev,
}

impl Profile {
    /// Resolve the active profile from (1) `MEMORY_LAYER_PROFILE` env var,
    /// otherwise (2) the location of the running binary — a path under a
    /// `target/{debug,release}/` directory counts as dev.
    pub fn detect() -> Self {
        if let Ok(value) = env::var("MEMORY_LAYER_PROFILE") {
            match value.trim().to_ascii_lowercase().as_str() {
                "dev" | "development" => return Profile::Dev,
                "prod" | "production" | "" => return Profile::Prod,
                _ => {}
            }
        }
        if current_exe_is_in_cargo_target() {
            return Profile::Dev;
        }
        Profile::Prod
    }

    /// Version-string suffix for this profile. Dev builds get `-dev` so
    /// `memory --version`, the health endpoint, and cluster discovery all
    /// make it obvious which stack produced a given response.
    pub fn version_suffix(self) -> &'static str {
        match self {
            Profile::Prod => "",
            Profile::Dev => "-dev",
        }
    }

    /// Apply [`Profile::version_suffix`] to a crate's `CARGO_PKG_VERSION`.
    pub fn display_version(self, pkg_version: &str) -> String {
        format!("{pkg_version}{}", self.version_suffix())
    }
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Profile::Prod => "prod",
            Profile::Dev => "dev",
        })
    }
}

pub fn current_exe_is_in_cargo_target() -> bool {
    let Ok(exe) = env::current_exe() else {
        return false;
    };
    let mut saw_profile_dir = false;
    for ancestor in exe.ancestors() {
        let Some(name) = ancestor.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !saw_profile_dir {
            if matches!(name, "debug" | "release") {
                saw_profile_dir = true;
            }
            continue;
        }
        if name == "target" {
            if let Some(parent) = ancestor.parent() {
                return parent.join("Cargo.toml").is_file();
            }
            return false;
        }
    }
    false
}

pub fn default_profile() -> Profile {
    Profile::Prod
}
