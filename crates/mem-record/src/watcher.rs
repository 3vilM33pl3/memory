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
pub enum AgentWorkspaceStatus {
    Active,
    Completed,
    Abandoned,
}

impl fmt::Display for AgentWorkspaceStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Active => "active",
            Self::Completed => "completed",
            Self::Abandoned => "abandoned",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentWorkspaceWarning {
    pub code: String,
    pub severity: DiagnosticSeverity,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentWorkspaceRecord {
    pub id: Uuid,
    pub project: String,
    pub repo_root: String,
    pub worktree_path: String,
    pub branch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_commit: Option<String>,
    #[serde(default)]
    pub dirty_files: Vec<String>,
    pub dirty_count: usize,
    pub agent_cli: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub writer_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_endpoint: Option<String>,
    pub started_at: DateTime<Utc>,
    pub last_heartbeat_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
    pub status: AgentWorkspaceStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pushed_branch: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_commit: Option<String>,
    #[serde(default)]
    pub warnings: Vec<AgentWorkspaceWarning>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentWorkspaceListResponse {
    pub project: String,
    #[serde(default)]
    pub workspaces: Vec<AgentWorkspaceRecord>,
    #[serde(default)]
    pub warnings: Vec<AgentWorkspaceWarning>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentWorkspaceStartRequest {
    pub project: String,
    pub repo_root: String,
    pub worktree_path: String,
    pub branch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_commit: Option<String>,
    #[serde(default)]
    pub dirty_files: Vec<String>,
    #[serde(default)]
    pub agent_cli: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub writer_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_endpoint: Option<String>,
}

impl AgentWorkspaceStartRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.project.trim().is_empty() {
            return Err(ValidationError::new("project must be non-empty"));
        }
        if self.repo_root.trim().is_empty() {
            return Err(ValidationError::new("repo_root must be non-empty"));
        }
        if self.worktree_path.trim().is_empty() {
            return Err(ValidationError::new("worktree_path must be non-empty"));
        }
        if self.branch.trim().is_empty() {
            return Err(ValidationError::new("branch must be non-empty"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentWorkspaceHeartbeatRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_commit: Option<String>,
    #[serde(default)]
    pub dirty_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_endpoint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentWorkspaceFinishRequest {
    #[serde(default)]
    pub status: Option<AgentWorkspaceStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_commit: Option<String>,
    #[serde(default)]
    pub dirty_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pushed_branch: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_commit: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutomationMode {
    #[default]
    Suggest,
    Auto,
}

impl fmt::Display for AutomationMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Suggest => "suggest",
            Self::Auto => "auto",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationStatus {
    pub enabled: bool,
    pub mode: AutomationMode,
    pub repo_root: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_activity_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_persisted_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dirty_file_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_note_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_decision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatcherPresenceSummary {
    pub active_count: usize,
    pub unhealthy_count: usize,
    pub stale_after_seconds: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_heartbeat_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub watchers: Vec<WatcherPresence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WatcherHealth {
    Healthy,
    Stale,
    Restarting,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatcherPresence {
    pub watcher_id: String,
    pub project: String,
    pub repo_root: String,
    pub hostname: String,
    pub host_service_id: String,
    pub pid: u32,
    pub mode: AutomationMode,
    pub managed_by_service: bool,
    pub health: WatcherHealth,
    pub started_at: DateTime<Utc>,
    pub last_heartbeat_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_cli: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_started_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_restart_attempt_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub restart_attempt_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatcherHeartbeatRequest {
    pub watcher_id: String,
    pub project: String,
    pub repo_root: String,
    pub hostname: String,
    pub host_service_id: String,
    pub pid: u32,
    pub mode: AutomationMode,
    pub managed_by_service: bool,
    pub started_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_cli: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_started_at: Option<DateTime<Utc>>,
}

impl WatcherHeartbeatRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.watcher_id.trim().is_empty() {
            return Err(ValidationError::new("watcher_id must be non-empty"));
        }
        if self.project.trim().is_empty() {
            return Err(ValidationError::new("project must be non-empty"));
        }
        if self.repo_root.trim().is_empty() {
            return Err(ValidationError::new("repo_root must be non-empty"));
        }
        if self.hostname.trim().is_empty() {
            return Err(ValidationError::new("hostname must be non-empty"));
        }
        if self.host_service_id.trim().is_empty() {
            return Err(ValidationError::new("host_service_id must be non-empty"));
        }
        if self.pid == 0 {
            return Err(ValidationError::new("pid must be non-zero"));
        }
        if self
            .agent_cli
            .as_ref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ValidationError::new("agent_cli must be non-empty when set"));
        }
        if self
            .agent_session_id
            .as_ref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ValidationError::new(
                "agent_session_id must be non-empty when set",
            ));
        }
        if self.agent_pid.is_some_and(|value| value == 0) {
            return Err(ValidationError::new("agent_pid must be non-zero when set"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatcherRestartRequest {
    pub project: String,
    pub watcher_id: String,
    pub host_service_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session_id: Option<String>,
}

impl WatcherRestartRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.project.trim().is_empty() {
            return Err(ValidationError::new("project must be non-empty"));
        }
        if self.watcher_id.trim().is_empty() {
            return Err(ValidationError::new("watcher_id must be non-empty"));
        }
        if self.host_service_id.trim().is_empty() {
            return Err(ValidationError::new("host_service_id must be non-empty"));
        }
        if self
            .agent_session_id
            .as_ref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ValidationError::new(
                "agent_session_id must be non-empty when set",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatcherRestartResponse {
    pub accepted: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatcherUnregisterRequest {
    pub watcher_id: String,
    pub project: String,
}

impl WatcherUnregisterRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.watcher_id.trim().is_empty() {
            return Err(ValidationError::new("watcher_id must be non-empty"));
        }
        if self.project.trim().is_empty() {
            return Err(ValidationError::new("project must be non-empty"));
        }
        Ok(())
    }
}
