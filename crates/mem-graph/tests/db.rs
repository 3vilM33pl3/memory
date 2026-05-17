use mem_analyze::{
    AnalysisReport, AnalyzerLanguage, AnalyzerSummary, CallFact, Span, SymbolFact, SymbolKind,
};
use mem_graph::{GraphExtractionRequest, PostgresGraphRepository};

#[tokio::test]
async fn graph_extract_persists_status_and_edges() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };
    let project = mem_test_support::unique_project_slug("graph-db");
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup old test project");

    let repo = PostgresGraphRepository::new(pool.clone());
    let report = repo
        .extract(GraphExtractionRequest {
            project: project.clone(),
            repo_root: "/tmp/memory-graph-test".to_string(),
            git_head: Some("abc123".to_string()),
            since: None,
            force: true,
            dry_run: false,
            index_reused: false,
            analysis: analysis_report(),
        })
        .await
        .expect("extract graph");

    assert_eq!(report.symbol_count, 2);
    assert_eq!(report.reference_count, 1);
    assert_eq!(report.resolved_reference_count, 1);
    assert_eq!(report.graph_node_count, 2);
    assert_eq!(report.graph_edge_count, 1);
    assert!(report.extraction_run_id.is_some());

    let status = repo
        .latest_status(&project)
        .await
        .expect("load graph status")
        .expect("stored graph status");
    assert_eq!(status.project, project);
    assert_eq!(status.symbol_count, 2);
    assert_eq!(status.reference_count, 1);
    assert_eq!(status.resolved_reference_count, 1);
    assert_eq!(status.graph_node_count, 2);
    assert_eq!(status.graph_edge_count, 1);

    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup test project");
}

fn analysis_report() -> AnalysisReport {
    AnalysisReport {
        enabled_analyzers: vec!["rust".to_string()],
        summaries: vec![AnalyzerSummary {
            analyzer: "rust".to_string(),
            files_seen: 1,
            files_parsed: 1,
            symbol_count: 2,
            call_count: 1,
            ..AnalyzerSummary::default()
        }],
        symbols: vec![
            SymbolFact {
                id: "symbol-run".to_string(),
                stable_identity: String::new(),
                language: AnalyzerLanguage::Rust,
                file_path: "src/lib.rs".to_string(),
                kind: SymbolKind::Function,
                name: "run".to_string(),
                qualified_name: Some("crate::run".to_string()),
                span: span(0, 12, 1, 3),
                display: "fn run()".to_string(),
                source_hash: Some("hash-run".to_string()),
            },
            SymbolFact {
                id: "symbol-helper".to_string(),
                stable_identity: String::new(),
                language: AnalyzerLanguage::Rust,
                file_path: "src/lib.rs".to_string(),
                kind: SymbolKind::Function,
                name: "helper".to_string(),
                qualified_name: Some("crate::helper".to_string()),
                span: span(14, 30, 5, 7),
                display: "fn helper()".to_string(),
                source_hash: Some("hash-helper".to_string()),
            },
        ],
        calls: vec![CallFact {
            id: "call-helper".to_string(),
            language: AnalyzerLanguage::Rust,
            file_path: "src/lib.rs".to_string(),
            callee_text: "helper".to_string(),
            caller_symbol: Some("run".to_string()),
            span: span(8, 14, 2, 2),
        }],
        ..AnalysisReport::default()
    }
}

fn span(start_byte: usize, end_byte: usize, start_line: usize, end_line: usize) -> Span {
    Span {
        start_byte,
        end_byte,
        start_line,
        end_line,
    }
}
