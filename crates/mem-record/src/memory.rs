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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemoryType {
    Architecture,
    Convention,
    Decision,
    Incident,
    Debugging,
    Environment,
    DomainFact,
    Documentation,
    Task,
    Plan,
    Implementation,
    Refactor,
    User,
    Feedback,
    Project,
    Reference,
    /// A consolidated meta-memory synthesized from a cluster of related
    /// memories: the schema/gist plus its tensions, gaps, and implications.
    Insight,
}

impl MemoryType {
    /// Every memory type, in canonical order. The exhaustive `match` forces
    /// this list (and the parity test that consumes it) to be updated whenever
    /// a variant is added, so the code and the documented type table cannot
    /// silently drift.
    pub const ALL: [MemoryType; 17] = {
        // Exhaustiveness guard: adding a variant makes this match fail to
        // compile until `ALL` is updated.
        const fn _assert_exhaustive(value: MemoryType) {
            match value {
                MemoryType::Architecture
                | MemoryType::Convention
                | MemoryType::Decision
                | MemoryType::Incident
                | MemoryType::Debugging
                | MemoryType::Environment
                | MemoryType::DomainFact
                | MemoryType::Documentation
                | MemoryType::Task
                | MemoryType::Plan
                | MemoryType::Implementation
                | MemoryType::Refactor
                | MemoryType::User
                | MemoryType::Feedback
                | MemoryType::Project
                | MemoryType::Reference
                | MemoryType::Insight => {}
            }
        }
        [
            MemoryType::Architecture,
            MemoryType::Convention,
            MemoryType::Decision,
            MemoryType::Incident,
            MemoryType::Debugging,
            MemoryType::Environment,
            MemoryType::DomainFact,
            MemoryType::Documentation,
            MemoryType::Task,
            MemoryType::Plan,
            MemoryType::Implementation,
            MemoryType::Refactor,
            MemoryType::User,
            MemoryType::Feedback,
            MemoryType::Project,
            MemoryType::Reference,
            MemoryType::Insight,
        ]
    };
}

impl fmt::Display for MemoryType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Architecture => "architecture",
            Self::Convention => "convention",
            Self::Decision => "decision",
            Self::Incident => "incident",
            Self::Debugging => "debugging",
            Self::Environment => "environment",
            Self::DomainFact => "domain_fact",
            Self::Documentation => "documentation",
            Self::Task => "task",
            Self::Plan => "plan",
            Self::Implementation => "implementation",
            Self::Refactor => "refactor",
            Self::User => "user",
            Self::Feedback => "feedback",
            Self::Project => "project",
            Self::Reference => "reference",
            Self::Insight => "insight",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReplacementPolicy {
    Conservative,
    #[default]
    Balanced,
    Aggressive,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplacementPolicyRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<String>,
    pub replacement_policy: ReplacementPolicy,
}

impl ReplacementPolicyRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self
            .repo_root
            .as_ref()
            .is_none_or(|value| value.trim().is_empty())
        {
            return Err(ValidationError::new("repo_root must be non-empty"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplacementPolicyResponse {
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<String>,
    pub replacement_policy: ReplacementPolicy,
    pub writable: bool,
}

impl fmt::Display for ReplacementPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Conservative => "conservative",
            Self::Balanced => "balanced",
            Self::Aggressive => "aggressive",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ValidationProofScope {
    #[default]
    SourceFilesFirst,
    WholeRepoScan,
    HybridFallback,
}

impl fmt::Display for ValidationProofScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::SourceFilesFirst => "source_files_first",
            Self::WholeRepoScan => "whole_repo_scan",
            Self::HybridFallback => "hybrid_fallback",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemoryRelationType {
    Duplicates,
    Supersedes,
    Supports,
    RelatedTo,
    DependsOn,
    /// A meta-memory (insight) that summarizes the target member memory.
    Summarizes,
}

impl fmt::Display for MemoryRelationType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Duplicates => "duplicates",
            Self::Supersedes => "supersedes",
            Self::Supports => "supports",
            Self::RelatedTo => "related_to",
            Self::DependsOn => "depends_on",
            Self::Summarizes => "summarizes",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryStatus {
    Active,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    TaskPrompt,
    File,
    GitCommit,
    CommandOutput,
    Test,
    Note,
    /// Provenance pointing at another memory (e.g. a consolidated insight's
    /// member memories).
    Memory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedMemorySummary {
    pub memory_id: Uuid,
    pub relation_type: MemoryRelationType,
    pub summary: String,
    pub memory_type: MemoryType,
    pub confidence: f32,
}

/// Reinforcement score state for one memory, as exposed by the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryScoreInfo {
    pub canonical_id: Uuid,
    pub memory_id: Uuid,
    pub summary: String,
    pub activation: f64,
    pub access_count: i64,
    pub citation_count: i64,
    pub propagated_count: i64,
    pub volatility: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_access_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validated_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_confidence: Option<f32>,
    pub needs_review: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub needs_review_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_invalidated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryScoresResponse {
    pub project: String,
    pub scores: Vec<MemoryScoreInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySourceRecord {
    pub id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_kind: Option<String>,
    pub source_kind: SourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<SourceProvenanceRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEmbeddingSpace {
    pub provider: String,
    pub model: String,
    pub base_url: String,
    pub chunk_count: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_updated: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntryResponse {
    pub id: Uuid,
    pub project: String,
    pub canonical_text: String,
    pub summary: String,
    pub memory_type: MemoryType,
    pub importance: i32,
    pub confidence: f32,
    pub status: MemoryStatus,
    pub tags: Vec<String>,
    pub sources: Vec<MemorySourceRecord>,
    #[serde(default)]
    pub related_memories: Vec<RelatedMemorySummary>,
    /// Embedding spaces that cover this memory's chunks. Distinct
    /// (provider, model, base_url) tuples with per-space chunk counts
    /// and the most recent embedding update timestamp.
    #[serde(default)]
    pub embedding_spaces: Vec<MemoryEmbeddingSpace>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Stable identifier shared by every version of this logical memory.
    /// Equal to `id` for never-edited memories.
    #[serde(default)]
    pub canonical_id: Uuid,
    /// 1-indexed version counter within `canonical_id`. New edits or
    /// replacements bump this by one; tombstone deletes also bump it.
    #[serde(default = "default_version_no")]
    pub version_no: i32,
    /// True when this row is the "deleted" sentinel. Content fields are
    /// empty on tombstones; clients should treat the canonical memory
    /// as gone unless they explicitly asked for history.
    #[serde(default)]
    pub is_tombstone: bool,
}

pub fn default_version_no() -> i32 {
    1
}

/// Full version chain for a single canonical memory. Resolved from any
/// version's `id`; the response walks back to the canonical_id and returns
/// every row (including tombstones) ordered oldest → newest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryHistoryResponse {
    pub canonical_id: Uuid,
    pub project: String,
    pub versions: Vec<MemoryEntryResponse>,
}
