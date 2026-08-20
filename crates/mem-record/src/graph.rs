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

pub const CODE_GRAPH_DEFAULT_DEPTH: u8 = 1;

pub const CODE_GRAPH_MAX_DEPTH: u8 = 2;

pub const CODE_GRAPH_DEFAULT_NODE_LIMIT: usize = 250;

pub const CODE_GRAPH_MAX_NODE_LIMIT: usize = 1000;

pub const CODE_GRAPH_DEFAULT_EDGE_LIMIT: usize = 500;

pub const CODE_GRAPH_MAX_EDGE_LIMIT: usize = 2000;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CodeGraphViewRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_nodes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_edges: Option<usize>,
}

impl CodeGraphViewRequest {
    pub fn normalize(&self) -> CodeGraphViewFilters {
        CodeGraphViewFilters {
            run_id: self.run_id,
            q: normalize_optional_filter(self.q.as_deref()),
            file_path: normalize_optional_filter(self.file_path.as_deref()),
            symbol: normalize_optional_filter(self.symbol.as_deref()),
            edge_kind: normalize_optional_filter(self.edge_kind.as_deref()),
            depth: self
                .depth
                .unwrap_or(CODE_GRAPH_DEFAULT_DEPTH)
                .min(CODE_GRAPH_MAX_DEPTH),
            limit_nodes: self
                .limit_nodes
                .unwrap_or(CODE_GRAPH_DEFAULT_NODE_LIMIT)
                .clamp(1, CODE_GRAPH_MAX_NODE_LIMIT),
            limit_edges: self
                .limit_edges
                .unwrap_or(CODE_GRAPH_DEFAULT_EDGE_LIMIT)
                .clamp(1, CODE_GRAPH_MAX_EDGE_LIMIT),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CodeGraphViewFilters {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_kind: Option<String>,
    pub depth: u8,
    pub limit_nodes: usize,
    pub limit_edges: usize,
}

impl Default for CodeGraphViewFilters {
    fn default() -> Self {
        CodeGraphViewRequest::default().normalize()
    }
}

impl CodeGraphViewFilters {
    pub fn has_seed_filter(&self) -> bool {
        self.q.is_some() || self.file_path.is_some() || self.symbol.is_some()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGraphStatusResponse {
    pub project: String,
    pub has_graph: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analyzer_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    pub symbol_count: i64,
    pub reference_count: i64,
    pub resolved_reference_count: i64,
    pub unresolved_reference_count: i64,
    pub ambiguous_reference_count: i64,
    pub graph_node_count: i64,
    pub graph_edge_count: i64,
    pub evidence_count: i64,
}

impl CodeGraphStatusResponse {
    pub fn empty(project: impl Into<String>) -> Self {
        Self {
            project: project.into(),
            has_graph: false,
            latest_run_id: None,
            repo_root: None,
            git_head: None,
            since: None,
            analyzer_version: None,
            strategy_version: None,
            status: None,
            completed_at: None,
            symbol_count: 0,
            reference_count: 0,
            resolved_reference_count: 0,
            unresolved_reference_count: 0,
            ambiguous_reference_count: 0,
            graph_node_count: 0,
            graph_edge_count: 0,
            evidence_count: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGraphResponse {
    pub project: String,
    pub status: CodeGraphStatusResponse,
    pub filters: CodeGraphViewFilters,
    pub stats: CodeGraphStats,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncation_reason: Option<String>,
    #[serde(default)]
    pub nodes: Vec<CodeGraphNode>,
    #[serde(default)]
    pub edges: Vec<CodeGraphEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CodeGraphStats {
    pub total_nodes: i64,
    pub total_edges: i64,
    pub total_symbols: i64,
    pub total_references: i64,
    pub unresolved_references: i64,
    pub returned_nodes: usize,
    pub returned_edges: usize,
    pub seed_nodes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGraphNode {
    pub id: Uuid,
    pub stable_identity: String,
    pub label: String,
    pub node_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qualified_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_line: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<i64>,
    pub degree: i64,
    pub seed: bool,
    pub group: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGraphEdge {
    pub id: Uuid,
    pub source: Uuid,
    pub target: Uuid,
    pub edge_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_kind: Option<String>,
    pub confidence: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_line: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<i64>,
    pub resolution_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemoryGraphResponse {
    pub project: String,
    pub total_memories: i64,
    pub returned_memories: usize,
    #[serde(default)]
    pub nodes: Vec<ProjectMemoryGraphNode>,
    #[serde(default)]
    pub edges: Vec<ProjectMemoryGraphEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemoryGraphNode {
    pub id: String,
    pub label: String,
    pub node_kind: ProjectMemoryGraphNodeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_type: Option<MemoryType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<SourceKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub importance: Option<i32>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance_status: Option<SourceProvenanceStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Decayed ACT-R activation at read time (same decay math as ranking).
    /// Only set on memory nodes with a reinforcement score row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activation: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProjectMemoryGraphNodeKind {
    Memory,
    Source,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemoryGraphEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    pub edge_kind: ProjectMemoryGraphEdgeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relation_type: Option<MemoryRelationType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<SourceKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProjectMemoryGraphEdgeKind {
    Provenance,
    MemoryRelation,
}

pub fn normalize_optional_filter(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}
