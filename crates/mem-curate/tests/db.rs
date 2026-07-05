use mem_api::{
    CaptureCandidateInput, CaptureCandidateSourceInput, CaptureTaskRequest, CurateRequest,
    MemoryType, ReplacementPolicy, SourceKind,
};
use mem_curate::{curate, preview_curate, refresh_semantic_relations, store_capture};
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
async fn capture_curate_and_replacement_proposal_flow_persists_rows() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };
    let project = mem_test_support::unique_project_slug("curate-db");
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup old test project");

    let first = capture_request(
        &project,
        "Initial memory",
        "The query pipeline stores source provenance in memory_sources for graph-aware retrieval.",
        "Query provenance pipeline",
    );
    let capture = store_capture(&pool, &first)
        .await
        .expect("store first capture");
    let response = curate(
        &pool,
        &CurateRequest {
            project: project.clone(),
            batch_size: Some(10),
            raw_capture_id: Some(capture.raw_capture_id),
            replacement_policy: Some(ReplacementPolicy::Balanced),
            dry_run: false,
        },
    )
    .await
    .expect("curate first capture");
    assert_eq!(response.output_count, 1);
    assert_eq!(response.proposal_count, 0);

    let second = capture_request(
        &project,
        "Candidate update",
        "The query pipeline stores source provenance in memory_sources for graph-aware retrieval and query diagnostics.",
        "Diagnostics candidate",
    );
    let second_capture = store_capture(&pool, &second)
        .await
        .expect("store second capture");
    let preview = preview_curate(
        &pool,
        &CurateRequest {
            project: project.clone(),
            batch_size: Some(10),
            raw_capture_id: Some(second_capture.raw_capture_id),
            replacement_policy: Some(ReplacementPolicy::Balanced),
            dry_run: true,
        },
    )
    .await
    .expect("preview second capture");
    assert_eq!(preview.proposal_count, 1);

    let response = curate(
        &pool,
        &CurateRequest {
            project: project.clone(),
            batch_size: Some(10),
            raw_capture_id: Some(second_capture.raw_capture_id),
            replacement_policy: Some(ReplacementPolicy::Balanced),
            dry_run: false,
        },
    )
    .await
    .expect("curate second capture");
    assert_eq!(response.output_count, 0);
    assert_eq!(response.proposal_count, 1);

    let memory_count: i64 = sqlx::query(
        r#"
        SELECT COUNT(*)::bigint AS count
        FROM memory_entries me
        JOIN projects p ON p.id = me.project_id
        WHERE p.slug = $1
        "#,
    )
    .bind(&project)
    .fetch_one(&pool)
    .await
    .expect("count memory rows")
    .try_get("count")
    .expect("decode memory count");
    assert_eq!(memory_count, 1);

    let proposal_count: i64 = sqlx::query(
        r#"
        SELECT COUNT(*)::bigint AS count
        FROM memory_replacement_proposals mrp
        JOIN projects p ON p.id = mrp.project_id
        WHERE p.slug = $1 AND mrp.status = 'pending'
        "#,
    )
    .bind(&project)
    .fetch_one(&pool)
    .await
    .expect("count proposal rows")
    .try_get("count")
    .expect("decode proposal count");
    assert_eq!(proposal_count, 1);

    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup test project");
}

#[tokio::test]
async fn semantic_dedup_links_paraphrases_and_flags_conflicts() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };
    let project = mem_test_support::unique_project_slug("semantic-db");
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup old test project");

    // Paraphrases with almost no shared >=3-char tokens and disjoint
    // tags/files, so the lexical relation classifier cannot link them.
    let duplicate_id = seed_memory(
        &pool,
        &project,
        "Deploy artifacts get shipped after checks finish running.",
        "Shipping happens post-checks",
        "deploy",
        "docs/deploy.md",
        "[1, 0, 0]",
    )
    .await;
    let paraphrase_id = seed_memory(
        &pool,
        &project,
        "Release bundles are published once verification completes.",
        "Publication follows verification",
        "release",
        "docs/release.md",
        "[0.999, 0.02, 0]",
    )
    .await;

    let duplicates = refresh_semantic_relations(&pool, &project, &[duplicate_id], 0.9)
        .await
        .expect("semantic pass over paraphrased pair");
    assert_eq!(duplicates.len(), 1);
    assert_eq!(duplicates[0].other_memory_id, paraphrase_id);
    assert!(duplicates[0].similarity >= 0.9);
    assert!(!duplicates[0].conflict);
    assert_eq!(
        relation_count(&pool, duplicate_id, paraphrase_id, "duplicates").await,
        1
    );
    assert_eq!(
        relation_count(&pool, paraphrase_id, duplicate_id, "duplicates").await,
        1
    );

    // Same embedding neighborhood, but supersede/negation language: must be
    // flagged as a conflict and linked related_to, never duplicates.
    let conflict_id = seed_memory(
        &pool,
        &project,
        "The pipeline no longer publishes bundles; that flow was retired.",
        "Bundle publishing retired",
        "pipeline",
        "docs/pipeline.md",
        "[0.998, 0.03, 0]",
    )
    .await;

    let conflicts = refresh_semantic_relations(&pool, &project, &[conflict_id], 0.9)
        .await
        .expect("semantic pass over conflicting memory");
    assert_eq!(conflicts.len(), 2);
    assert!(conflicts.iter().all(|pair| pair.conflict));
    assert_eq!(
        relation_count(&pool, conflict_id, duplicate_id, "related_to").await,
        1
    );
    assert_eq!(
        relation_count(&pool, conflict_id, duplicate_id, "duplicates").await,
        0
    );

    // Below-threshold pairs are ignored entirely.
    let unrelated = refresh_semantic_relations(&pool, &project, &[duplicate_id], 0.99999)
        .await
        .expect("semantic pass with strict threshold");
    assert!(unrelated.is_empty());

    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup test project");
}

async fn seed_memory(
    pool: &sqlx::PgPool,
    project: &str,
    canonical_text: &str,
    summary: &str,
    tag: &str,
    file_path: &str,
    embedding_literal: &str,
) -> Uuid {
    let mut request = capture_request(project, summary, canonical_text, summary);
    request.files_changed = vec![file_path.to_string()];
    request.structured_candidates[0].tags = vec![tag.to_string()];
    request.structured_candidates[0].sources[0].file_path = Some(file_path.to_string());
    let capture = store_capture(pool, &request).await.expect("store capture");
    curate(
        pool,
        &CurateRequest {
            project: project.to_string(),
            batch_size: Some(10),
            raw_capture_id: Some(capture.raw_capture_id),
            replacement_policy: Some(ReplacementPolicy::Balanced),
            dry_run: false,
        },
    )
    .await
    .expect("curate capture");

    let memory_id: Uuid = sqlx::query(
        r#"
        SELECT me.id
        FROM memory_entries me
        JOIN projects p ON p.id = me.project_id
        WHERE p.slug = $1 AND me.summary = $2
        "#,
    )
    .bind(project)
    .bind(summary)
    .fetch_one(pool)
    .await
    .expect("fetch seeded memory id")
    .try_get("id")
    .expect("decode memory id");

    // Chunk embeddings are normally built by service embedding maintenance;
    // insert deterministic vectors directly so the test needs no embedder.
    let chunk_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO memory_chunks (id, memory_entry_id, chunk_text, search_text, tsv)
        VALUES ($1, $2, $3, $3, to_tsvector('english', $3))
        "#,
    )
    .bind(chunk_id)
    .bind(memory_id)
    .bind(canonical_text)
    .execute(pool)
    .await
    .expect("insert chunk");
    sqlx::query(
        r#"
        INSERT INTO memory_chunk_embeddings (
            chunk_id, embedding_space, embedding, embedding_dimension, embedding_updated_at
        )
        VALUES ($1, 'test-space', $2::vector, 3, now())
        "#,
    )
    .bind(chunk_id)
    .bind(embedding_literal)
    .execute(pool)
    .await
    .expect("insert chunk embedding");
    memory_id
}

async fn relation_count(pool: &sqlx::PgPool, src: Uuid, dst: Uuid, relation_type: &str) -> i64 {
    sqlx::query(
        r#"
        SELECT COUNT(*)::bigint AS count
        FROM memory_relations
        WHERE src_memory_id = $1 AND dst_memory_id = $2 AND relation_type = $3
        "#,
    )
    .bind(src)
    .bind(dst)
    .bind(relation_type)
    .fetch_one(pool)
    .await
    .expect("count relations")
    .try_get("count")
    .expect("decode relation count")
}

fn capture_request(
    project: &str,
    title: &str,
    canonical_text: &str,
    summary: &str,
) -> CaptureTaskRequest {
    CaptureTaskRequest {
        project: project.to_string(),
        task_title: title.to_string(),
        user_prompt: "Record durable query provenance behavior.".to_string(),
        writer_id: "db-test".to_string(),
        writer_name: Some("DB Test".to_string()),
        agent_summary: "Captured a structured candidate for DB integration tests.".to_string(),
        files_changed: vec!["crates/mem-search/src/lib.rs".to_string()],
        git_diff_summary: None,
        tests: Vec::new(),
        notes: Vec::new(),
        structured_candidates: vec![CaptureCandidateInput {
            canonical_text: canonical_text.to_string(),
            summary: summary.to_string(),
            memory_type: MemoryType::Implementation,
            confidence: 0.9,
            importance: 3,
            tags: vec!["query".to_string()],
            sources: vec![CaptureCandidateSourceInput {
                file_path: Some("crates/mem-search/src/lib.rs".to_string()),
                symbol_name: None,
                symbol_kind: None,
                source_kind: SourceKind::File,
                excerpt: Some("query provenance".to_string()),
            }],
        }],
        command_output: None,
        idempotency_key: None,
        dry_run: false,
    }
}
