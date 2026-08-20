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
pub enum LoopMode {
    Off,
    Observe,
    SuggestOnly,
    DraftOutput,
    AutonomousSafe,
    Paused,
    Snoozed,
}

impl LoopMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Observe => "observe",
            Self::SuggestOnly => "suggest_only",
            Self::DraftOutput => "draft_output",
            Self::AutonomousSafe => "autonomous_safe",
            Self::Paused => "paused",
            Self::Snoozed => "snoozed",
        }
    }
}

impl fmt::Display for LoopMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LoopRiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

impl LoopRiskLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

impl fmt::Display for LoopRiskLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LoopScopeType {
    User,
    Workspace,
    Project,
    Repo,
}

impl LoopScopeType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Workspace => "workspace",
            Self::Project => "project",
            Self::Repo => "repo",
        }
    }
}

impl fmt::Display for LoopScopeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LoopTrustLevel {
    High,
    #[default]
    Medium,
    Low,
    DataOnly,
}

impl LoopTrustLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
            Self::DataOnly => "data_only",
        }
    }
}

impl fmt::Display for LoopTrustLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LoopRunStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Blocked,
}

impl LoopRunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LoopApprovalStatus {
    Pending,
    Approved,
    Rejected,
    Edited,
}

impl LoopApprovalStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Edited => "edited",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LoopActionKind {
    ReadMemory,
    ReadRepo,
    WriteRepo,
    RunCommand,
    CreateBranch,
    InvokeRunner,
    WriteMemoryProposal,
    MutateMemory,
    PushMain,
    Deploy,
    AccessSecret,
    DestructiveMigration,
    EnableLoop,
    SubmitFeedback,
}

impl LoopActionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ReadMemory => "read_memory",
            Self::ReadRepo => "read_repo",
            Self::WriteRepo => "write_repo",
            Self::RunCommand => "run_command",
            Self::CreateBranch => "create_branch",
            Self::InvokeRunner => "invoke_runner",
            Self::WriteMemoryProposal => "write_memory_proposal",
            Self::MutateMemory => "mutate_memory",
            Self::PushMain => "push_main",
            Self::Deploy => "deploy",
            Self::AccessSecret => "access_secret",
            Self::DestructiveMigration => "destructive_migration",
            Self::EnableLoop => "enable_loop",
            Self::SubmitFeedback => "submit_feedback",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopDefinitionRecord {
    pub id: Uuid,
    pub loop_id: String,
    pub version: i32,
    pub name: String,
    pub description: String,
    pub risk_level: LoopRiskLevel,
    pub default_mode: LoopMode,
    #[serde(default)]
    pub trigger_spec: serde_json::Value,
    #[serde(default)]
    pub context_spec: serde_json::Value,
    #[serde(default)]
    pub policy_spec: serde_json::Value,
    #[serde(default)]
    pub output_spec: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

/// Learned procedural utility for one loop, advisory only. Present on the
/// listing when a project is supplied and utility learning is enabled.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopUtilityInfo {
    pub loop_id: String,
    pub utility: f64,
    pub update_count: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommendation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopDefinitionsResponse {
    pub definitions: Vec<LoopDefinitionRecord>,
    /// Ordered highest-utility first; informational, never a permission input.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub utilities: Vec<LoopUtilityInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopDefinitionResponse {
    pub definition: LoopDefinitionRecord,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_settings: Option<EffectiveLoopSettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopSettingRecord {
    pub id: Uuid,
    pub loop_id: String,
    pub scope_type: LoopScopeType,
    pub scope_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<LoopMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budgets: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_overrides: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused_until: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snoozed_until: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectiveLoopSettings {
    pub loop_id: String,
    pub enabled: bool,
    pub mode: LoopMode,
    pub scope_type: LoopScopeType,
    pub scope_id: String,
    #[serde(default)]
    pub global_kill_switch: bool,
    #[serde(default)]
    pub blocked_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budgets: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_overrides: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused_until: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snoozed_until: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopSettingsUpdateRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_type: Option<LoopScopeType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<LoopMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budgets: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_overrides: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused_until: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snoozed_until: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default)]
    pub explicit_user_approval: bool,
}

impl LoopSettingsUpdateRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self
            .scope_id
            .as_ref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ValidationError::new("scope_id must be non-empty"));
        }
        if self
            .project
            .as_ref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ValidationError::new("project must be non-empty"));
        }
        if self
            .repo_root
            .as_ref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ValidationError::new("repo_root must be non-empty"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopSettingResponse {
    pub setting: LoopSettingRecord,
    pub effective_settings: EffectiveLoopSettings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<LoopApprovalRequestRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopGlobalStateResponse {
    pub kill_switch_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopGlobalStateUpdateRequest {
    pub kill_switch_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopRunRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_type: Option<LoopScopeType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<String>,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_payload: Option<serde_json::Value>,
}

impl LoopRunRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self
            .project
            .as_ref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ValidationError::new("project must be non-empty"));
        }
        if self
            .repo_root
            .as_ref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ValidationError::new("repo_root must be non-empty"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopTriggerRouteRequest {
    pub source: String,
    pub event_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<String>,
    #[serde(default)]
    pub payload: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_key: Option<String>,
    #[serde(default)]
    pub trust_level: LoopTrustLevel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debounce_seconds: Option<i64>,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidate_loop_ids: Vec<String>,
}

impl LoopTriggerRouteRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.source.trim().is_empty() {
            return Err(ValidationError::new("source must be non-empty"));
        }
        if self.event_type.trim().is_empty() {
            return Err(ValidationError::new("event_type must be non-empty"));
        }
        if self
            .project
            .as_ref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ValidationError::new("project must be non-empty"));
        }
        if self
            .repo_root
            .as_ref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ValidationError::new("repo_root must be non-empty"));
        }
        if self
            .dedupe_key
            .as_ref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ValidationError::new("dedupe_key must be non-empty"));
        }
        if self.debounce_seconds.is_some_and(|seconds| seconds < 0) {
            return Err(ValidationError::new(
                "debounce_seconds must be greater than or equal to zero",
            ));
        }
        if self
            .candidate_loop_ids
            .iter()
            .any(|loop_id| loop_id.trim().is_empty())
        {
            return Err(ValidationError::new(
                "candidate_loop_ids must not contain empty loop ids",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopTriggerEventRecord {
    pub id: Uuid,
    pub source: String,
    pub event_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<String>,
    pub payload_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_key: Option<String>,
    pub trust_level: LoopTrustLevel,
    #[serde(default)]
    pub payload: serde_json::Value,
    pub received_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopTriggerRouteDecision {
    pub loop_id: String,
    pub supported: bool,
    pub eligible: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<LoopMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_type: Option<LoopScopeType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopTriggerRouteResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<LoopTriggerEventRecord>,
    #[serde(default)]
    pub duplicate: bool,
    #[serde(default)]
    pub debounced: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decisions: Vec<LoopTriggerRouteDecision>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<LoopRunSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopRunSummary {
    pub id: Uuid,
    pub loop_id: String,
    pub definition_version: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<String>,
    pub mode: LoopMode,
    pub status: LoopRunStatus,
    pub started_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_summary: Option<String>,
    pub trace_count: i32,
    #[serde(default)]
    pub blocked_reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopTraceRecord {
    pub id: Uuid,
    pub run_id: Uuid,
    pub sequence: i32,
    pub trace_type: String,
    pub title: String,
    #[serde(default)]
    pub payload: serde_json::Value,
    pub redacted: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopMemoryProposalRecord {
    pub id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    pub loop_id: String,
    pub proposal_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_memory_id: Option<Uuid>,
    #[serde(default)]
    pub candidate: serde_json::Value,
    #[serde(default)]
    pub evidence: serde_json::Value,
    pub confidence: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk_notes: Option<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopMemoryProposalsResponse {
    pub total_returned: usize,
    #[serde(default)]
    pub proposals: Vec<LoopMemoryProposalRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopMemoryProposalCreateRequest {
    pub project: String,
    pub loop_id: String,
    pub proposal_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_memory_id: Option<Uuid>,
    #[serde(default)]
    pub candidate: serde_json::Value,
    #[serde(default)]
    pub evidence: serde_json::Value,
    pub confidence: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk_notes: Option<String>,
}

impl LoopMemoryProposalCreateRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.project.trim().is_empty() {
            return Err(ValidationError::new("project is required"));
        }
        if self.loop_id.trim().is_empty() {
            return Err(ValidationError::new("loop_id is required"));
        }
        validate_loop_memory_proposal_type(&self.proposal_type)?;
        validate_loop_memory_proposal_confidence(self.confidence)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopMemoryProposalDecisionRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited_candidate: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited_evidence: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited_risk_notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopMemoryProposalDecisionResponse {
    pub proposal: LoopMemoryProposalRecord,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_id: Option<Uuid>,
}

pub fn validate_loop_memory_proposal_type(value: &str) -> Result<(), ValidationError> {
    match value {
        "add" | "update" | "deprecate" | "merge" | "link" => Ok(()),
        _ => Err(ValidationError::new(
            "proposal_type must be add, update, deprecate, merge, or link",
        )),
    }
}

pub fn validate_loop_memory_proposal_confidence(value: f32) -> Result<(), ValidationError> {
    if (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        Err(ValidationError::new("confidence must be between 0 and 1"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopRunDetail {
    pub summary: LoopRunSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_event: Option<LoopTriggerEventRecord>,
    #[serde(default)]
    pub effective_settings: serde_json::Value,
    #[serde(default)]
    pub policy_decisions: serde_json::Value,
    #[serde(default)]
    pub cost: serde_json::Value,
    #[serde(default)]
    pub output: serde_json::Value,
    #[serde(default)]
    pub traces: Vec<LoopTraceRecord>,
    #[serde(default)]
    pub memory_proposals: Vec<LoopMemoryProposalRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_pack: Option<LoopContextPack>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_diff: Option<LoopContextPackDiff>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopRunResponse {
    pub run: LoopRunDetail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopRunsResponse {
    pub total_returned: usize,
    #[serde(default)]
    pub runs: Vec<LoopRunSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopContextPackRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<Uuid>,
    #[serde(default = "default_context_pack_token_budget")]
    pub token_budget: usize,
    #[serde(default = "default_context_pack_limit")]
    pub limit: usize,
}

impl LoopContextPackRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self
            .project
            .as_deref()
            .is_none_or(|project| project.trim().is_empty())
        {
            return Err(ValidationError::new("project must be non-empty"));
        }
        if self.token_budget == 0 {
            return Err(ValidationError::new("token_budget must be positive"));
        }
        if self.limit == 0 {
            return Err(ValidationError::new("limit must be positive"));
        }
        Ok(())
    }
}

pub fn default_context_pack_token_budget() -> usize {
    4_000
}

pub fn default_context_pack_limit() -> usize {
    24
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopContextPackResponse {
    pub pack: LoopContextPack,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff: Option<LoopContextPackDiff>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopContextPack {
    pub id: Uuid,
    pub loop_id: String,
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<Uuid>,
    pub generated_at: DateTime<Utc>,
    pub token_budget: usize,
    pub estimated_tokens: usize,
    #[serde(default)]
    pub instructions: Vec<LoopContextInstructionRef>,
    #[serde(default)]
    pub memories: Vec<LoopContextMemory>,
    #[serde(default)]
    pub exclusions: Vec<LoopContextExclusion>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopContextInstructionRef {
    pub path: String,
    pub reason: String,
    pub estimated_tokens: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopContextMemory {
    pub memory_id: Uuid,
    pub canonical_id: Uuid,
    pub summary: String,
    pub preview: String,
    pub memory_type: MemoryType,
    pub confidence: f32,
    pub importance: i32,
    pub freshness: String,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub source_refs: Vec<LoopContextSourceRef>,
    pub estimated_tokens: usize,
    #[serde(default)]
    pub stale: bool,
    #[serde(default)]
    pub contradictory: bool,
    #[serde(default)]
    pub inclusion_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopContextSourceRef {
    pub source_kind: SourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance_status: Option<SourceProvenanceStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopContextExclusion {
    pub memory_id: Uuid,
    pub summary: String,
    pub reason: String,
    pub estimated_tokens: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LoopContextPackDiff {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_pack_id: Option<Uuid>,
    #[serde(default)]
    pub added_memory_ids: Vec<Uuid>,
    #[serde(default)]
    pub removed_memory_ids: Vec<Uuid>,
    #[serde(default)]
    pub changed_memory_ids: Vec<Uuid>,
    #[serde(default)]
    pub token_delta: isize,
    #[serde(default)]
    pub warning_delta: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopCancelRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopFeedbackRequest {
    pub rating: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl LoopFeedbackRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.rating.trim().is_empty() {
            return Err(ValidationError::new("rating must be non-empty"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopApprovalRequestRecord {
    pub id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    pub loop_id: String,
    pub action_type: String,
    #[serde(default)]
    pub proposed_action: serde_json::Value,
    pub risk_reason: String,
    pub status: LoopApprovalStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requester: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopApprovalsResponse {
    pub total_returned: usize,
    #[serde(default)]
    pub approvals: Vec<LoopApprovalRequestRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopApprovalDecisionRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited_action: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopApprovalDecisionResponse {
    pub approval: LoopApprovalRequestRecord,
}
