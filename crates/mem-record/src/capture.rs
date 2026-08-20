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
pub struct TestResult {
    pub command: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureCandidateSourceInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_kind: Option<String>,
    pub source_kind: SourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureCandidateInput {
    pub canonical_text: String,
    pub summary: String,
    pub memory_type: MemoryType,
    #[serde(default = "default_candidate_confidence")]
    pub confidence: f32,
    #[serde(default = "default_candidate_importance")]
    pub importance: i32,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub sources: Vec<CaptureCandidateSourceInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureTaskRequest {
    pub project: String,
    pub task_title: String,
    pub user_prompt: String,
    #[serde(alias = "agent_id")]
    pub writer_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "agent_name")]
    pub writer_name: Option<String>,
    pub agent_summary: String,
    #[serde(default)]
    pub files_changed: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_diff_summary: Option<String>,
    #[serde(default)]
    pub tests: Vec<TestResult>,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub structured_candidates: Vec<CaptureCandidateInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    #[serde(default)]
    pub dry_run: bool,
}

impl CaptureTaskRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.project.trim().is_empty() {
            return Err(ValidationError::new("project must be non-empty"));
        }
        if self.task_title.trim().is_empty() {
            return Err(ValidationError::new("task_title must be non-empty"));
        }
        if self.user_prompt.trim().is_empty() {
            return Err(ValidationError::new("user_prompt must be non-empty"));
        }
        if self.writer_id.trim().is_empty() {
            return Err(ValidationError::new("writer_id must be non-empty"));
        }
        if self.agent_summary.trim().is_empty() {
            return Err(ValidationError::new("agent_summary must be non-empty"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurateRequest {
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_size: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_capture_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement_policy: Option<ReplacementPolicy>,
    #[serde(default)]
    pub dry_run: bool,
}

impl CurateRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.project.trim().is_empty() {
            return Err(ValidationError::new("project must be non-empty"));
        }
        if let Some(batch_size) = self.batch_size
            && batch_size <= 0
        {
            return Err(ValidationError::new("batch_size must be positive"));
        }
        Ok(())
    }
}

pub fn default_candidate_confidence() -> f32 {
    0.75
}

pub fn default_candidate_importance() -> i32 {
    2
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureTaskResponse {
    pub project_id: Uuid,
    pub session_id: Uuid,
    pub task_id: Uuid,
    pub raw_capture_id: Uuid,
    pub idempotency_key: String,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub queued_offline: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offline_queue_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offline_message: Option<String>,
}

pub fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfflinePendingResponse {
    pub enabled: bool,
    pub database_path: Option<String>,
    pub pending_count: u64,
    pub items: Vec<OfflinePendingItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfflinePendingItem {
    pub queue_id: Uuid,
    pub item_kind: String,
    pub project: String,
    pub summary: Option<String>,
    pub idempotency_key: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub attempt_count: u64,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurateResponse {
    pub project_id: Uuid,
    pub run_id: Uuid,
    pub input_count: i64,
    pub output_count: i64,
    #[serde(default)]
    pub memory_ids: Vec<Uuid>,
    #[serde(default)]
    pub replaced_count: i64,
    #[serde(default)]
    pub proposal_count: i64,
    #[serde(default)]
    pub replacements: Vec<AppliedMemoryReplacement>,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<DiagnosticInfo>,
    /// Memories whose reinforcement activation makes them due for
    /// validation, reported when the reinforcement system is enabled.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub validation_due: Vec<ValidationDueInfo>,
    /// Clusters of related memories that passed the value gate and are not yet
    /// covered by an insight, reported when consolidation is enabled.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub consolidation_due: Vec<ConsolidationDueInfo>,
}

/// One consolidation candidate surfaced on the curate response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationDueInfo {
    /// Smallest member canonical id, a stable handle for the cluster.
    pub cluster_seed: Uuid,
    pub size: usize,
    pub coaccess_mass: f64,
    pub activation_mass: f64,
    /// `salient` (use) or `cold_dense` (non-use).
    pub trigger: String,
}
