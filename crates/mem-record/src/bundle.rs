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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemoryExportOptions {
    #[serde(default)]
    pub include_archived: bool,
    #[serde(default = "default_true")]
    pub include_tags: bool,
    #[serde(default = "default_true")]
    pub include_relations: bool,
    #[serde(default)]
    pub include_source_file_paths: bool,
    #[serde(default)]
    pub include_git_commits: bool,
    #[serde(default)]
    pub include_source_excerpts: bool,
}

impl Default for ProjectMemoryExportOptions {
    fn default() -> Self {
        Self {
            include_archived: false,
            include_tags: true,
            include_relations: true,
            include_source_file_paths: false,
            include_git_commits: false,
            include_source_excerpts: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemoryBundlePreview {
    pub bundle_id: String,
    pub source_project: String,
    pub exported_at: DateTime<Utc>,
    pub summary_markdown: String,
    pub memory_count: usize,
    pub relation_count: usize,
    pub warning_count: usize,
    #[serde(default)]
    pub warnings: Vec<String>,
    pub options: ProjectMemoryExportOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemoryImportPreview {
    pub bundle_id: String,
    pub bundle_hash: String,
    pub source_project: String,
    pub target_project: String,
    pub exported_at: DateTime<Utc>,
    pub summary_markdown: String,
    pub memory_count: usize,
    pub relation_count: usize,
    pub new_count: usize,
    pub unchanged_count: usize,
    pub replacing_count: usize,
    pub warning_count: usize,
    #[serde(default)]
    pub warnings: Vec<String>,
    pub options: ProjectMemoryExportOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemoryImportResponse {
    pub target_project: String,
    pub bundle_id: String,
    pub bundle_hash: String,
    pub imported_count: usize,
    pub replaced_count: usize,
    pub skipped_count: usize,
    pub relation_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemoryBundleEntryRelation {
    pub relation_type: MemoryRelationType,
    pub target_entry_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemoryBundleSource {
    pub source_kind: SourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_start: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_end: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemoryBundleEntry {
    pub entry_key: String,
    /// Stable identity in the source instance (v2 bundles).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_no: Option<i32>,
    pub canonical_text: String,
    pub summary: String,
    pub memory_type: MemoryType,
    pub importance: i32,
    pub confidence: f32,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub relations: Vec<ProjectMemoryBundleEntryRelation>,
    #[serde(default)]
    pub sources: Vec<ProjectMemoryBundleSource>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemoryBundleManifest {
    pub schema_version: u32,
    pub bundle_id: String,
    pub source_project: String,
    pub exported_at: DateTime<Utc>,
    pub summary_markdown: String,
    pub bundle_hash: String,
    pub options: ProjectMemoryExportOptions,
    #[serde(default)]
    pub entries: Vec<ProjectMemoryBundleEntry>,
}

pub fn default_true() -> bool {
    true
}
