use std::{
    collections::HashMap,
    env, fmt,
    io::Cursor,
    path::{Path, PathBuf},
    time::Duration,
};

use capnp::{message::ReaderOptions, serialize};
use chrono::{DateTime, Utc};
use config::{Config, ConfigError, Environment, File, FileFormat};
use mem_platform::{default_shared_capnp_unix_socket, discover_existing_global_config_path};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use uuid::Uuid;

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
    Task,
    Plan,
    Implementation,
    User,
    Feedback,
    Project,
    Reference,
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
            Self::Task => "task",
            Self::Plan => "plan",
            Self::Implementation => "implementation",
            Self::User => "user",
            Self::Feedback => "feedback",
            Self::Project => "project",
            Self::Reference => "reference",
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemoryRelationType {
    Duplicates,
    Supersedes,
    Supports,
    RelatedTo,
    DependsOn,
}

impl fmt::Display for MemoryRelationType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Duplicates => "duplicates",
            Self::Supersedes => "supersedes",
            Self::Supports => "supports",
            Self::RelatedTo => "related_to",
            Self::DependsOn => "depends_on",
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
}

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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QueryFilters {
    #[serde(default)]
    pub types: Vec<MemoryType>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub enum QueryRetrievalMode {
    Lexical,
    Semantic,
    Graph,
    #[default]
    FullMemory,
}

impl fmt::Display for QueryRetrievalMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Lexical => "lexical",
            Self::Semantic => "semantic",
            Self::Graph => "graph",
            Self::FullMemory => "full-memory",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub enum QueryAnswerMode {
    #[default]
    Auto,
    Deterministic,
    Llm,
}

impl fmt::Display for QueryAnswerMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Auto => "auto",
            Self::Deterministic => "deterministic",
            Self::Llm => "llm",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryRequest {
    pub project: String,
    pub query: String,
    #[serde(default)]
    pub filters: QueryFilters,
    #[serde(default = "default_top_k")]
    pub top_k: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_confidence: Option<f32>,
    /// When true, search across every version of every canonical memory
    /// (including tombstones). Default is false, which restricts the search
    /// to the latest non-tombstone version per canonical_id. Use for
    /// deep-history or audit-style queries.
    #[serde(default)]
    pub history: bool,
    /// Optional eval/debug control for isolating retrieval channels.
    /// Normal user queries should omit this and use full memory behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retrieval_mode: Option<QueryRetrievalMode>,
    /// Optional eval/debug control for answer synthesis.
    /// Normal user queries should omit this and let the service choose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer_mode: Option<QueryAnswerMode>,
}

impl QueryRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.project.trim().is_empty() {
            return Err(ValidationError::new("project must be non-empty"));
        }
        if self.query.trim().is_empty() {
            return Err(ValidationError::new("query must be non-empty"));
        }
        if !(1..=50).contains(&self.top_k) {
            return Err(ValidationError::new("top_k must be in 1..=50"));
        }
        if let Some(value) = self.min_confidence
            && !(0.0..=1.0).contains(&value)
        {
            return Err(ValidationError::new("min_confidence must be in 0.0..=1.0"));
        }
        Ok(())
    }
}

fn default_top_k() -> i64 {
    8
}

fn default_candidate_confidence() -> f32 {
    0.75
}

fn default_candidate_importance() -> i32 {
    2
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuerySource {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    pub source_kind: SourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub memory_id: Uuid,
    pub summary: String,
    pub memory_type: MemoryType,
    pub score: f64,
    pub snippet: String,
    #[serde(default)]
    pub match_kind: QueryMatchKind,
    #[serde(default)]
    pub score_explanation: Vec<String>,
    #[serde(default)]
    pub debug: QueryResultDebug,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub sources: Vec<QuerySource>,
    #[serde(default)]
    pub graph_connections: Vec<QueryGraphConnection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QueryGraphConnection {
    pub file_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub neighbor_symbol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
    pub score_boost: f64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum QueryMatchKind {
    #[default]
    Lexical,
    Semantic,
    Hybrid,
}

impl fmt::Display for QueryMatchKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Lexical => "lexical",
            Self::Semantic => "semantic",
            Self::Hybrid => "hybrid",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QueryResultDebug {
    #[serde(default)]
    pub chunk_fts: f64,
    #[serde(default)]
    pub entry_fts: f64,
    #[serde(default)]
    pub semantic_similarity: f64,
    #[serde(default)]
    pub exact_phrase_matches: usize,
    #[serde(default)]
    pub term_overlap: f64,
    #[serde(default)]
    pub tag_match_count: usize,
    #[serde(default)]
    pub path_match_count: usize,
    #[serde(default)]
    pub relation_boost: f64,
    #[serde(default)]
    pub graph_boost: f64,
    #[serde(default)]
    pub graph_match_count: usize,
    #[serde(default)]
    pub graph_edge_count: usize,
    #[serde(default)]
    pub importance: i32,
    #[serde(default)]
    pub memory_confidence: f32,
    #[serde(default)]
    pub recency_boost: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QueryDiagnostics {
    #[serde(default)]
    pub retrieval_mode: QueryRetrievalMode,
    #[serde(default)]
    pub lexical_enabled: bool,
    #[serde(default)]
    pub semantic_enabled: bool,
    #[serde(default)]
    pub graph_enabled: bool,
    #[serde(default)]
    pub relation_boost_enabled: bool,
    #[serde(default)]
    pub lexical_candidates: usize,
    #[serde(default)]
    pub semantic_candidates: usize,
    #[serde(default)]
    pub merged_candidates: usize,
    #[serde(default)]
    pub returned_results: usize,
    #[serde(default)]
    pub relation_augmented_candidates: usize,
    #[serde(default)]
    pub graph_candidates: usize,
    #[serde(default)]
    pub graph_augmented_candidates: usize,
    #[serde(default)]
    pub lexical_duration_ms: u64,
    #[serde(default)]
    pub semantic_duration_ms: u64,
    #[serde(default)]
    pub rerank_duration_ms: u64,
    #[serde(default)]
    pub graph_duration_ms: u64,
    #[serde(default)]
    pub total_duration_ms: u64,
    #[serde(default)]
    pub semantic_status: String,
    #[serde(default)]
    pub graph_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum QueryAnswerMethod {
    #[default]
    Deterministic,
    Llm,
    Fallback,
}

impl fmt::Display for QueryAnswerMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Deterministic => "deterministic",
            Self::Llm => "llm",
            Self::Fallback => "fallback",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QueryAnswerGeneration {
    #[serde(default)]
    pub method: QueryAnswerMethod,
    #[serde(default)]
    pub cited_result_numbers: Vec<usize>,
    #[serde(default)]
    pub evidence_count: usize,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<TokenUsage>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_tokens: u64,
    #[serde(default)]
    pub cache_write_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryAnswerCitation {
    pub result_number: usize,
    pub memory_id: Uuid,
    pub memory_type: MemoryType,
    pub summary: String,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedMemorySummary {
    pub memory_id: Uuid,
    pub relation_type: MemoryRelationType,
    pub summary: String,
    pub memory_type: MemoryType,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResponse {
    pub answer: String,
    pub confidence: f32,
    pub results: Vec<QueryResult>,
    pub insufficient_evidence: bool,
    #[serde(default)]
    pub answer_generation: QueryAnswerGeneration,
    #[serde(default)]
    pub answer_citations: Vec<QueryAnswerCitation>,
    #[serde(default)]
    pub diagnostics: QueryDiagnostics,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySourceRecord {
    pub id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_commit: Option<String>,
    pub source_kind: SourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
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

fn default_version_no() -> i32 {
    1
}

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
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemoryBundleEntry {
    pub entry_key: String,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    #[default]
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct DiagnosticInfo {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub component: String,
    #[serde(default)]
    pub operation: String,
    #[serde(default)]
    pub severity: DiagnosticSeverity,
    #[serde(default)]
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doctor_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_hint: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEvent {
    #[serde(default = "Uuid::new_v4")]
    pub id: Uuid,
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

fn default_up_to_speed_limit() -> usize {
    20
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

fn default_archive_threshold() -> f32 {
    0.3
}

fn default_archive_importance() -> i32 {
    1
}

fn default_true() -> bool {
    true
}

fn default_resume_limit() -> usize {
    12
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveResponse {
    pub archived_count: u64,
    #[serde(default)]
    pub dry_run: bool,
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
    /// Restrict to a single configured backend by name. `None` means
    /// every configured backend is reindexed so all spaces stay covered.
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

/// Full version chain for a single canonical memory. Resolved from any
/// version's `id`; the response walks back to the canonical_id and returns
/// every row (including tombstones) ordered oldest → newest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryHistoryResponse {
    pub canonical_id: Uuid,
    pub project: String,
    pub versions: Vec<MemoryEntryResponse>,
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

fn current_exe_is_in_cargo_target() -> bool {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub service: ServiceConfig,
    pub database: DatabaseConfig,
    #[serde(default)]
    pub features: FeatureFlags,
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub embeddings: EmbeddingsConfig,
    #[serde(default)]
    pub cluster: ClusterConfig,
    #[serde(default, alias = "agent")]
    pub writer: WriterConfig,
    #[serde(default)]
    pub automation: AutomationConfig,
    #[serde(default)]
    pub retention: RetentionConfig,
    #[serde(skip, default = "default_profile")]
    pub profile: Profile,
    /// Path of the resolved config file (base file in dev mode). Useful when
    /// spawning subprocesses that must reuse the same config.
    #[serde(skip)]
    pub resolved_config_path: Option<PathBuf>,
    /// Path of the dev overlay if one was applied.
    #[serde(skip)]
    pub resolved_dev_overlay_path: Option<PathBuf>,
}

fn default_profile() -> Profile {
    Profile::Prod
}

impl AppConfig {
    pub fn load_from_path(path: Option<PathBuf>) -> Result<Self, ConfigError> {
        Self::load_with_profile(path, Profile::detect())
    }

    pub fn load_with_profile(path: Option<PathBuf>, profile: Profile) -> Result<Self, ConfigError> {
        let mut builder = Config::builder();
        let mut env_files = Vec::new();
        let mut resolved_config_path: Option<PathBuf> = None;
        let mut resolved_dev_overlay_path: Option<PathBuf> = None;

        if let Some(path) = path {
            env_files.push(env_path_for_config(&path));
            resolved_config_path = Some(path.clone());
            builder = builder.add_source(File::from(path).required(false));
        } else {
            // Global config is part of the installed/prod stack; the dev
            // stack ignores it so a cargo-run service cannot silently pick
            // up the packaged machine-wide settings. Bootstrap shared
            // values (database URL, LLM endpoints) into .mem/config.dev.toml
            // via `memory dev init --copy-from-global`.
            if profile == Profile::Prod {
                if let Some(path) = discover_global_config_path() {
                    env_files.push(env_path_for_config(&path));
                    builder = builder.add_source(File::from(path).required(false));
                } else {
                    builder = builder.add_source(File::with_name("memory-layer").required(false));
                }
            }
            if let Some(path) = discover_repo_config_path() {
                env_files.push(
                    path.parent()
                        .unwrap_or_else(|| Path::new("."))
                        .join("memory-layer.env"),
                );
                resolved_config_path = Some(path.clone());
                builder = builder.add_source(File::from(path).required(false));
            }
        }

        if profile == Profile::Dev {
            let overlay_path = resolved_config_path
                .as_deref()
                .and_then(dev_overlay_path_for_base)
                .or_else(discover_repo_dev_config_path);
            match overlay_path {
                Some(path) if path.is_file() => {
                    resolved_dev_overlay_path = Some(path.clone());
                    builder = builder.add_source(File::from(path).required(false));
                }
                _ => {
                    return Err(ConfigError::Message(dev_overlay_missing_message(
                        resolved_config_path.as_deref(),
                    )));
                }
            }
        }

        for env_file in env_files {
            if let Some(source) = env_file_source(&env_file)? {
                builder = builder.add_source(source);
            }
        }

        let config = builder
            .add_source(Environment::with_prefix("MEMORY_LAYER").separator("__"))
            .build()?;
        let mut value: serde_json::Value = config.try_deserialize()?;
        normalize_legacy_config_keys(&mut value);
        let mut config: AppConfig =
            serde_json::from_value(value).map_err(|error| ConfigError::Foreign(Box::new(error)))?;
        config.profile = profile;
        config.resolved_config_path = resolved_config_path;
        config.resolved_dev_overlay_path = resolved_dev_overlay_path;
        config.apply_runtime_defaults();
        Ok(config)
    }

    fn apply_runtime_defaults(&mut self) {
        self.embeddings.normalize_backend_names();
        if self.cluster.service_id.trim().is_empty() {
            self.cluster.service_id = format!(
                "service-{}",
                self.service
                    .bind_addr
                    .chars()
                    .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
                    .collect::<String>()
                    .trim_matches('-')
            );
        }
    }
}

impl EmbeddingsConfig {
    /// Fills in a sensible `name` for every configured embedding backend
    /// that didn't ship one, deduplicating against existing names.
    /// Silently drops the `active` selector if it points at a missing
    /// backend so search falls back to "no embeddings" rather than
    /// crashing the service on startup — doctor/health will still flag it.
    /// If no explicit `active` is set and exactly one backend is
    /// configured, materialize that one into `active` so downstream
    /// code can trust `active` as the persistent source of truth.
    pub fn normalize_backend_names(&mut self) {
        let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
        for backend in &mut self.backends {
            if backend.name.trim().is_empty() {
                backend.name = derive_embedding_backend_name(backend);
            }
            let mut candidate = backend.name.clone();
            let mut suffix = 2;
            while used.contains(&candidate) {
                candidate = format!("{}-{suffix}", backend.name);
                suffix += 1;
            }
            backend.name = candidate.clone();
            used.insert(candidate);
        }
        if let Some(active) = self.active.as_deref()
            && !used.contains(active)
        {
            self.active = None;
        }
        if self.enabled && self.active.is_none() && self.backends.len() == 1 {
            self.active = Some(self.backends[0].name.clone());
        }
    }
}

fn derive_embedding_backend_name(backend: &EmbeddingBackendConfig) -> String {
    let provider = backend.provider.trim();
    let model = backend.model.trim();
    let combined = if provider.is_empty() && model.is_empty() {
        "embeddings".to_string()
    } else if provider.is_empty() {
        model.to_string()
    } else if model.is_empty() {
        provider.to_string()
    } else {
        format!("{provider}-{model}")
    };
    combined
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn normalize_legacy_config_keys(value: &mut serde_json::Value) {
    let Some(root) = value.as_object_mut() else {
        return;
    };
    let Some(automation) = root
        .get_mut("automation")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return;
    };

    if automation.contains_key("capture_idle_threshold") {
        automation.remove("idle_threshold");
    } else if let Some(legacy) = automation.remove("idle_threshold") {
        automation.insert("capture_idle_threshold".to_string(), legacy);
    }
}

fn env_file_source(
    path: &Path,
) -> Result<Option<config::File<config::FileSourceString, FileFormat>>, ConfigError> {
    let values = memory_layer_env_file_values(path)?;
    if values.is_empty() {
        return Ok(None);
    }

    let mut lines = values
        .into_iter()
        .map(|(key, value)| format!("{key} = {}", serde_json::to_string(&value).unwrap()))
        .collect::<Vec<_>>();
    lines.sort();
    Ok(Some(File::from_str(&lines.join("\n"), FileFormat::Toml)))
}

fn memory_layer_env_file_values(path: &Path) -> Result<HashMap<String, String>, ConfigError> {
    let mut values = HashMap::new();
    let Ok(content) = std::fs::read_to_string(path) else {
        return Ok(values);
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((name, value)) = trimmed.split_once('=') else {
            continue;
        };
        let key = name.trim();
        if !key.starts_with("MEMORY_LAYER__") {
            continue;
        }
        let config_key = key["MEMORY_LAYER__".len()..]
            .split("__")
            .map(|segment| segment.trim().to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join(".");
        values.insert(config_key, value.trim().to_string());
    }

    Ok(values)
}

fn env_path_for_config(path: &Path) -> PathBuf {
    path.parent()
        .unwrap_or_else(|| Path::new("."))
        .join("memory-layer.env")
}

pub fn discover_repo_config_path() -> Option<PathBuf> {
    let cwd = env::current_dir().ok()?;
    find_repo_config_path(&cwd)
}

pub fn dev_overlay_path_for_base(base: &Path) -> Option<PathBuf> {
    base.parent().map(|parent| parent.join("config.dev.toml"))
}

pub fn discover_repo_dev_config_path() -> Option<PathBuf> {
    let cwd = env::current_dir().ok()?;
    for directory in cwd.ancestors() {
        let candidate = directory.join(".mem").join("config.dev.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn dev_overlay_missing_message(base: Option<&Path>) -> String {
    let hint = match base.and_then(Path::parent) {
        Some(dir) => format!("{}/config.dev.toml", dir.display()),
        None => "<repo>/.mem/config.dev.toml".to_string(),
    };
    format!(
        "dev profile active but {hint} is missing. Run `memory dev init` to \
         scaffold it, or set MEMORY_LAYER_PROFILE=prod to opt out."
    )
}

pub fn discover_repo_env_path() -> Option<PathBuf> {
    let config_path = discover_repo_config_path()?;
    Some(env_path_for_config(&config_path))
}

pub fn discover_global_env_path() -> Option<PathBuf> {
    let config_path = discover_global_config_path()?;
    Some(env_path_for_config(&config_path))
}

pub fn discover_global_config_path() -> Option<PathBuf> {
    discover_existing_global_config_path()
}

pub fn find_repo_config_path(start: &Path) -> Option<PathBuf> {
    for directory in start.ancestors() {
        let candidate = directory.join(".mem").join("config.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentProjectConfig {
    #[serde(default)]
    pub capture: AgentCaptureConfig,
    #[serde(default)]
    pub analysis: AgentAnalysisConfig,
    #[serde(default)]
    pub retrieval: AgentRetrievalConfig,
    #[serde(default)]
    pub curation: AgentCurationConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentCaptureConfig {
    #[serde(default)]
    pub include_paths: Vec<String>,
    #[serde(default)]
    pub ignore_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAnalysisConfig {
    #[serde(default = "default_agent_analyzers")]
    pub analyzers: Vec<String>,
}

impl Default for AgentAnalysisConfig {
    fn default() -> Self {
        Self {
            analyzers: default_agent_analyzers(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentRetrievalConfig {
    #[serde(default)]
    pub graph_enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentCurationConfig {
    #[serde(default)]
    pub replacement_policy: ReplacementPolicy,
}

fn default_agent_analyzers() -> Vec<String> {
    vec![
        "rust".to_string(),
        "typescript".to_string(),
        "python".to_string(),
    ]
}

pub fn repo_agent_settings_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".agents").join("memory-layer.toml")
}

pub fn read_repo_project_slug(repo_root: &Path) -> Option<String> {
    let project_path = repo_root.join(".mem").join("project.toml");
    let content = std::fs::read_to_string(project_path).ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("slug = ") {
            let slug = value.trim().trim_matches('"').trim();
            if !slug.is_empty() {
                return Some(slug.to_string());
            }
        }
    }
    None
}

pub fn load_repo_agent_settings(repo_root: &Path) -> Result<AgentProjectConfig, ConfigError> {
    let path = repo_agent_settings_path(repo_root);
    if !path.exists() {
        return Ok(AgentProjectConfig::default());
    }

    let config = Config::builder()
        .add_source(File::from(path).format(FileFormat::Toml).required(false))
        .build()?;
    config.try_deserialize()
}

pub fn load_repo_replacement_policy(repo_root: &Path) -> Result<ReplacementPolicy, ConfigError> {
    Ok(load_repo_agent_settings(repo_root)?
        .curation
        .replacement_policy)
}

pub fn resolve_secret_value(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .or_else(|| discover_repo_env_path().and_then(|path| env_lookup(&path, key)))
        .or_else(|| discover_global_env_path().and_then(|path| env_lookup(&path, key)))
}

fn env_lookup(path: &Path, key: &str) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((name, value)) = trimmed.split_once('=')
            && name.trim() == key
        {
            return Some(value.trim().to_string());
        }
    }
    None
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceConfig {
    #[serde(default = "default_bind_addr")]
    pub bind_addr: String,
    #[serde(default = "default_capnp_unix_socket")]
    pub capnp_unix_socket: String,
    #[serde(default = "default_capnp_tcp_addr")]
    pub capnp_tcp_addr: String,
    #[serde(default)]
    pub web_root: Option<String>,
    #[serde(default = "default_api_token")]
    pub api_token: String,
    #[serde(default = "default_request_timeout")]
    #[serde(with = "humantime_serde")]
    pub request_timeout: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterConfig {
    #[serde(default = "default_cluster_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub service_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advertise_addr: Option<String>,
    #[serde(default = "default_cluster_discovery_multicast_addr")]
    pub discovery_multicast_addr: String,
    #[serde(default = "default_cluster_announce_interval")]
    #[serde(with = "humantime_serde")]
    pub announce_interval: Duration,
    #[serde(default = "default_cluster_peer_ttl")]
    #[serde(with = "humantime_serde")]
    pub peer_ttl: Duration,
    #[serde(default = "default_cluster_priority")]
    pub priority: i32,
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            enabled: default_cluster_enabled(),
            service_id: String::new(),
            advertise_addr: None,
            discovery_multicast_addr: default_cluster_discovery_multicast_addr(),
            announce_interval: default_cluster_announce_interval(),
            peer_ttl: default_cluster_peer_ttl(),
            priority: default_cluster_priority(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WriterConfig {
    #[serde(default)]
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FeatureFlags {
    #[serde(default)]
    pub llm_curation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    #[serde(default = "default_llm_provider")]
    pub provider: String,
    #[serde(default = "default_llm_base_url")]
    pub base_url: String,
    #[serde(default = "default_llm_api_key_env")]
    pub api_key_env: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub temperature: f32,
    #[serde(default = "default_llm_max_input_bytes")]
    pub max_input_bytes: usize,
    #[serde(default = "default_llm_max_output_tokens")]
    pub max_output_tokens: u32,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: default_llm_provider(),
            base_url: default_llm_base_url(),
            api_key_env: default_llm_api_key_env(),
            model: String::new(),
            temperature: 0.0,
            max_input_bytes: default_llm_max_input_bytes(),
            max_output_tokens: default_llm_max_output_tokens(),
        }
    }
}

pub const OPENAI_COMPATIBLE_PROVIDER: &str = "openai_compatible";
pub const OLLAMA_PROVIDER: &str = "ollama";
pub const OPENAI_COMPATIBLE_BASE_URL: &str = "https://api.openai.com/v1";
pub const OLLAMA_BASE_URL: &str = "http://127.0.0.1:11434/v1";

pub fn is_ollama_provider(provider: &str) -> bool {
    provider.trim().eq_ignore_ascii_case(OLLAMA_PROVIDER)
}

pub fn is_supported_llm_provider(provider: &str) -> bool {
    matches!(
        provider.trim(),
        OPENAI_COMPATIBLE_PROVIDER | OLLAMA_PROVIDER
    )
}

pub fn effective_llm_base_url(config: &LlmConfig) -> String {
    effective_llm_base_url_for(&config.provider, &config.base_url)
}

pub fn effective_llm_base_url_for(provider: &str, configured: &str) -> String {
    let configured = configured.trim().trim_end_matches('/');
    if is_ollama_provider(provider)
        && (configured.is_empty() || configured == OPENAI_COMPATIBLE_BASE_URL)
    {
        OLLAMA_BASE_URL.to_string()
    } else if configured.is_empty() {
        OPENAI_COMPATIBLE_BASE_URL.to_string()
    } else {
        configured.to_string()
    }
}

pub fn llm_max_output_tokens_field(provider: &str) -> &'static str {
    if is_ollama_provider(provider) {
        "max_tokens"
    } else {
        "max_completion_tokens"
    }
}

pub fn resolve_llm_api_key(config: &LlmConfig) -> Option<String> {
    let key = config.llm_api_key_env_for_resolution()?;
    resolve_secret_value(key).filter(|value| !value.trim().is_empty())
}

pub fn llm_requires_api_key(config: &LlmConfig) -> bool {
    if is_ollama_provider(&config.provider) {
        let key = config.api_key_env.trim();
        !key.is_empty() && key != default_llm_api_key_env()
    } else {
        true
    }
}

impl LlmConfig {
    fn llm_api_key_env_for_resolution(&self) -> Option<&str> {
        let key = self.api_key_env.trim();
        if key.is_empty() {
            return None;
        }
        if is_ollama_provider(&self.provider) && key == default_llm_api_key_env() {
            return None;
        }
        Some(key)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingBackendConfig {
    /// Stable identifier used by CLI/API activation and user-visible
    /// listings. When omitted in config, the loader auto-derives one
    /// from `{provider}-{model}` so users can start without naming.
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_embeddings_provider")]
    pub provider: String,
    #[serde(default = "default_embeddings_base_url")]
    pub base_url: String,
    #[serde(default = "default_embeddings_api_key_env")]
    pub api_key_env: String,
    #[serde(default)]
    pub model: String,
    #[serde(default = "default_embeddings_batch_size")]
    pub batch_size: usize,
    #[serde(default, alias = "dimension", skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<u32>,
    #[serde(default = "default_true")]
    pub create_enabled: bool,
}

impl Default for EmbeddingBackendConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            provider: default_embeddings_provider(),
            base_url: default_embeddings_base_url(),
            api_key_env: default_embeddings_api_key_env(),
            model: String::new(),
            batch_size: default_embeddings_batch_size(),
            dimensions: None,
            create_enabled: true,
        }
    }
}

/// Wraps one or more configured embedding backends plus the name of the
/// one currently used for search. Accepts two TOML shapes:
///
/// 1. Legacy singleton — a flat `[embeddings]` table with `provider`,
///    `model`, etc. Loads as a single backend named after
///    `{provider}-{model}` and marked active.
/// 2. Multi-backend — `[embeddings] active = "<name>"` plus one or
///    more `[[embeddings.backends]]` array-of-tables.
#[derive(Debug, Clone, Serialize)]
pub struct EmbeddingsConfig {
    pub enabled: bool,
    pub create_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
    #[serde(default)]
    pub backends: Vec<EmbeddingBackendConfig>,
}

impl Default for EmbeddingsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            create_enabled: true,
            active: None,
            backends: Vec::new(),
        }
    }
}

impl EmbeddingsConfig {
    /// The currently-active backend according to `active`, falling back
    /// to the sole backend when exactly one is configured and no
    /// explicit `active` was given. Returns `None` when no backends are
    /// configured at all.
    pub fn active_backend(&self) -> Option<&EmbeddingBackendConfig> {
        if !self.enabled {
            return None;
        }
        if let Some(name) = self.active.as_deref() {
            return self.backends.iter().find(|b| b.name == name);
        }
        if self.backends.len() == 1 {
            return self.backends.first();
        }
        None
    }

    /// Lookup a backend by name.
    pub fn backend(&self, name: &str) -> Option<&EmbeddingBackendConfig> {
        self.backends.iter().find(|b| b.name == name)
    }
}

impl<'de> Deserialize<'de> for EmbeddingsConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        let value = serde_json::Value::deserialize(deserializer)?;
        let map = match value {
            serde_json::Value::Null => return Ok(Self::default()),
            serde_json::Value::Object(map) => map,
            other => {
                return Err(D::Error::custom(format!(
                    "expected [embeddings] to be a table, got {other}"
                )));
            }
        };

        // New-form: presence of `backends` array wins.
        if map.contains_key("backends") {
            let enabled = match map.get("enabled") {
                Some(serde_json::Value::Bool(value)) => *value,
                Some(serde_json::Value::Null) | None => true,
                Some(other) => {
                    return Err(D::Error::custom(format!(
                        "embeddings.enabled must be a boolean, got {other}"
                    )));
                }
            };
            let create_enabled = match map.get("create_enabled") {
                Some(serde_json::Value::Bool(value)) => *value,
                Some(serde_json::Value::Null) | None => true,
                Some(other) => {
                    return Err(D::Error::custom(format!(
                        "embeddings.create_enabled must be a boolean, got {other}"
                    )));
                }
            };
            let active = match map.get("active") {
                Some(serde_json::Value::String(s)) => Some(s.clone()),
                Some(serde_json::Value::Null) | None => None,
                Some(other) => {
                    return Err(D::Error::custom(format!(
                        "embeddings.active must be a string, got {other}"
                    )));
                }
            };
            let backends_value = map.get("backends").cloned().unwrap_or_default();
            let backends: Vec<EmbeddingBackendConfig> =
                serde_json::from_value(backends_value).map_err(D::Error::custom)?;
            return Ok(Self {
                enabled,
                create_enabled,
                active,
                backends,
            });
        }

        let enabled = match map.get("enabled") {
            Some(serde_json::Value::Bool(value)) => *value,
            Some(serde_json::Value::Null) | None => true,
            Some(other) => {
                return Err(D::Error::custom(format!(
                    "embeddings.enabled must be a boolean, got {other}"
                )));
            }
        };
        let create_enabled = match map.get("create_enabled") {
            Some(serde_json::Value::Bool(value)) => *value,
            Some(serde_json::Value::Null) | None => true,
            Some(other) => {
                return Err(D::Error::custom(format!(
                    "embeddings.create_enabled must be a boolean, got {other}"
                )));
            }
        };

        // Legacy singleton: if nothing relevant is set, return a wholly
        // empty config so other code paths still see "no backends".
        let is_empty = !map.contains_key("provider")
            && !map.contains_key("model")
            && !map.contains_key("base_url")
            && !map.contains_key("api_key_env")
            && !map.contains_key("batch_size")
            && !map.contains_key("dimensions")
            && !map.contains_key("dimension")
            && !map.contains_key("name")
            && !map.contains_key("enabled")
            && !map.contains_key("create_enabled");
        if is_empty {
            return Ok(Self::default());
        }

        if !map.contains_key("provider")
            && !map.contains_key("model")
            && !map.contains_key("base_url")
            && !map.contains_key("api_key_env")
            && !map.contains_key("batch_size")
            && !map.contains_key("dimensions")
            && !map.contains_key("dimension")
            && !map.contains_key("name")
        {
            return Ok(Self {
                enabled,
                create_enabled,
                active: None,
                backends: Vec::new(),
            });
        }

        let backend: EmbeddingBackendConfig =
            serde_json::from_value(serde_json::Value::Object(map)).map_err(D::Error::custom)?;
        Ok(Self {
            enabled,
            create_enabled,
            active: if backend.name.is_empty() {
                None
            } else {
                Some(backend.name.clone())
            },
            backends: vec![backend],
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub mode: AutomationMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<String>,
    #[serde(default = "default_poll_interval")]
    #[serde(with = "humantime_serde")]
    pub poll_interval: Duration,
    #[serde(default = "default_file_events")]
    pub file_events: bool,
    #[serde(default = "default_capture_idle_threshold", alias = "idle_threshold")]
    #[serde(with = "humantime_serde")]
    pub capture_idle_threshold: Duration,
    #[serde(default = "default_min_changed_files")]
    pub min_changed_files: usize,
    #[serde(default)]
    pub require_passing_test: bool,
    #[serde(default = "default_curate_after_captures")]
    pub curate_after_captures: usize,
    #[serde(default = "default_curate_on_explicit_flush")]
    pub curate_on_explicit_flush: bool,
    #[serde(default)]
    pub ignored_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audit_log_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_file_path: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RetentionConfig {
    /// Delete a canonical memory entirely (all its versions) when its
    /// latest version is a tombstone older than this duration. Default
    /// None means "never prune tombstones".
    #[serde(default, with = "humantime_serde::option")]
    pub tombstone_after: Option<Duration>,
    /// Delete superseded (non-latest, non-tombstone) versions whose
    /// `updated_at` is older than this duration. The latest version of
    /// each canonical memory is always kept. Default None means "never
    /// prune superseded versions".
    #[serde(default, with = "humantime_serde::option")]
    pub superseded_after: Option<Duration>,
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

impl Default for AutomationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: AutomationMode::Suggest,
            repo_root: None,
            poll_interval: default_poll_interval(),
            file_events: default_file_events(),
            capture_idle_threshold: default_capture_idle_threshold(),
            min_changed_files: default_min_changed_files(),
            require_passing_test: false,
            curate_after_captures: default_curate_after_captures(),
            curate_on_explicit_flush: default_curate_on_explicit_flush(),
            ignored_paths: Vec::new(),
            audit_log_path: None,
            state_file_path: None,
        }
    }
}

fn default_bind_addr() -> String {
    "127.0.0.1:4040".to_string()
}

fn default_cluster_enabled() -> bool {
    true
}

fn default_cluster_discovery_multicast_addr() -> String {
    "239.255.42.99:4042".to_string()
}

fn default_cluster_announce_interval() -> Duration {
    Duration::from_secs(5)
}

fn default_file_events() -> bool {
    true
}

fn default_cluster_peer_ttl() -> Duration {
    Duration::from_secs(15)
}

fn default_cluster_priority() -> i32 {
    100
}

fn default_api_token() -> String {
    "dev-memory-token".to_string()
}

fn default_capnp_unix_socket() -> String {
    default_shared_capnp_unix_socket()
}

fn default_capnp_tcp_addr() -> String {
    "127.0.0.1:4041".to_string()
}

fn default_request_timeout() -> Duration {
    Duration::from_secs(30)
}

fn default_llm_provider() -> String {
    "openai_compatible".to_string()
}

fn default_llm_base_url() -> String {
    "https://api.openai.com/v1".to_string()
}

fn default_llm_api_key_env() -> String {
    "OPENAI_API_KEY".to_string()
}

fn default_embeddings_provider() -> String {
    "openai".to_string()
}

fn default_embeddings_base_url() -> String {
    "https://api.openai.com/v1".to_string()
}

fn default_embeddings_api_key_env() -> String {
    "OPENAI_API_KEY".to_string()
}

fn default_embeddings_batch_size() -> usize {
    16
}

fn default_llm_max_input_bytes() -> usize {
    120_000
}

fn default_llm_max_output_tokens() -> u32 {
    3_000
}

fn default_poll_interval() -> Duration {
    Duration::from_secs(60)
}

fn default_capture_idle_threshold() -> Duration {
    Duration::from_secs(600)
}

fn default_min_changed_files() -> usize {
    2
}

fn default_curate_after_captures() -> usize {
    3
}

fn default_curate_on_explicit_flush() -> bool {
    true
}

#[derive(Debug, Error)]
#[error("{message}")]
pub struct ValidationError {
    message: String,
}

pub async fn write_capnp_text_frame<W>(writer: &mut W, text: &str) -> Result<(), std::io::Error>
where
    W: AsyncWrite + Unpin,
{
    let payload = encode_capnp_text(text)?;
    writer.write_u32_le(payload.len() as u32).await?;
    writer.write_all(&payload).await?;
    writer.flush().await
}

pub async fn read_capnp_text_frame<R>(reader: &mut R) -> Result<Option<String>, std::io::Error>
where
    R: AsyncRead + Unpin,
{
    let len = match reader.read_u32_le().await {
        Ok(len) => len,
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut buf = vec![0_u8; len as usize];
    reader.read_exact(&mut buf).await?;
    decode_capnp_text(&buf).map(Some)
}

pub fn encode_capnp_text(text: &str) -> Result<Vec<u8>, std::io::Error> {
    let mut message = capnp::message::Builder::new_default();
    message
        .set_root::<capnp::text::Owned>(text)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))?;
    let mut bytes = Vec::new();
    serialize::write_message(&mut bytes, &message)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))?;
    Ok(bytes)
}

pub fn decode_capnp_text(bytes: &[u8]) -> Result<String, std::io::Error> {
    let mut cursor = Cursor::new(bytes);
    let reader = serialize::read_message(&mut cursor, ReaderOptions::new())
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))?;
    let text = reader
        .get_root::<capnp::text::Reader<'_>>()
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))?;
    text.to_string()
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))
}

impl ValidationError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        sync::{Mutex, OnceLock},
    };

    use super::*;

    #[test]
    fn memory_type_task_displays_as_snake_case() {
        assert_eq!(MemoryType::Task.to_string(), "task");
    }

    #[test]
    fn ollama_llm_uses_local_default_and_no_inherited_openai_key() {
        let config = LlmConfig {
            provider: OLLAMA_PROVIDER.to_string(),
            base_url: OPENAI_COMPATIBLE_BASE_URL.to_string(),
            api_key_env: "OPENAI_API_KEY".to_string(),
            model: "llama3.2".to_string(),
            ..LlmConfig::default()
        };
        assert_eq!(effective_llm_base_url(&config), OLLAMA_BASE_URL);
        assert_eq!(llm_max_output_tokens_field(&config.provider), "max_tokens");
        assert!(!llm_requires_api_key(&config));
        assert!(resolve_llm_api_key(&config).is_none());
        let empty_key_config = LlmConfig {
            api_key_env: String::new(),
            ..config
        };
        assert!(!llm_requires_api_key(&empty_key_config));
    }

    #[test]
    fn openai_compatible_llm_keeps_existing_defaults() {
        let config = LlmConfig::default();
        assert_eq!(effective_llm_base_url(&config), OPENAI_COMPATIBLE_BASE_URL);
        assert_eq!(
            llm_max_output_tokens_field(&config.provider),
            "max_completion_tokens"
        );
        assert!(llm_requires_api_key(&config));
    }

    #[test]
    fn profile_display_version_adds_dev_suffix_only_in_dev() {
        assert_eq!(Profile::Prod.version_suffix(), "");
        assert_eq!(Profile::Dev.version_suffix(), "-dev");
        assert_eq!(Profile::Prod.display_version("0.6.0"), "0.6.0");
        assert_eq!(Profile::Dev.display_version("0.6.0"), "0.6.0-dev");
    }

    fn parse_embeddings(input: &str) -> EmbeddingsConfig {
        #[derive(Deserialize)]
        struct Wrap {
            #[serde(default)]
            embeddings: EmbeddingsConfig,
        }
        let wrap: Wrap = toml::from_str(input).expect("parse embeddings TOML");
        wrap.embeddings
    }

    #[test]
    fn embeddings_config_legacy_singleton_deserializes_and_auto_names() {
        let mut cfg = parse_embeddings(
            r#"
            [embeddings]
            provider = "voyage"
            model = "voyage-code-3"
            api_key_env = "VOYAGE_API_KEY"
            "#,
        );
        cfg.normalize_backend_names();
        assert_eq!(cfg.backends.len(), 1);
        let only = &cfg.backends[0];
        assert_eq!(only.provider, "voyage");
        assert_eq!(only.model, "voyage-code-3");
        assert_eq!(only.name, "voyage-voyage-code-3");
        assert_eq!(cfg.active.as_deref(), Some("voyage-voyage-code-3"));
    }

    #[test]
    fn embeddings_config_new_form_with_multiple_backends() {
        let mut cfg = parse_embeddings(
            r#"
            [embeddings]
            active = "voyage-code"

            [[embeddings.backends]]
            name = "openai-3-small"
            provider = "openai"
            model = "text-embedding-3-small"
            api_key_env = "OPENAI_API_KEY"
            dimensions = 512

            [[embeddings.backends]]
            name = "voyage-code"
            provider = "voyage"
            model = "voyage-code-3"
            api_key_env = "VOYAGE_API_KEY"
            "#,
        );
        cfg.normalize_backend_names();
        assert_eq!(cfg.backends.len(), 2);
        assert!(cfg.enabled);
        assert!(cfg.create_enabled);
        assert_eq!(cfg.active.as_deref(), Some("voyage-code"));
        assert_eq!(cfg.backend("openai-3-small").unwrap().provider, "openai");
        assert_eq!(cfg.backend("openai-3-small").unwrap().dimensions, Some(512));
        assert_eq!(cfg.active_backend().unwrap().model, "voyage-code-3");
    }

    #[test]
    fn embeddings_config_create_enabled_false_keeps_search_enabled() {
        let mut cfg = parse_embeddings(
            r#"
            [embeddings]
            create_enabled = false
            active = "openai"

            [[embeddings.backends]]
            name = "openai"
            provider = "openai"
            model = "text-embedding-3-small"
            create_enabled = false
            "#,
        );
        cfg.normalize_backend_names();

        assert!(cfg.enabled);
        assert!(!cfg.create_enabled);
        assert!(!cfg.backend("openai").unwrap().create_enabled);
        assert_eq!(cfg.active_backend().unwrap().name, "openai");
    }

    #[test]
    fn embeddings_config_enabled_false_disables_active_backend() {
        let mut cfg = parse_embeddings(
            r#"
            [embeddings]
            enabled = false
            active = "openai"

            [[embeddings.backends]]
            name = "openai"
            provider = "openai"
            model = "text-embedding-3-small"
            "#,
        );
        cfg.normalize_backend_names();

        assert!(!cfg.enabled);
        assert_eq!(cfg.active.as_deref(), Some("openai"));
        assert!(cfg.active_backend().is_none());
    }

    #[test]
    fn embeddings_config_duplicate_names_get_unique_suffixes() {
        let mut cfg = parse_embeddings(
            r#"
            [[embeddings.backends]]
            name = "shared"
            provider = "openai_compatible"
            model = "a"

            [[embeddings.backends]]
            name = "shared"
            provider = "voyage"
            model = "b"
            "#,
        );
        cfg.normalize_backend_names();
        let names: Vec<_> = cfg.backends.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, vec!["shared", "shared-2"]);
    }

    #[test]
    fn embeddings_config_unknown_active_falls_back_to_sole_backend() {
        let mut cfg = parse_embeddings(
            r#"
            [embeddings]
            active = "does-not-exist"

            [[embeddings.backends]]
            name = "openai"
            provider = "openai"
            model = "text-embedding-3-small"
            "#,
        );
        cfg.normalize_backend_names();
        // With exactly one backend configured, an unknown `active`
        // collapses onto that backend rather than leaving search
        // silently disabled.
        assert_eq!(cfg.active.as_deref(), Some("openai"));
        assert_eq!(
            cfg.active_backend().unwrap().model,
            "text-embedding-3-small"
        );
    }

    #[test]
    fn embeddings_config_unknown_active_with_multiple_backends_clears_active() {
        let mut cfg = parse_embeddings(
            r#"
            [embeddings]
            active = "does-not-exist"

            [[embeddings.backends]]
            name = "a"
            provider = "openai"
            model = "m1"

            [[embeddings.backends]]
            name = "b"
            provider = "voyage"
            model = "m2"
            "#,
        );
        cfg.normalize_backend_names();
        assert_eq!(cfg.active, None);
        assert!(cfg.active_backend().is_none());
    }

    #[test]
    fn embeddings_config_empty_table_produces_no_backends() {
        let cfg = parse_embeddings("[embeddings]\n");
        assert!(cfg.backends.is_empty());
        assert!(cfg.active.is_none());
    }

    #[test]
    fn prune_history_rejects_missing_thresholds() {
        let request = PruneHistoryRequest::default();
        let err = request
            .validate()
            .expect_err("missing thresholds must fail");
        let message = format!("{err}");
        assert!(
            message.contains("no retention threshold configured"),
            "unexpected message: {message}"
        );
    }

    #[test]
    fn prune_history_accepts_either_threshold_alone() {
        let req = PruneHistoryRequest {
            tombstone_after: Some(Duration::from_secs(86_400)),
            ..PruneHistoryRequest::default()
        };
        assert!(req.validate().is_ok());

        let req = PruneHistoryRequest {
            superseded_after: Some(Duration::from_secs(3_600)),
            ..PruneHistoryRequest::default()
        };
        assert!(req.validate().is_ok());
    }

    #[test]
    fn prune_history_rejects_empty_project_override() {
        let req = PruneHistoryRequest {
            project: Some(String::new()),
            tombstone_after: Some(Duration::from_secs(10)),
            ..PruneHistoryRequest::default()
        };
        assert!(req.validate().is_err());
    }

    #[test]
    fn query_request_rejects_empty_query() {
        let request = QueryRequest {
            project: "memory".to_string(),
            query: String::new(),
            filters: QueryFilters::default(),
            top_k: 8,
            min_confidence: None,
            history: false,
            retrieval_mode: None,
            answer_mode: None,
        };

        assert!(request.validate().is_err());
    }

    #[test]
    fn query_response_defaults_answer_metadata_for_older_json() {
        let payload = serde_json::json!({
            "answer": "Stored answer",
            "confidence": 0.7,
            "results": [],
            "insufficient_evidence": false
        });

        let response: QueryResponse =
            serde_json::from_value(payload).expect("query response should deserialize");

        assert_eq!(response.answer, "Stored answer");
        assert_eq!(
            response.answer_generation.method,
            QueryAnswerMethod::Deterministic
        );
        assert!(response.answer_generation.cited_result_numbers.is_empty());
        assert!(response.answer_citations.is_empty());
    }

    #[test]
    fn query_activity_details_defaults_graph_metadata_for_older_json() {
        let payload = serde_json::json!({
            "type": "query",
            "query": "How does query activity work?",
            "top_k": 8,
            "result_count": 2,
            "confidence": 0.7,
            "insufficient_evidence": false,
            "total_duration_ms": 42,
            "answer": "Stored answer"
        });

        let details: ActivityDetails =
            serde_json::from_value(payload).expect("query activity details should deserialize");

        match details {
            ActivityDetails::Query {
                graph_status,
                graph_candidates,
                graph_augmented_candidates,
                graph_duration_ms,
                graph_result_count,
                graph_connection_count,
                graph_connections,
                ..
            } => {
                assert_eq!(graph_status, None);
                assert_eq!(graph_candidates, 0);
                assert_eq!(graph_augmented_candidates, 0);
                assert_eq!(graph_duration_ms, 0);
                assert_eq!(graph_result_count, 0);
                assert_eq!(graph_connection_count, 0);
                assert!(graph_connections.is_empty());
            }
            other => panic!("unexpected activity details: {other:?}"),
        }
    }

    #[test]
    fn capture_task_requires_project() {
        let request = CaptureTaskRequest {
            project: String::new(),
            task_title: "task".to_string(),
            user_prompt: "prompt".to_string(),
            writer_id: "codex-writer".to_string(),
            writer_name: Some("Codex".to_string()),
            agent_summary: "summary".to_string(),
            files_changed: Vec::new(),
            git_diff_summary: None,
            tests: Vec::new(),
            notes: Vec::new(),
            structured_candidates: Vec::new(),
            command_output: None,
            idempotency_key: None,
            dry_run: false,
        };

        assert!(request.validate().is_err());
    }

    #[test]
    fn commit_sync_request_requires_commits() {
        let request = CommitSyncRequest {
            project: "memory".to_string(),
            repo_root: "/repo".to_string(),
            commits: Vec::new(),
            dry_run: false,
        };

        assert!(request.validate().is_err());
    }

    #[test]
    fn plan_activity_request_requires_valid_counts() {
        let request = PlanActivityRequest {
            project: "memory".to_string(),
            action: PlanActivityAction::FinishBlocked,
            title: "Plan".to_string(),
            thread_key: "thread".to_string(),
            total_items: 1,
            completed_items: 2,
            remaining_items: vec!["left".to_string()],
            source_path: None,
        };

        assert!(request.validate().is_err());
    }

    #[test]
    fn plan_activity_request_requires_thread_key() {
        let request = PlanActivityRequest {
            project: "memory".to_string(),
            action: PlanActivityAction::Started,
            title: "Plan".to_string(),
            thread_key: String::new(),
            total_items: 1,
            completed_items: 0,
            remaining_items: vec!["left".to_string()],
            source_path: None,
        };

        assert!(request.validate().is_err());
    }

    #[test]
    fn graph_activity_request_requires_persisted_run_for_non_dry_run() {
        let request = GraphActivityRequest {
            project: "memory".to_string(),
            repo_root: "/repo".to_string(),
            git_head: Some("abc123".to_string()),
            since: None,
            extraction_run_id: None,
            dry_run: false,
            reused_existing_run: false,
            index_reused: true,
            analyzer_version: "mem-analyze-v2".to_string(),
            strategy_version: "code-graph-resolution-v1".to_string(),
            symbol_count: 1,
            reference_count: 2,
            resolved_reference_count: 1,
            unresolved_reference_count: 1,
            ambiguous_reference_count: 0,
            graph_node_count: 1,
            graph_edge_count: 1,
            evidence_count: 2,
        };

        assert!(request.validate().is_err());
    }

    #[test]
    fn graph_extract_activity_details_roundtrip() {
        let run_id = Uuid::new_v4();
        let details = ActivityDetails::GraphExtract {
            repo_root: "/repo".to_string(),
            git_head: Some("abc123".to_string()),
            since: Some("HEAD~1".to_string()),
            extraction_run_id: Some(run_id),
            dry_run: false,
            reused_existing_run: false,
            index_reused: true,
            analyzer_version: "mem-analyze-v2".to_string(),
            strategy_version: "code-graph-resolution-v1".to_string(),
            symbol_count: 10,
            reference_count: 20,
            resolved_reference_count: 12,
            unresolved_reference_count: 7,
            ambiguous_reference_count: 1,
            graph_node_count: 10,
            graph_edge_count: 9,
            evidence_count: 19,
        };

        let encoded = serde_json::to_value(&details).expect("serialize details");
        let decoded: ActivityDetails =
            serde_json::from_value(encoded).expect("deserialize details");

        match decoded {
            ActivityDetails::GraphExtract {
                extraction_run_id,
                symbol_count,
                graph_edge_count,
                ..
            } => {
                assert_eq!(extraction_run_id, Some(run_id));
                assert_eq!(symbol_count, 10);
                assert_eq!(graph_edge_count, 9);
            }
            other => panic!("unexpected activity details: {other:?}"),
        }
    }

    #[test]
    fn finds_repo_local_mem_config() {
        let temp_dir = unique_temp_dir("mem-api-config");
        let mem_dir = temp_dir.join(".mem");
        fs::create_dir_all(&mem_dir).unwrap();
        let config_path = mem_dir.join("config.toml");
        fs::write(&config_path, "test = true\n").unwrap();

        let nested = temp_dir.join("nested").join("deeper");
        fs::create_dir_all(&nested).unwrap();

        let discovered = find_repo_config_path(&nested).unwrap();
        assert_eq!(discovered, config_path);

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn prefers_xdg_global_config_path_when_present() {
        let _guard = env_lock().lock().unwrap();
        let temp_dir = unique_temp_dir("mem-api-global");
        let config_home = temp_dir.join("config-home");
        fs::create_dir_all(config_home.join("memory-layer")).unwrap();
        let global_path = config_home.join("memory-layer").join("memory-layer.toml");
        fs::write(&global_path, "test = true\n").unwrap();

        unsafe {
            env::set_var("XDG_CONFIG_HOME", &config_home);
        }
        let discovered = discover_global_config_path().unwrap();
        unsafe {
            env::remove_var("XDG_CONFIG_HOME");
        }

        assert_eq!(discovered, global_path);
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn repo_config_is_found_from_nested_directory() {
        let temp_dir = unique_temp_dir("mem-api-repo");
        let mem_dir = temp_dir.join(".mem");
        fs::create_dir_all(&mem_dir).unwrap();
        let config_path = mem_dir.join("config.toml");
        fs::write(&config_path, "[automation]\nenabled = false\n").unwrap();

        let nested = temp_dir.join("nested").join("src");
        fs::create_dir_all(&nested).unwrap();

        assert_eq!(find_repo_config_path(&nested).unwrap(), config_path);
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn loads_repo_agent_settings_from_agents_directory() {
        let temp_dir = unique_temp_dir("mem-api-agent-settings");
        let agents_dir = temp_dir.join(".agents");
        fs::create_dir_all(&agents_dir).unwrap();
        fs::write(
            agents_dir.join("memory-layer.toml"),
            "[capture]\ninclude_paths = [\"ops/\"]\nignore_paths = [\"tmp/\"]\n\n[analysis]\nanalyzers = [\"rust\"]\n\n[retrieval]\ngraph_enabled = true\n",
        )
        .unwrap();

        let settings = load_repo_agent_settings(&temp_dir).unwrap();

        assert_eq!(settings.capture.include_paths, vec!["ops/"]);
        assert_eq!(settings.analysis.analyzers, vec!["rust"]);
        assert!(settings.retrieval.graph_enabled);
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn prefers_macos_application_support_global_config_path_when_present() {
        let _guard = env_lock().lock().unwrap();
        let temp_dir = unique_temp_dir("mem-api-macos-global");
        let home = temp_dir.join("home");
        let app_support = home
            .join("Library")
            .join("Application Support")
            .join("memory-layer");
        fs::create_dir_all(&app_support).unwrap();
        let global_path = app_support.join("memory-layer.toml");
        fs::write(&global_path, "test = true\n").unwrap();
        let original_home = env::var("HOME").ok();

        unsafe {
            env::remove_var("XDG_CONFIG_HOME");
            env::set_var("HOME", &home);
        }
        let discovered = discover_global_config_path().unwrap();
        unsafe {
            if let Some(value) = original_home {
                env::set_var("HOME", value);
            } else {
                env::remove_var("HOME");
            }
        }

        assert_eq!(discovered, global_path);
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn shared_env_file_overrides_config_for_explicit_path() {
        let _guard = env_lock().lock().unwrap();
        let temp_dir = unique_temp_dir("mem-api-shared-env");
        let config_dir = temp_dir.join("config");
        fs::create_dir_all(&config_dir).unwrap();
        let config_path = config_dir.join("memory-layer.toml");
        fs::write(
            &config_path,
            "[service]\nbind_addr = \"127.0.0.1:4040\"\ncapnp_unix_socket = \"/tmp/a.sock\"\ncapnp_tcp_addr = \"127.0.0.1:4041\"\napi_token = \"from-config\"\nrequest_timeout = \"30s\"\n\n[database]\nurl = \"postgresql://config\"\n",
        )
        .unwrap();
        fs::write(
            config_dir.join("memory-layer.env"),
            "MEMORY_LAYER__DATABASE__URL=postgresql://from-env\nMEMORY_LAYER__SERVICE__API_TOKEN=from-env\nOPENAI_API_KEY=test\n",
        )
        .unwrap();

        unsafe {
            env::remove_var("MEMORY_LAYER__DATABASE__URL");
            env::remove_var("MEMORY_LAYER__SERVICE__API_TOKEN");
        }
        let config = AppConfig::load_with_profile(Some(config_path), Profile::Prod).unwrap();
        unsafe {
            env::remove_var("MEMORY_LAYER__DATABASE__URL");
            env::remove_var("MEMORY_LAYER__SERVICE__API_TOKEN");
        }

        assert_eq!(config.database.url, "postgresql://from-env");
        assert_eq!(config.service.api_token, "from-env");
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn process_env_still_wins_over_env_file_for_explicit_path() {
        let _guard = env_lock().lock().unwrap();
        let temp_dir = unique_temp_dir("mem-api-env-precedence");
        let config_dir = temp_dir.join("config");
        fs::create_dir_all(&config_dir).unwrap();
        let config_path = config_dir.join("memory-layer.toml");
        fs::write(
            &config_path,
            "[service]\nbind_addr = \"127.0.0.1:4040\"\ncapnp_unix_socket = \"/tmp/a.sock\"\ncapnp_tcp_addr = \"127.0.0.1:4041\"\napi_token = \"from-config\"\nrequest_timeout = \"30s\"\n\n[database]\nurl = \"postgresql://config\"\n",
        )
        .unwrap();
        fs::write(
            config_dir.join("memory-layer.env"),
            "MEMORY_LAYER__DATABASE__URL=postgresql://from-env-file\n",
        )
        .unwrap();

        unsafe {
            env::remove_var("MEMORY_LAYER__DATABASE__URL");
            env::set_var(
                "MEMORY_LAYER__DATABASE__URL",
                "postgresql://from-process-env",
            );
        }
        let config = AppConfig::load_with_profile(Some(config_path), Profile::Prod).unwrap();
        unsafe {
            env::remove_var("MEMORY_LAYER__DATABASE__URL");
        }

        assert_eq!(config.database.url, "postgresql://from-process-env");
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn read_repo_project_slug_uses_project_metadata() {
        let repo_root = unique_temp_dir("mem-api-project-slug");
        fs::create_dir_all(repo_root.join(".mem")).unwrap();
        fs::write(
            repo_root.join(".mem").join("project.toml"),
            "slug = \"sctp\"\nrepo_root = \"/tmp/sctp-conformance\"\n",
        )
        .unwrap();

        assert_eq!(read_repo_project_slug(&repo_root).as_deref(), Some("sctp"));

        let _ = fs::remove_dir_all(repo_root);
    }

    #[test]
    fn legacy_and_new_capture_threshold_keys_can_be_merged() {
        let _guard = env_lock().lock().unwrap();
        let temp_dir = unique_temp_dir("mem-api-threshold-merge");
        let config_home = temp_dir.join("config-home");
        let repo_dir = temp_dir.join("repo");
        let global_dir = config_home.join("memory-layer");
        let mem_dir = repo_dir.join(".mem");
        fs::create_dir_all(&global_dir).unwrap();
        fs::create_dir_all(&mem_dir).unwrap();

        fs::write(
            global_dir.join("memory-layer.toml"),
            "[service]\nbind_addr = \"127.0.0.1:4040\"\ncapnp_unix_socket = \"/tmp/a.sock\"\ncapnp_tcp_addr = \"127.0.0.1:4041\"\napi_token = \"from-config\"\nrequest_timeout = \"30s\"\n\n[database]\nurl = \"postgresql://config\"\n\n[automation]\nidle_threshold = \"5m\"\n",
        )
        .unwrap();
        fs::write(
            mem_dir.join("config.toml"),
            "[automation]\ncapture_idle_threshold = \"10m\"\n",
        )
        .unwrap();

        let original_dir = env::current_dir().unwrap();
        unsafe {
            env::set_var("XDG_CONFIG_HOME", &config_home);
        }
        env::set_current_dir(&repo_dir).unwrap();
        let config = AppConfig::load_with_profile(None, Profile::Prod).unwrap();
        env::set_current_dir(original_dir).unwrap();
        unsafe {
            env::remove_var("XDG_CONFIG_HOME");
        }

        assert_eq!(
            config.automation.capture_idle_threshold,
            Duration::from_secs(600)
        );
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn dev_profile_overlays_config_dev_toml_on_top_of_base() {
        let temp_dir = unique_temp_dir("mem-api-dev-overlay");
        let mem_dir = temp_dir.join(".mem");
        fs::create_dir_all(&mem_dir).unwrap();
        fs::write(
            mem_dir.join("config.toml"),
            "[service]\nbind_addr = \"10.0.0.1:4150\"\ncapnp_unix_socket = \"/tmp/prod.sock\"\ncapnp_tcp_addr = \"10.0.0.1:4151\"\napi_token = \"t\"\nrequest_timeout = \"30s\"\n\n[database]\nurl = \"postgresql://shared\"\n",
        )
        .unwrap();
        fs::write(
            mem_dir.join("config.dev.toml"),
            "[service]\nbind_addr = \"127.0.0.1:4250\"\n",
        )
        .unwrap();

        let config =
            AppConfig::load_with_profile(Some(mem_dir.join("config.toml")), Profile::Dev).unwrap();

        assert_eq!(config.profile, Profile::Dev);
        assert_eq!(config.service.bind_addr, "127.0.0.1:4250");
        assert_eq!(config.database.url, "postgresql://shared");
        assert_eq!(
            config.resolved_dev_overlay_path.as_deref(),
            Some(mem_dir.join("config.dev.toml").as_path())
        );
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn dev_profile_errors_when_overlay_is_missing() {
        let temp_dir = unique_temp_dir("mem-api-dev-overlay-missing");
        let mem_dir = temp_dir.join(".mem");
        fs::create_dir_all(&mem_dir).unwrap();
        fs::write(
            mem_dir.join("config.toml"),
            "[service]\nbind_addr = \"10.0.0.1:4150\"\ncapnp_unix_socket = \"/tmp/p.sock\"\ncapnp_tcp_addr = \"10.0.0.1:4151\"\napi_token = \"t\"\nrequest_timeout = \"30s\"\n\n[database]\nurl = \"postgresql://shared\"\n",
        )
        .unwrap();

        let err = AppConfig::load_with_profile(Some(mem_dir.join("config.toml")), Profile::Dev)
            .unwrap_err();
        let message = format!("{err}");
        assert!(message.contains("config.dev.toml"), "message: {message}");
        let _ = fs::remove_dir_all(temp_dir);
    }

    fn unique_temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        if path.exists() {
            let _ = fs::remove_dir_all(&path);
        }
        path
    }

    fn env_lock() -> &'static Mutex<()> {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }
}
