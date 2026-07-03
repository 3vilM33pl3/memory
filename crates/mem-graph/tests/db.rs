use mem_analyze::{
    AnalysisReport, AnalyzerLanguage, AnalyzerSummary, CallFact, Span, SymbolFact, SymbolKind,
};
use mem_api::CodeGraphViewRequest;
use mem_graph::{GraphExtractionRequest, PostgresGraphRepository};
use sqlx::PgPool;
use uuid::Uuid;

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

#[tokio::test]
async fn status_response_reports_empty_project_without_graph() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };

    let slug = mem_test_support::unique_project_slug("graph-empty");
    mem_test_support::cleanup_project(&pool, &slug)
        .await
        .expect("cleanup old project");

    let repository = PostgresGraphRepository::new(pool.clone());
    let status = repository
        .status_response(&slug)
        .await
        .expect("read status");

    assert_eq!(status.project, slug);
    assert!(!status.has_graph);
    assert_eq!(status.graph_node_count, 0);
}

#[tokio::test]
async fn visualization_graph_returns_seeded_neighborhood() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };

    let slug = mem_test_support::unique_project_slug("graph-view");
    let run_id = insert_graph_fixture(&pool, &slug).await;
    let repository = PostgresGraphRepository::new(pool.clone());

    let graph = repository
        .visualization_graph(
            &slug,
            CodeGraphViewRequest {
                symbol: Some("GraphHandler".to_string()),
                depth: Some(1),
                limit_nodes: Some(10),
                limit_edges: Some(10),
                ..CodeGraphViewRequest::default()
            }
            .normalize(),
        )
        .await
        .expect("read graph view");

    assert!(graph.status.has_graph);
    assert_eq!(graph.status.latest_run_id, Some(run_id));
    assert_eq!(graph.stats.seed_nodes, 1);
    assert_eq!(graph.nodes.len(), 3);
    assert_eq!(graph.edges.len(), 2);
    assert!(
        graph
            .nodes
            .iter()
            .any(|node| node.seed && node.label == "GraphHandler")
    );
    assert!(graph.edges.iter().any(
        |edge| edge.edge_kind == "calls" && edge.file_path.as_deref() == Some("src/handler.rs")
    ));

    mem_test_support::cleanup_project(&pool, &slug)
        .await
        .expect("cleanup project");
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

async fn insert_graph_fixture(pool: &PgPool, slug: &str) -> Uuid {
    mem_test_support::cleanup_project(pool, slug)
        .await
        .expect("cleanup old project");
    let project_id = Uuid::new_v4();
    let run_id = Uuid::new_v4();
    let handler_node = Uuid::new_v4();
    let repo_node = Uuid::new_v4();
    let ui_node = Uuid::new_v4();
    let calls_edge = Uuid::new_v4();
    let renders_edge = Uuid::new_v4();

    sqlx::query(
        "INSERT INTO projects (id, slug, name, root_path, created_at) VALUES ($1, $2, $2, '/repo', now())",
    )
    .bind(project_id)
    .bind(slug)
    .execute(pool)
    .await
    .expect("insert project");
    sqlx::query(
        r#"
        INSERT INTO graph_extraction_runs
            (id, project_id, repo_root, git_head, analyzer_version, strategy_version,
             status, started_at, completed_at, summary_json)
        VALUES ($1, $2, '/repo', 'abc', 'mem-analyze-v2', 'code-graph-resolution-v1',
                'completed', now(), now(), '{}'::jsonb)
        "#,
    )
    .bind(run_id)
    .bind(project_id)
    .execute(pool)
    .await
    .expect("insert graph run");

    insert_symbol(
        pool,
        project_id,
        run_id,
        handler_node,
        "rust:src/handler.rs:function:GraphHandler:10-20",
        "GraphHandler",
        "rust",
        "src/handler.rs",
        "function",
        10,
        20,
    )
    .await;
    insert_symbol(
        pool,
        project_id,
        run_id,
        repo_node,
        "rust:src/repository.rs:function:GraphRepository:30-40",
        "GraphRepository",
        "rust",
        "src/repository.rs",
        "function",
        30,
        40,
    )
    .await;
    insert_symbol(
        pool,
        project_id,
        run_id,
        ui_node,
        "ts:web/src/GraphTab.tsx:function:GraphTab:5-25",
        "GraphTab",
        "typescript",
        "web/src/GraphTab.tsx",
        "component",
        5,
        25,
    )
    .await;
    insert_edge(
        pool,
        project_id,
        run_id,
        GraphEdgeFixture {
            edge_id: calls_edge,
            source: handler_node,
            target: repo_node,
            edge_kind: "calls",
            file_path: "src/handler.rs",
            line: 15,
        },
    )
    .await;
    insert_edge(
        pool,
        project_id,
        run_id,
        GraphEdgeFixture {
            edge_id: renders_edge,
            source: handler_node,
            target: ui_node,
            edge_kind: "references",
            file_path: "src/handler.rs",
            line: 18,
        },
    )
    .await;
    insert_reference(pool, project_id, run_id, Some(calls_edge), "resolved").await;
    insert_reference(pool, project_id, run_id, Some(renders_edge), "resolved").await;
    insert_reference(pool, project_id, run_id, None, "unresolved").await;

    run_id
}

#[allow(clippy::too_many_arguments)]
async fn insert_symbol(
    pool: &PgPool,
    project_id: Uuid,
    run_id: Uuid,
    node_id: Uuid,
    stable_identity: &str,
    display_name: &str,
    language: &str,
    file_path: &str,
    symbol_kind: &str,
    start_line: i64,
    end_line: i64,
) {
    sqlx::query(
        r#"
        INSERT INTO graph_nodes
            (id, project_id, extraction_run_id, node_kind, stable_identity,
             display_name, metadata_json, created_at)
        VALUES ($1, $2, $3, 'code_symbol', $4, $5, '{}'::jsonb, now())
        "#,
    )
    .bind(node_id)
    .bind(project_id)
    .bind(run_id)
    .bind(stable_identity)
    .bind(display_name)
    .execute(pool)
    .await
    .expect("insert graph node");
    sqlx::query(
        r#"
        INSERT INTO code_symbols
            (id, project_id, extraction_run_id, graph_node_id, fact_id, stable_identity,
             language, file_path, symbol_kind, name, qualified_name, start_byte, end_byte,
             start_line, end_line, display_name, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $10,
                0, 10, $11, $12, $10, now())
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(run_id)
    .bind(node_id)
    .bind(format!("fact-{display_name}"))
    .bind(stable_identity)
    .bind(language)
    .bind(file_path)
    .bind(symbol_kind)
    .bind(display_name)
    .bind(start_line)
    .bind(end_line)
    .execute(pool)
    .await
    .expect("insert code symbol");
}

struct GraphEdgeFixture<'a> {
    edge_id: Uuid,
    source: Uuid,
    target: Uuid,
    edge_kind: &'a str,
    file_path: &'a str,
    line: i64,
}

async fn insert_edge(pool: &PgPool, project_id: Uuid, run_id: Uuid, edge: GraphEdgeFixture<'_>) {
    sqlx::query(
        r#"
        INSERT INTO graph_edges
            (id, project_id, extraction_run_id, source_node_id, target_node_id,
             edge_kind, confidence, metadata_json, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, 0.95,
                jsonb_build_object('reference_kind', 'call'), now())
        "#,
    )
    .bind(edge.edge_id)
    .bind(project_id)
    .bind(run_id)
    .bind(edge.source)
    .bind(edge.target)
    .bind(edge.edge_kind)
    .execute(pool)
    .await
    .expect("insert graph edge");
    sqlx::query(
        r#"
        INSERT INTO graph_evidence
            (id, project_id, extraction_run_id, edge_id, evidence_kind,
             file_path, start_line, end_line, confidence, strategy_version, created_at)
        VALUES ($1, $2, $3, $4, 'file_span', $5, $6, $6, 0.95,
                'code-graph-resolution-v1', now())
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(run_id)
    .bind(edge.edge_id)
    .bind(edge.file_path)
    .bind(edge.line)
    .execute(pool)
    .await
    .expect("insert graph evidence");
}

async fn insert_reference(
    pool: &PgPool,
    project_id: Uuid,
    run_id: Uuid,
    edge_id: Option<Uuid>,
    resolution_status: &str,
) {
    sqlx::query(
        r#"
        INSERT INTO code_references
            (id, project_id, extraction_run_id, graph_edge_id, fact_id, reference_kind,
             language, file_path, target_text, resolution_status, confidence,
             start_byte, end_byte, start_line, end_line, created_at)
        VALUES ($1, $2, $3, $4, $5, 'call', 'rust', 'src/handler.rs',
                'GraphRepository', $6, 0.95, 0, 10, 15, 15, now())
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(run_id)
    .bind(edge_id)
    .bind(format!("ref-{resolution_status}-{}", Uuid::new_v4()))
    .bind(resolution_status)
    .execute(pool)
    .await
    .expect("insert code reference");
}
