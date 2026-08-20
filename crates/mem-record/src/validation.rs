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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ValidateMemoryRequest {
    /// Overrides the configured `reinforcement.validation_dry_run`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
    /// Controls how broadly validation searches for codebase proof.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof_scope: Option<ValidationProofScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationEvidenceInfo {
    pub kind: String,
    pub evidence_ref: String,
    pub stance: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationRunInfo {
    pub id: Uuid,
    pub canonical_id: Uuid,
    pub memory_id: Uuid,
    pub summary: String,
    pub trigger: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    pub dry_run: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_status: Option<String>,
    #[serde(default)]
    pub reasons: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<ValidationEvidenceInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof_scope: Option<ValidationProofScope>,
    #[serde(default)]
    pub proof_fallback_used: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposed_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposed_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub started_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationRunsResponse {
    pub project: String,
    pub runs: Vec<ValidationRunInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewValidationRequest {
    /// `apply`, `reject`, or `apply_preview`.
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewValidationResponse {
    pub run_id: Uuid,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_memory_id: Option<Uuid>,
}

/// A memory due for reinforcement validation (activation over threshold,
/// past cooldown, revalidation interval elapsed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationDueInfo {
    pub canonical_id: Uuid,
    pub memory_id: Uuid,
    pub summary: String,
    pub activation: f64,
    pub volatility: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppliedMemoryReplacement {
    pub old_memory_id: Uuid,
    pub old_summary: String,
    pub new_memory_id: Uuid,
    pub new_summary: String,
    pub automatic: bool,
    pub policy: ReplacementPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplacementProposalRecord {
    pub id: Uuid,
    pub project: String,
    pub target_memory_id: Uuid,
    pub target_summary: String,
    pub candidate_summary: String,
    pub candidate_canonical_text: String,
    pub candidate_memory_type: MemoryType,
    pub score: i32,
    pub policy: ReplacementPolicy,
    #[serde(default)]
    pub reasons: Vec<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplacementProposalListResponse {
    pub project: String,
    #[serde(default)]
    pub proposals: Vec<ReplacementProposalRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplacementProposalResolutionResponse {
    pub project: String,
    pub proposal_id: Uuid,
    pub status: String,
    pub policy: ReplacementPolicy,
    pub target_memory_id: Uuid,
    pub target_summary: String,
    pub candidate_summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_memory_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceProvenanceStatus {
    Verified,
    MissingFile,
    MissingSymbol,
    Unverifiable,
    Stale,
}

impl SourceProvenanceStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::MissingFile => "missing_file",
            Self::MissingSymbol => "missing_symbol",
            Self::Unverifiable => "unverifiable",
            Self::Stale => "stale",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceProvenanceRecord {
    pub status: SourceProvenanceStatus,
    pub checked_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceVerificationRequest {
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<String>,
    #[serde(default)]
    pub dry_run: bool,
}

impl ProvenanceVerificationRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.project.trim().is_empty() {
            return Err(ValidationError::new("project must be non-empty"));
        }
        if let Some(repo_root) = self.repo_root.as_deref()
            && repo_root.trim().is_empty()
        {
            return Err(ValidationError::new("repo_root must be non-empty"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceProvenanceVerification {
    pub source_id: Uuid,
    pub memory_id: Uuid,
    pub memory_summary: String,
    pub source_kind: SourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_kind: Option<String>,
    pub status: SourceProvenanceStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceVerificationResponse {
    pub project: String,
    pub repo_root: String,
    pub dry_run: bool,
    pub checked_at: DateTime<Utc>,
    pub checked_count: usize,
    pub verified_count: usize,
    pub missing_file_count: usize,
    pub missing_symbol_count: usize,
    pub unverifiable_count: usize,
    pub stale_count: usize,
    pub stored_count: usize,
    #[serde(default)]
    pub warnings: Vec<DiagnosticInfo>,
    #[serde(default)]
    pub items: Vec<SourceProvenanceVerification>,
}
