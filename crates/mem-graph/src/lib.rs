use std::{future::Future, pin::Pin};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use mem_analyze::{ResolutionStatus, ResolvedAnalysisReport, resolve_analysis};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

mod repository;
pub use repository::PostgresGraphRepository;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphExtractionRequest {
    pub project: String,
    pub repo_root: String,
    pub git_head: Option<String>,
    pub since: Option<String>,
    pub force: bool,
    pub dry_run: bool,
    pub index_reused: bool,
    pub analysis: mem_analyze::AnalysisReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphExtractionReport {
    pub project: String,
    pub repo_root: String,
    pub git_head: Option<String>,
    pub since: Option<String>,
    pub analyzer_version: String,
    pub strategy_version: String,
    pub extraction_run_id: Option<Uuid>,
    pub reused_existing_run: bool,
    pub dry_run: bool,
    pub index_reused: bool,
    pub symbol_count: usize,
    pub reference_count: usize,
    pub resolved_reference_count: usize,
    pub unresolved_reference_count: usize,
    pub ambiguous_reference_count: usize,
    pub graph_node_count: usize,
    pub graph_edge_count: usize,
    pub evidence_count: usize,
    pub sample_unresolved_references: Vec<GraphReferencePreview>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphReferencePreview {
    pub kind: String,
    pub file_path: String,
    pub target_text: String,
    pub resolution_status: String,
    pub start_line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphStatusReport {
    pub project: String,
    pub repo_root: String,
    pub git_head: Option<String>,
    pub since: Option<String>,
    pub analyzer_version: String,
    pub strategy_version: String,
    pub extraction_run_id: Uuid,
    pub status: String,
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

pub fn build_extraction_preview(request: &GraphExtractionRequest) -> GraphExtractionReport {
    let resolved = resolve_analysis(&request.analysis);
    report_from_resolved(request, &resolved, None, false)
}

pub trait GraphRepository {
    fn extract(
        &self,
        request: GraphExtractionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<GraphExtractionReport>> + Send + '_>>;

    fn latest_status<'a>(
        &'a self,
        project: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<GraphStatusReport>>> + Send + 'a>>;
}

pub async fn run_migrations(pool: &PgPool) -> Result<()> {
    sqlx::migrate!("../../migrations")
        .run(pool)
        .await
        .context("run graph migrations")
}

fn report_from_resolved(
    request: &GraphExtractionRequest,
    resolved: &ResolvedAnalysisReport,
    run_id: Option<Uuid>,
    reused_existing_run: bool,
) -> GraphExtractionReport {
    let resolved_reference_count = resolved
        .references
        .iter()
        .filter(|reference| reference.resolution_status == ResolutionStatus::Resolved)
        .count();
    let unresolved_reference_count = resolved
        .references
        .iter()
        .filter(|reference| reference.resolution_status == ResolutionStatus::Unresolved)
        .count();
    let ambiguous_reference_count = resolved
        .references
        .iter()
        .filter(|reference| reference.resolution_status == ResolutionStatus::Ambiguous)
        .count();
    let graph_edge_count = resolved
        .references
        .iter()
        .filter(|reference| {
            reference.resolution_status == ResolutionStatus::Resolved
                && reference.source_symbol_identity.is_some()
                && reference.target_symbol_identity.is_some()
        })
        .count();
    GraphExtractionReport {
        project: request.project.clone(),
        repo_root: request.repo_root.clone(),
        git_head: request.git_head.clone(),
        since: request.since.clone(),
        analyzer_version: resolved.analyzer_version.clone(),
        strategy_version: resolved.resolution_strategy_version.clone(),
        extraction_run_id: run_id,
        reused_existing_run,
        dry_run: request.dry_run,
        index_reused: request.index_reused,
        symbol_count: resolved.symbols.len(),
        reference_count: resolved.references.len(),
        resolved_reference_count,
        unresolved_reference_count,
        ambiguous_reference_count,
        graph_node_count: resolved.symbols.len(),
        graph_edge_count,
        evidence_count: resolved.symbols.len() + graph_edge_count,
        sample_unresolved_references: unresolved_previews(resolved),
    }
}

fn unresolved_previews(resolved: &ResolvedAnalysisReport) -> Vec<GraphReferencePreview> {
    resolved
        .references
        .iter()
        .filter(|reference| reference.resolution_status != ResolutionStatus::Resolved)
        .take(10)
        .map(|reference| GraphReferencePreview {
            kind: reference.graph_edge_kind.clone(),
            file_path: reference.file_path.clone(),
            target_text: reference.target_text.clone(),
            resolution_status: reference.resolution_status.as_str().to_string(),
            start_line: reference.span.start_line,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dry_run_preview_reports_resolution_counts() {
        let report = mem_analyze::AnalysisReport {
            analyzer_version: mem_analyze::ANALYZER_VERSION.to_string(),
            symbols: vec![mem_analyze::SymbolFact {
                id: "helper".to_string(),
                stable_identity: "rust:src/lib.rs:function:helper:1-1".to_string(),
                language: mem_analyze::AnalyzerLanguage::Rust,
                file_path: "src/lib.rs".to_string(),
                kind: mem_analyze::SymbolKind::Function,
                name: "helper".to_string(),
                qualified_name: Some("helper".to_string()),
                span: mem_analyze::Span {
                    start_byte: 0,
                    end_byte: 1,
                    start_line: 1,
                    end_line: 1,
                },
                display: "helper".to_string(),
                source_hash: None,
            }],
            calls: vec![mem_analyze::CallFact {
                id: "call".to_string(),
                language: mem_analyze::AnalyzerLanguage::Rust,
                file_path: "src/lib.rs".to_string(),
                callee_text: "helper".to_string(),
                caller_symbol: None,
                span: mem_analyze::Span {
                    start_byte: 2,
                    end_byte: 3,
                    start_line: 2,
                    end_line: 2,
                },
            }],
            ..mem_analyze::AnalysisReport::default()
        };
        let request = GraphExtractionRequest {
            project: "memory".to_string(),
            repo_root: "/repo".to_string(),
            git_head: Some("abc".to_string()),
            since: None,
            force: false,
            dry_run: true,
            index_reused: false,
            analysis: report,
        };
        let preview = build_extraction_preview(&request);
        assert_eq!(preview.symbol_count, 1);
        assert_eq!(preview.reference_count, 1);
        assert_eq!(preview.resolved_reference_count, 1);
        assert_eq!(preview.graph_edge_count, 0);
        assert_eq!(
            preview.strategy_version,
            mem_analyze::RESOLUTION_STRATEGY_VERSION
        );
    }
}
