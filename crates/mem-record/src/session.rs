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
pub struct ResumeCheckpoint {
    pub project: String,
    pub repo_root: String,
    pub marked_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_head: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeRequest {
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<ResumeCheckpoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<DateTime<Utc>>,
    #[serde(default = "default_true")]
    pub include_llm_summary: bool,
    #[serde(default = "default_resume_limit")]
    pub limit: usize,
}

impl ResumeRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.project.trim().is_empty() {
            return Err(ValidationError::new("project must be non-empty"));
        }
        if self.limit == 0 {
            return Err(ValidationError::new("limit must be greater than zero"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeAction {
    pub title: String,
    pub rationale: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeResponse {
    pub project: String,
    pub generated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<ResumeCheckpoint>,
    pub briefing: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_thread: Option<String>,
    #[serde(default)]
    pub change_summary: Vec<String>,
    #[serde(default)]
    pub attention_items: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_next_step: Option<ResumeAction>,
    #[serde(default)]
    pub secondary_next_steps: Vec<ResumeAction>,
    #[serde(default)]
    pub context_items: Vec<ProjectMemoryListItem>,
    #[serde(default)]
    pub timeline: Vec<ActivityEvent>,
    #[serde(default)]
    pub commits: Vec<CommitRecord>,
    #[serde(default)]
    pub changed_memories: Vec<ProjectMemoryListItem>,
    #[serde(default)]
    pub durable_context: Vec<ProjectMemoryListItem>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub actions: Vec<ResumeAction>,
    pub overview: ProjectOverviewResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitSyncItem {
    pub hash: String,
    pub short_hash: String,
    pub subject: String,
    #[serde(default)]
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_email: Option<String>,
    pub committed_at: DateTime<Utc>,
    #[serde(default)]
    pub parent_hashes: Vec<String>,
    #[serde(default)]
    pub changed_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitSyncRequest {
    pub project: String,
    pub repo_root: String,
    #[serde(default)]
    pub commits: Vec<CommitSyncItem>,
    #[serde(default)]
    pub dry_run: bool,
}

impl CommitSyncRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.project.trim().is_empty() {
            return Err(ValidationError::new("project must be non-empty"));
        }
        if self.repo_root.trim().is_empty() {
            return Err(ValidationError::new("repo_root must be non-empty"));
        }
        if self.commits.is_empty() {
            return Err(ValidationError::new("commits must be non-empty"));
        }
        for commit in &self.commits {
            if commit.hash.trim().is_empty() {
                return Err(ValidationError::new("commit hash must be non-empty"));
            }
            if commit.short_hash.trim().is_empty() {
                return Err(ValidationError::new("commit short_hash must be non-empty"));
            }
            if commit.subject.trim().is_empty() {
                return Err(ValidationError::new("commit subject must be non-empty"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitSyncResponse {
    pub project_id: Uuid,
    pub imported_count: usize,
    pub updated_count: usize,
    pub total_received: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub newest_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oldest_commit: Option<String>,
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitRecord {
    pub hash: String,
    pub short_hash: String,
    pub subject: String,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_email: Option<String>,
    pub committed_at: DateTime<Utc>,
    #[serde(default)]
    pub parent_hashes: Vec<String>,
    #[serde(default)]
    pub changed_paths: Vec<String>,
    pub imported_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectCommitsResponse {
    pub project: String,
    pub total: i64,
    pub items: Vec<CommitRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitDetailResponse {
    pub project: String,
    pub commit: CommitRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsResponse {
    pub projects: i64,
    pub sessions: i64,
    pub tasks: i64,
    pub raw_captures: i64,
    pub memory_entries: i64,
    pub curation_runs: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamRequest {
    Authenticate { token: String },
    Health,
    ProjectOverview { project: String },
    ProjectMemories { project: String },
    MemoryDetail { memory_id: Uuid },
    SubscribeProject { project: String },
    SubscribeMemory { memory_id: Uuid },
    UnsubscribeMemory,
    Ping,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamResponse {
    Health {
        value: serde_json::Value,
    },
    ProjectOverview {
        value: ProjectOverviewResponse,
    },
    ProjectMemories {
        value: ProjectMemoriesResponse,
    },
    MemoryDetail {
        value: Option<MemoryEntryResponse>,
    },
    ProjectSnapshot {
        overview: ProjectOverviewResponse,
        memories: ProjectMemoriesResponse,
    },
    ProjectChanged {
        overview: ProjectOverviewResponse,
        memories: ProjectMemoriesResponse,
    },
    MemorySnapshot {
        detail: Option<MemoryEntryResponse>,
    },
    MemoryChanged {
        detail: Option<MemoryEntryResponse>,
    },
    Activity {
        event: ActivityEvent,
    },
    Ack {
        message: String,
    },
    Pong,
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpToSpeedRequest {
    pub project: String,
    #[serde(default)]
    pub include_llm_summary: bool,
    #[serde(default = "default_up_to_speed_limit")]
    pub limit: usize,
}

impl UpToSpeedRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.project.trim().is_empty() {
            return Err(ValidationError::new("project must be non-empty"));
        }
        if self.limit == 0 {
            return Err(ValidationError::new("limit must be positive"));
        }
        Ok(())
    }
}

pub fn default_up_to_speed_limit() -> usize {
    20
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpToSpeedResponse {
    pub project: String,
    pub generated_at: DateTime<Utc>,
    pub briefing: String,
    #[serde(default)]
    pub current_focus: Vec<String>,
    #[serde(default)]
    pub recent_work: Vec<String>,
    #[serde(default)]
    pub blockers: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<ResumeAction>,
    #[serde(default)]
    pub useful_memories: Vec<ProjectMemoryListItem>,
    #[serde(default)]
    pub recent_activities: Vec<ActivityEvent>,
    #[serde(default)]
    pub token_usage: TokenUsageSummary,
    #[serde(default)]
    pub warnings: Vec<String>,
}

pub fn default_resume_limit() -> usize {
    12
}
