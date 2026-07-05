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
            include_stale: false,
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

#[tokio::test]
async fn provenance_decay_ranks_missing_file_below_verified_memory() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };

    let slug = mem_test_support::unique_project_slug("provenance-rank");
    let project_id = Uuid::new_v4();
    let verified_id = Uuid::new_v4();
    let missing_id = Uuid::new_v4();
    insert_provenance_ranking_fixture(&pool, &slug, project_id, verified_id, missing_id).await;

    let request = QueryRequest {
        project: slug.clone(),
        query: "ranking proof".to_string(),
        filters: Default::default(),
        top_k: 2,
        min_confidence: None,
        include_stale: false,
        history: false,
        retrieval_mode: None,
        answer_mode: None,
    };
    let response = mem_search::query_memory(&pool, &request, None)
        .await
        .expect("query memory");

    assert_eq!(response.results.len(), 2);
    assert_eq!(response.results[0].memory_id, verified_id);
    assert_eq!(response.results[1].memory_id, missing_id);
    assert!(
        response.results[1]
            .score_explanation
            .iter()
            .any(|line| line == "provenance decay x0.50 (missing_file)")
    );
    assert_eq!(response.diagnostics.provenance_decayed_candidates, 1);

    let include_stale = QueryRequest {
        include_stale: true,
        ..request
    };
    let response = mem_search::query_memory(&pool, &include_stale, None)
        .await
        .expect("query memory include stale");

    assert_eq!(response.results[0].memory_id, missing_id);
    assert!(
        response.results[0]
            .score_explanation
            .iter()
            .any(|line| line == "provenance stale bypassed (missing_file)")
    );

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

async fn insert_provenance_ranking_fixture(
    pool: &PgPool,
    slug: &str,
    project_id: Uuid,
    verified_id: Uuid,
    missing_id: Uuid,
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
    insert_rank_memory(
        pool,
        project_id,
        verified_id,
        "verified",
        "src/live.rs",
        "verified",
        10,
    )
    .await;
    insert_rank_memory(
        pool,
        project_id,
        missing_id,
        "missing",
        "src/deleted.rs",
        "missing_file",
        0,
    )
    .await;
}

async fn insert_rank_memory(
    pool: &PgPool,
    project_id: Uuid,
    memory_id: Uuid,
    label: &str,
    file_path: &str,
    provenance_status: &str,
    age_seconds: i64,
) {
    sqlx::query(
        r#"
        INSERT INTO memory_entries
            (id, project_id, canonical_id, version_no, is_tombstone, canonical_text,
             summary, memory_type, scope, importance, confidence, status,
             created_at, updated_at, archived_at, search_document)
        VALUES ($1, $2, $1, 1, FALSE, 'ranking proof durable detail',
                $3, 'implementation', 'project', 3, 0.9,
                'active', now(), now() - make_interval(secs => $4), NULL,
                to_tsvector('english', 'ranking proof durable detail'))
        "#,
    )
    .bind(memory_id)
    .bind(project_id)
    .bind(format!("{label} ranking proof"))
    .bind(age_seconds)
    .execute(pool)
    .await
    .expect("insert rank memory");
    let source_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO memory_sources (id, memory_entry_id, source_kind, file_path, created_at) VALUES ($1, $2, 'file', $3, now())",
    )
    .bind(source_id)
    .bind(memory_id)
    .bind(file_path)
    .execute(pool)
    .await
    .expect("insert rank source");
    sqlx::query(
        r#"
        INSERT INTO memory_source_verifications
            (source_id, status, checked_at, reason, resolved_path)
        VALUES ($1, $2, now(), $3, $4)
        "#,
    )
    .bind(source_id)
    .bind(provenance_status)
    .bind(format!("{provenance_status} test fixture"))
    .bind(format!("/repo/{file_path}"))
    .execute(pool)
    .await
    .expect("insert source verification");
}

#[tokio::test]
async fn activation_boost_reorders_equal_candidates_and_flags_needs_review() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };

    let slug = mem_test_support::unique_project_slug("reinforce-rank");
    let project_id = Uuid::new_v4();
    let cold_id = Uuid::new_v4();
    let hot_id = Uuid::new_v4();

    sqlx::query("INSERT INTO projects (id, slug, name, root_path, created_at) VALUES ($1, $2, $2, '/repo', now())")
        .bind(project_id)
        .bind(&slug)
        .execute(&pool)
        .await
        .expect("insert project");
    for memory_id in [cold_id, hot_id] {
        sqlx::query(
            r#"
            INSERT INTO memory_entries
                (id, project_id, canonical_id, version_no, is_tombstone, canonical_text,
                 summary, memory_type, scope, importance, confidence, status,
                 created_at, updated_at, archived_at, search_document)
            VALUES ($1, $2, $1, 1, FALSE, 'Reinforcement ranking twin memory.',
                    'Reinforcement ranking twin', 'implementation', 'project', 3, 0.9,
                    'active', now(), now(), NULL,
                    to_tsvector('english', 'Reinforcement ranking twin memory.'))
            "#,
        )
        .bind(memory_id)
        .bind(project_id)
        .execute(&pool)
        .await
        .expect("insert memory entry");
    }
    sqlx::query(
        "INSERT INTO memory_scores (canonical_id, project_id, activation) VALUES ($1, $2, 10.0)",
    )
    .bind(hot_id)
    .bind(project_id)
    .execute(&pool)
    .await
    .expect("insert score row");

    let request = QueryRequest {
        project: slug.clone(),
        query: "reinforcement ranking twin".to_string(),
        filters: Default::default(),
        top_k: 5,
        min_confidence: None,
        include_stale: false,
        history: false,
        retrieval_mode: None,
        answer_mode: None,
    };
    let params = mem_search::ReinforcementRankParams::default();
    let response =
        mem_search::query_memory_with_configs(&pool, &request, None, &Default::default(), &params)
            .await
            .expect("query with activation");
    assert_eq!(response.results.len(), 2);
    assert_eq!(
        response.results[0].memory_id, hot_id,
        "activation must outrank the identical cold twin"
    );
    assert!(
        response.results[0]
            .score_explanation
            .iter()
            .any(|item| item.starts_with("activation")),
        "explanation must show the activation boost"
    );

    // Flag the hot memory for review: the penalty must drop it below the twin.
    sqlx::query(
        "UPDATE memory_scores SET needs_review = TRUE, activation = 0 WHERE canonical_id = $1",
    )
    .bind(hot_id)
    .execute(&pool)
    .await
    .expect("flag needs_review");
    let response =
        mem_search::query_memory_with_configs(&pool, &request, None, &Default::default(), &params)
            .await
            .expect("query with needs_review");
    assert_eq!(response.results[0].memory_id, cold_id);
    assert!(response.results[1].needs_review);

    // Weight 0 restores parity with the unscored baseline ordering rules.
    let disabled = mem_search::ReinforcementRankParams {
        weight: 0.0,
        ..params
    };
    let response = mem_search::query_memory_with_configs(
        &pool,
        &request,
        None,
        &Default::default(),
        &disabled,
    )
    .await
    .expect("query with disabled weight");
    assert!(response.results.iter().all(|result| {
        result
            .score_explanation
            .iter()
            .all(|item| !item.starts_with("activation") && !item.starts_with("needs review"))
    }));

    mem_test_support::cleanup_project(&pool, &slug)
        .await
        .expect("cleanup project");
}
