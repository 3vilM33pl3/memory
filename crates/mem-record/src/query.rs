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
    /// When true, bypass provenance-based stale-source de-ranking.
    /// Stale memories are included either way; this flag preserves their
    /// pre-provenance score for audit/debug cases.
    #[serde(default)]
    pub include_stale: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalQueryRequest {
    pub query: String,
    #[serde(default)]
    pub filters: QueryFilters,
    #[serde(default = "default_top_k")]
    pub top_k: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_confidence: Option<f32>,
    #[serde(default)]
    pub include_stale: bool,
    #[serde(default)]
    pub history: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retrieval_mode: Option<QueryRetrievalMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer_mode: Option<QueryAnswerMode>,
}

impl GlobalQueryRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
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

    pub fn to_prompt_query_request(&self) -> QueryRequest {
        QueryRequest {
            project: "all-projects".to_string(),
            query: self.query.clone(),
            filters: self.filters.clone(),
            top_k: self.top_k,
            min_confidence: self.min_confidence,
            include_stale: self.include_stale,
            history: self.history,
            retrieval_mode: self.retrieval_mode,
            answer_mode: self.answer_mode,
        }
    }
}

pub fn default_top_k() -> i64 {
    8
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuerySource {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_kind: Option<String>,
    pub source_kind: SourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<SourceProvenanceRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub memory_id: Uuid,
    /// Stable identity of the memory across versions; `memory_id` is the
    /// exact version row and may be pruned by retention.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_no: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<String>,
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
    /// Set when the reinforcement validation pipeline flagged this memory
    /// for human review (weak or contradictory evidence). Ranked with a
    /// penalty rather than excluded; consumers should caveat it.
    #[serde(default)]
    pub needs_review: bool,
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
    /// Overlap over content-bearing query terms only (stopwords removed).
    /// Natural-language questions are mostly stopwords, so this is the
    /// anchor signal `term_overlap` understates.
    #[serde(default)]
    pub content_term_overlap: f64,
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
    pub provenance_decayed_candidates: usize,
    #[serde(default)]
    pub provenance_unverified_candidates: usize,
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
    #[serde(default)]
    pub provenance_warnings: Vec<DiagnosticInfo>,
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
    /// Stable identity: still resolvable after retention prunes the cited
    /// version row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_no: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<String>,
    pub memory_type: MemoryType,
    pub summary: String,
    pub snippet: String,
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
