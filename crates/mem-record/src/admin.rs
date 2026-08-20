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

/// The project's meta-memory structure: currently discovered consolidation
/// groups plus the committed insight tree (`summarizes` relations, recursive
/// across tiers).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectStructureResponse {
    pub project: String,
    /// Clusters the deterministic scan discovers right now (value gate passed,
    /// not yet covered by an insight) — what consolidation would propose next.
    #[serde(default)]
    pub groups: Vec<StructureGroupInfo>,
    /// Community-detection candidates considered in this scan.
    #[serde(default)]
    pub candidate_count: usize,
    /// Candidates rejected by the value gate.
    #[serde(default)]
    pub rejected_count: usize,
    /// Candidates skipped because an existing insight already covers them.
    #[serde(default)]
    pub covered_count: usize,
    /// Roots of the committed insight tree: insights not themselves summarized
    /// by another insight, with their members nested recursively.
    #[serde(default)]
    pub insights: Vec<StructureInsightNode>,
}

/// One currently discovered (uncommitted) consolidation group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructureGroupInfo {
    pub size: usize,
    /// `salient` (use) or `cold_dense` (non-use).
    pub trigger: String,
    pub intra_density: f64,
    pub coaccess_mass: f64,
    pub activation_mass: f64,
    pub members: Vec<StructureMemberInfo>,
}

/// A memory referenced from the structure view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructureMemberInfo {
    pub canonical_id: Uuid,
    pub summary: String,
    pub memory_type: String,
}

/// One node of the insight tree: an insight with everything it `summarizes`.
/// Children that are themselves insights recurse into deeper tiers; leaf
/// children carry an empty `children` list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructureInsightNode {
    pub canonical_id: Uuid,
    pub summary: String,
    pub memory_type: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<StructureInsightNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveRequest {
    pub project: String,
    #[serde(default = "default_archive_threshold")]
    pub max_confidence: f32,
    #[serde(default = "default_archive_importance")]
    pub max_importance: i32,
    #[serde(default)]
    pub dry_run: bool,
}

impl ArchiveRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.project.trim().is_empty() {
            return Err(ValidationError::new("project must be non-empty"));
        }
        Ok(())
    }
}

pub fn default_archive_threshold() -> f32 {
    0.3
}

pub fn default_archive_importance() -> i32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveResponse {
    pub archived_count: u64,
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveMemoryResponse {
    pub memory_id: Uuid,
    pub project: String,
    pub summary: String,
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteMemoryRequest {
    pub memory_id: Uuid,
}

impl DeleteMemoryRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.memory_id.is_nil() {
            return Err(ValidationError::new("memory_id must be non-nil"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteMemoryResponse {
    pub memory_id: Uuid,
    pub project: String,
    pub summary: String,
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReindexRequest {
    pub project: String,
    #[serde(default)]
    pub dry_run: bool,
    /// Compatibility backfill target. `None` performs a full chunk rebuild
    /// and populates every configured backend. `Some(name)` only fills missing
    /// embeddings for that backend, preserving embeddings already stored for
    /// other backend spaces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
}

impl ReindexRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.project.trim().is_empty() {
            return Err(ValidationError::new("project must be non-empty"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReindexResponse {
    pub reindexed_entries: u64,
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReembedRequest {
    pub project: String,
    #[serde(default)]
    pub dry_run: bool,
    /// Restrict to a single configured backend by name. `None` means
    /// every configured backend is reembedded so all spaces stay covered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
}

impl ReembedRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.project.trim().is_empty() {
            return Err(ValidationError::new("project must be non-empty"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReembedResponse {
    pub reembedded_chunks: u64,
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruneEmbeddingsRequest {
    pub project: String,
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingBackendInfo {
    pub name: String,
    pub provider: String,
    pub base_url: String,
    pub model: String,
    pub active: bool,
    /// Whether the backend resolved at service-startup — `false` means
    /// the backend is declared in config but the API key or model is
    /// missing, so it won't embed until fixed.
    pub ready: bool,
    /// Whether automatic curation/import writes should create embeddings
    /// for this backend. Manual reembed/reindex operations ignore this.
    #[serde(default = "default_true")]
    pub create_enabled: bool,
    /// Chunks in the requested project that currently have an
    /// embedding in this backend's space. Present only when the
    /// request scoped the listing to a project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_chunk_count: Option<i64>,
    /// Distinct memories in the requested project with at least one
    /// chunk covered by this backend.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_memory_count: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingBackendsResponse {
    pub backends: Vec<EmbeddingBackendInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
    #[serde(default = "default_true")]
    pub create_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivateEmbeddingBackendRequest {
    pub name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeactivateEmbeddingBackendRequest {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetEmbeddingCreationRequest {
    pub name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmAuditStatusResponse {
    pub enabled: bool,
    pub redacted: bool,
    pub max_message_chars: usize,
    pub max_total_chars: usize,
    pub profile: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetLlmAuditRequest {
    pub enabled: bool,
}

impl ActivateEmbeddingBackendRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.name.trim().is_empty() {
            return Err(ValidationError::new("name must be non-empty"));
        }
        Ok(())
    }
}

impl PruneEmbeddingsRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.project.trim().is_empty() {
            return Err(ValidationError::new("project must be non-empty"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruneEmbeddingsResponse {
    pub pruned_embeddings: u64,
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemoryListItem {
    pub id: Uuid,
    pub summary: String,
    pub preview: String,
    pub memory_type: MemoryType,
    pub status: MemoryStatus,
    pub confidence: f32,
    pub importance: i32,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub tag_count: i64,
    pub source_count: i64,
    /// Stable identifier shared by every version of this memory.
    #[serde(default)]
    pub canonical_id: Uuid,
    /// 1-indexed version number.
    #[serde(default = "default_version_no")]
    pub version_no: i32,
    /// True when this is the deleted sentinel for a canonical memory.
    #[serde(default)]
    pub is_tombstone: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemoriesResponse {
    pub project: String,
    pub total: i64,
    pub items: Vec<ProjectMemoryListItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryTypeCount {
    pub memory_type: MemoryType,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceKindCount {
    pub source_kind: SourceKind,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedCount {
    pub name: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectOverviewResponse {
    pub project: String,
    pub service_status: String,
    pub database_status: String,
    pub memory_entries_total: i64,
    pub active_memories: i64,
    pub archived_memories: i64,
    pub raw_captures_total: i64,
    pub uncurated_raw_captures: i64,
    pub tasks_total: i64,
    pub sessions_total: i64,
    pub curation_runs_total: i64,
    pub recent_memories_7d: i64,
    pub recent_captures_7d: i64,
    pub high_confidence_memories: i64,
    pub medium_confidence_memories: i64,
    pub low_confidence_memories: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_memory_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_capture_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_curation_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oldest_uncurated_capture_age_hours: Option<i64>,
    #[serde(default)]
    pub embedding_chunks_total: i64,
    #[serde(default)]
    pub fresh_embedding_chunks: i64,
    #[serde(default)]
    pub stale_embedding_chunks: i64,
    #[serde(default)]
    pub missing_embedding_chunks: i64,
    #[serde(default)]
    pub embedding_spaces_total: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_embedding_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_embedding_model: Option<String>,
    #[serde(default)]
    pub memory_type_breakdown: Vec<MemoryTypeCount>,
    #[serde(default)]
    pub source_kind_breakdown: Vec<SourceKindCount>,
    #[serde(default)]
    pub top_tags: Vec<NamedCount>,
    #[serde(default)]
    pub top_files: Vec<NamedCount>,
    #[serde(default)]
    pub pending_replacement_proposals: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automation: Option<AutomationStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watchers: Option<WatcherPresenceSummary>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PruneHistoryRequest {
    /// Limit the prune to a single project. None means every project in
    /// the database.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// Overrides `RetentionConfig::tombstone_after` for this call.
    #[serde(default, with = "humantime_serde::option")]
    pub tombstone_after: Option<Duration>,
    /// Overrides `RetentionConfig::superseded_after` for this call.
    #[serde(default, with = "humantime_serde::option")]
    pub superseded_after: Option<Duration>,
    /// When true, count what would be deleted without actually deleting.
    #[serde(default)]
    pub dry_run: bool,
}

impl PruneHistoryRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.tombstone_after.is_none() && self.superseded_after.is_none() {
            return Err(ValidationError::new(
                "no retention threshold configured; pass --tombstone-after or --superseded-after, \
                 or set retention.tombstone_after / retention.superseded_after in config",
            ));
        }
        if let Some(project) = &self.project
            && project.trim().is_empty()
        {
            return Err(ValidationError::new(
                "project must be non-empty when provided",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PruneHistoryResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    pub canonicals_tombstoned_deleted: u64,
    pub superseded_versions_pruned: u64,
    pub dry_run: bool,
}
