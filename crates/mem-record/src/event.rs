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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    Checkpoint,
    Scan,
    Plan,
    CommitSync,
    BundleExport,
    BundleImport,
    GraphExtract,
    Query,
    QueryError,
    WatcherHealth,
    MemoryReplacement,
    CaptureTask,
    Curate,
    Reindex,
    Reembed,
    Archive,
    DeleteMemory,
    Briefing,
    Diagnostic,
    LlmAudit,
    MemoryValidation,
    LoopRunStarted,
    LoopRunFinished,
    LoopRunFailed,
    LoopSettingChanged,
    ProposalCreated,
    ProposalDecided,
    ProposalApplied,
    Consolidation,
    ProvenanceCheck,
    WorkspaceChanged,
    TriggerReceived,
    AuthEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActivityDetails {
    Checkpoint {
        repo_root: String,
        marked_at: DateTime<Utc>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        git_branch: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        git_head: Option<String>,
    },
    Plan {
        action: PlanActivityAction,
        title: String,
        thread_key: String,
        total_items: usize,
        completed_items: usize,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        remaining_items: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_path: Option<String>,
        verified_complete: bool,
    },
    Scan {
        dry_run: bool,
        candidate_count: usize,
        files_considered: usize,
        commits_considered: usize,
        index_reused: bool,
        report_path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        capture_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        curate_run_id: Option<String>,
    },
    GraphExtract {
        repo_root: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        git_head: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        since: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extraction_run_id: Option<Uuid>,
        dry_run: bool,
        reused_existing_run: bool,
        index_reused: bool,
        analyzer_version: String,
        strategy_version: String,
        symbol_count: usize,
        reference_count: usize,
        resolved_reference_count: usize,
        unresolved_reference_count: usize,
        ambiguous_reference_count: usize,
        graph_node_count: usize,
        graph_edge_count: usize,
        evidence_count: usize,
    },
    CommitSync {
        imported_count: usize,
        updated_count: usize,
        total_received: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        newest_commit: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        oldest_commit: Option<String>,
    },
    BundleTransfer {
        bundle_id: String,
        item_count: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_project: Option<String>,
    },
    Query {
        query: String,
        top_k: i64,
        result_count: usize,
        confidence: f32,
        insufficient_evidence: bool,
        total_duration_ms: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        graph_status: Option<String>,
        #[serde(default)]
        graph_candidates: usize,
        #[serde(default)]
        graph_augmented_candidates: usize,
        #[serde(default)]
        graph_duration_ms: u64,
        #[serde(default)]
        graph_result_count: usize,
        #[serde(default)]
        graph_connection_count: usize,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        graph_connections: Vec<QueryGraphConnection>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        answer: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    LlmAudit {
        operation: String,
        request_summary: String,
        status: String,
        redacted: bool,
        truncated: bool,
        messages: Vec<LlmAuditMessage>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    WatcherHealth {
        watcher_id: String,
        hostname: String,
        health: WatcherHealth,
        managed_by_service: bool,
        restart_attempt_count: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_cli: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_session_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_pid: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        previous_health: Option<WatcherHealth>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        recovered_after_restart_attempts: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    MemoryReplacement {
        old_memory_id: Uuid,
        old_summary: String,
        new_memory_id: Uuid,
        new_summary: String,
        automatic: bool,
        policy: ReplacementPolicy,
    },
    CaptureTask {
        session_id: Uuid,
        task_id: Uuid,
        raw_capture_id: Uuid,
        idempotency_key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        task_title: Option<String>,
        #[serde(alias = "agent_id")]
        writer_id: String,
    },
    Curate {
        run_id: Uuid,
        input_count: i64,
        output_count: i64,
        replaced_count: i64,
        proposal_count: i64,
    },
    Reindex {
        reindexed_entries: u64,
    },
    Reembed {
        reembedded_chunks: u64,
    },
    Archive {
        archived_count: u64,
        max_confidence: f32,
        max_importance: i32,
    },
    DeleteMemory {
        deleted: bool,
        summary: String,
    },
    Diagnostic {
        diagnostic: DiagnosticInfo,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LlmAuditMessage {
    pub role: String,
    pub content: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEvent {
    pub id: Uuid,
    /// Monotonic position in the project timeline log, when persisted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<i64>,
    pub recorded_at: DateTime<Utc>,
    pub project: String,
    pub kind: ActivityKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_id: Option<Uuid>,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<ActivityDetails>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<TokenUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityListResponse {
    pub project: String,
    pub total_returned: usize,
    #[serde(default)]
    pub items: Vec<ActivityEvent>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsageSummary {
    #[serde(default)]
    pub action_count: usize,
    #[serde(default)]
    pub total_input_tokens: u64,
    #[serde(default)]
    pub total_output_tokens: u64,
    #[serde(default)]
    pub total_cache_read_tokens: u64,
    #[serde(default)]
    pub total_cache_write_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanActivityAction {
    Started,
    Synced,
    FinishBlocked,
    FinishVerified,
}

impl fmt::Display for PlanActivityAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Started => "started",
            Self::Synced => "synced",
            Self::FinishBlocked => "finish_blocked",
            Self::FinishVerified => "finish_verified",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanActivityRequest {
    pub project: String,
    pub action: PlanActivityAction,
    pub title: String,
    pub thread_key: String,
    pub total_items: usize,
    pub completed_items: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remaining_items: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
}

impl PlanActivityRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.project.trim().is_empty() {
            return Err(ValidationError::new("project must be non-empty"));
        }
        if self.title.trim().is_empty() {
            return Err(ValidationError::new("title must be non-empty"));
        }
        if self.thread_key.trim().is_empty() {
            return Err(ValidationError::new("thread_key must be non-empty"));
        }
        if self.completed_items > self.total_items {
            return Err(ValidationError::new(
                "completed_items must not exceed total_items",
            ));
        }
        if self.remaining_items.len() > self.total_items {
            return Err(ValidationError::new(
                "remaining_items must not exceed total_items",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanActivityRequest {
    pub project: String,
    pub dry_run: bool,
    pub candidate_count: usize,
    pub files_considered: usize,
    pub commits_considered: usize,
    pub index_reused: bool,
    pub report_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curate_run_id: Option<String>,
}

impl ScanActivityRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.project.trim().is_empty() {
            return Err(ValidationError::new("project must be non-empty"));
        }
        if self.report_path.trim().is_empty() {
            return Err(ValidationError::new("report_path must be non-empty"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphActivityRequest {
    pub project: String,
    pub repo_root: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extraction_run_id: Option<Uuid>,
    pub dry_run: bool,
    pub reused_existing_run: bool,
    pub index_reused: bool,
    pub analyzer_version: String,
    pub strategy_version: String,
    pub symbol_count: usize,
    pub reference_count: usize,
    pub resolved_reference_count: usize,
    pub unresolved_reference_count: usize,
    pub ambiguous_reference_count: usize,
    pub graph_node_count: usize,
    pub graph_edge_count: usize,
    pub evidence_count: usize,
}

impl GraphActivityRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.project.trim().is_empty() {
            return Err(ValidationError::new("project must be non-empty"));
        }
        if self.repo_root.trim().is_empty() {
            return Err(ValidationError::new("repo_root must be non-empty"));
        }
        if self.analyzer_version.trim().is_empty() {
            return Err(ValidationError::new("analyzer_version must be non-empty"));
        }
        if self.strategy_version.trim().is_empty() {
            return Err(ValidationError::new("strategy_version must be non-empty"));
        }
        if !self.dry_run && self.extraction_run_id.is_none() {
            return Err(ValidationError::new(
                "extraction_run_id is required for persisted graph extraction activity",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointActivityRequest {
    pub project: String,
    pub checkpoint: ResumeCheckpoint,
}

impl CheckpointActivityRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.project.trim().is_empty() {
            return Err(ValidationError::new("project must be non-empty"));
        }
        if self.checkpoint.project.trim().is_empty() {
            return Err(ValidationError::new("checkpoint.project must be non-empty"));
        }
        if self.checkpoint.repo_root.trim().is_empty() {
            return Err(ValidationError::new(
                "checkpoint.repo_root must be non-empty",
            ));
        }
        Ok(())
    }
}
