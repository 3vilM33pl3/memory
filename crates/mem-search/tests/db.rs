use mem_api::QueryRequest;
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[tokio::test]
async fn backend_scoped_reindex_preserves_other_backend_embeddings() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };

    let slug = mem_test_support::unique_project_slug("embedding-reindex");
    let project_id = Uuid::new_v4();
    let memory_id = Uuid::new_v4();

    insert_embedding_reindex_fixture(&pool, &slug, project_id, memory_id).await;

    let first_registry = mem_search::test_support::static_embedding_registry(&[("first", 1.0)]);
    let first_space = first_registry
        .get("first")
        .expect("first backend")
        .embedding_space_key();
    mem_search::rebuild_chunks(&pool, &slug, &first_registry, None)
        .await
        .expect("initial reindex");
    let first_count_before = count_embeddings_for_space(&pool, memory_id, &first_space).await;
    assert!(first_count_before > 0);

    let second_registry =
        mem_search::test_support::static_embedding_registry(&[("first", 1.0), ("second", 2.0)]);
    let second_space = second_registry
        .get("second")
        .expect("second backend")
        .embedding_space_key();
    mem_search::rebuild_chunks(&pool, &slug, &second_registry, Some("second"))
        .await
        .expect("backend-scoped reindex");

    assert_eq!(
        count_embeddings_for_space(&pool, memory_id, &first_space).await,
        first_count_before,
        "backend-scoped reindex must not delete embeddings for other spaces"
    );
    assert_eq!(
        count_embeddings_for_space(&pool, memory_id, &second_space).await,
        first_count_before,
        "backend-scoped reindex should fill the selected backend on existing chunks"
    );

    mem_test_support::cleanup_project(&pool, &slug)
        .await
        .expect("cleanup project");
}

#[tokio::test]
async fn graph_candidates_return_memory_by_file_provenance() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };

    let slug = mem_test_support::unique_project_slug("graph-query");
    let project_id = Uuid::new_v4();
    let memory_id = Uuid::new_v4();
    let run_id = Uuid::new_v4();
    let node_id = Uuid::new_v4();
    let symbol_id = Uuid::new_v4();

    insert_graph_query_fixture(
        &pool, &slug, project_id, memory_id, run_id, node_id, symbol_id,
    )
    .await;

    let response = mem_search::query_memory(
        &pool,
        &QueryRequest {
            project: slug.clone(),
            query: "GraphTarget".to_string(),
            filters: Default::default(),
            top_k: 5,
            min_confidence: None,
            history: false,
            retrieval_mode: None,
            answer_mode: None,
        },
        None,
    )
    .await
    .expect("query memory");

    assert_eq!(response.diagnostics.graph_status, "active");
    assert_eq!(response.diagnostics.graph_candidates, 1);
    assert_eq!(response.results[0].memory_id, memory_id);
    assert!(response.results[0].debug.graph_boost > 0.0);
    assert_eq!(response.results[0].graph_connections.len(), 1);

    mem_test_support::cleanup_project(&pool, &slug)
        .await
        .expect("cleanup project");
}

async fn insert_embedding_reindex_fixture(
    pool: &PgPool,
    slug: &str,
    project_id: Uuid,
    memory_id: Uuid,
) {
    mem_test_support::cleanup_project(pool, slug)
        .await
        .expect("cleanup old project");
    sqlx::query("INSERT INTO projects (id, slug, name, root_path, created_at) VALUES ($1, $2, $2, '/repo', now())")
        .bind(project_id)
        .bind(slug)
        .execute(pool)
        .await
        .expect("insert project");
    sqlx::query(
        r#"
        INSERT INTO memory_entries
            (id, project_id, canonical_id, version_no, is_tombstone, canonical_text,
             summary, memory_type, scope, importance, confidence, status,
             created_at, updated_at, archived_at, search_document)
        VALUES ($1, $2, $1, 1, FALSE, 'Persistent backend embedding coverage.',
                'Backend coverage summary', 'implementation', 'project', 3, 0.9,
                'active', now(), now(), NULL,
                to_tsvector('english', 'Persistent backend embedding coverage. Backend coverage summary'))
        "#,
    )
    .bind(memory_id)
    .bind(project_id)
    .execute(pool)
    .await
    .expect("insert memory");
}

async fn count_embeddings_for_space(pool: &PgPool, memory_id: Uuid, space_key: &str) -> i64 {
    sqlx::query(
        r#"
        SELECT COUNT(*) AS count
        FROM memory_chunk_embeddings mce
        JOIN memory_chunks mc ON mc.id = mce.chunk_id
        WHERE mc.memory_entry_id = $1
          AND mce.embedding_space = $2
        "#,
    )
    .bind(memory_id)
    .bind(space_key)
    .fetch_one(pool)
    .await
    .expect("count embeddings")
    .try_get::<i64, _>("count")
    .expect("decode count")
}

async fn insert_graph_query_fixture(
    pool: &PgPool,
    slug: &str,
    project_id: Uuid,
    memory_id: Uuid,
    run_id: Uuid,
    node_id: Uuid,
    symbol_id: Uuid,
) {
    mem_test_support::cleanup_project(pool, slug)
        .await
        .expect("cleanup old project");
    sqlx::query("INSERT INTO projects (id, slug, name, root_path, created_at) VALUES ($1, $2, $2, '/repo', now())")
        .bind(project_id)
        .bind(slug)
        .execute(pool)
        .await
        .expect("insert project");
    sqlx::query(
        r#"
        INSERT INTO memory_entries
            (id, project_id, canonical_id, version_no, is_tombstone, canonical_text,
             summary, memory_type, scope, importance, confidence, status,
             created_at, updated_at, archived_at, search_document)
        VALUES ($1, $2, $1, 1, FALSE, 'Durable implementation detail.',
                'Unrelated summary', 'implementation', 'project', 3, 0.9,
                'active', now(), now(), NULL,
                to_tsvector('english', 'Durable implementation detail. Unrelated summary'))
        "#,
    )
    .bind(memory_id)
    .bind(project_id)
    .execute(pool)
    .await
    .expect("insert memory");
    sqlx::query(
        "INSERT INTO memory_sources (id, memory_entry_id, source_kind, file_path, created_at) VALUES ($1, $2, 'file', 'src/lib.rs', now())",
    )
    .bind(Uuid::new_v4())
    .bind(memory_id)
    .execute(pool)
    .await
    .expect("insert memory source");
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
    sqlx::query(
        r#"
        INSERT INTO graph_nodes
            (id, project_id, extraction_run_id, node_kind, stable_identity, display_name, metadata_json, created_at)
        VALUES ($1, $2, $3, 'code_symbol', 'rust:src/lib.rs:function:GraphTarget:1-1', 'GraphTarget', '{}'::jsonb, now())
        "#,
    )
    .bind(node_id)
    .bind(project_id)
    .bind(run_id)
    .execute(pool)
    .await
    .expect("insert graph node");
    sqlx::query(
        r#"
        INSERT INTO code_symbols
            (id, project_id, extraction_run_id, graph_node_id, fact_id, stable_identity,
             language, file_path, symbol_kind, name, qualified_name, start_byte, end_byte,
             start_line, end_line, display_name, created_at)
        VALUES ($1, $2, $3, $4, 'fact', 'rust:src/lib.rs:function:GraphTarget:1-1',
                'rust', 'src/lib.rs', 'function', 'GraphTarget', 'GraphTarget',
                0, 10, 1, 1, 'GraphTarget', now())
        "#,
    )
    .bind(symbol_id)
    .bind(project_id)
    .bind(run_id)
    .bind(node_id)
    .execute(pool)
    .await
    .expect("insert code symbol");
}
