use mem_reinforce::{AccessBatch, AccessKind, ScoreParams, record_access_batch};
use sqlx::PgPool;
use uuid::Uuid;

async fn insert_project(pool: &PgPool, slug: &str, project_id: Uuid) {
    sqlx::query(
        "INSERT INTO projects (id, slug, name, root_path, created_at) VALUES ($1, $2, $2, '/repo', now())",
    )
    .bind(project_id)
    .bind(slug)
    .execute(pool)
    .await
    .expect("insert project");
}

async fn insert_memory(pool: &PgPool, project_id: Uuid, memory_id: Uuid, text: &str) {
    sqlx::query(
        r#"
        INSERT INTO memory_entries
            (id, project_id, canonical_id, version_no, is_tombstone, canonical_text,
             summary, memory_type, scope, importance, confidence, status,
             created_at, updated_at, archived_at, search_document)
        VALUES ($1, $2, $1, 1, FALSE, $3, $3, 'implementation', 'project', 3, 0.9,
                'active', now(), now(), NULL, to_tsvector('english', $3))
        "#,
    )
    .bind(memory_id)
    .bind(project_id)
    .bind(text)
    .execute(pool)
    .await
    .expect("insert memory entry");
}

async fn insert_relation(pool: &PgPool, src: Uuid, relation_type: &str, dst: Uuid) {
    sqlx::query(
        "INSERT INTO memory_relations (id, src_memory_id, relation_type, dst_memory_id) VALUES ($1, $2, $3, $4)",
    )
    .bind(Uuid::new_v4())
    .bind(src)
    .bind(relation_type)
    .bind(dst)
    .execute(pool)
    .await
    .expect("insert relation");
}

async fn cleanup(pool: &PgPool, slug: &str, project_id: Uuid) {
    for table in [
        "memory_access_events",
        "memory_score_audit",
        "memory_validation_runs",
    ] {
        sqlx::query(&format!("DELETE FROM {table} WHERE project_id = $1"))
            .bind(project_id)
            .execute(pool)
            .await
            .expect("cleanup reinforcement table");
    }
    mem_test_support::cleanup_project(pool, slug)
        .await
        .expect("cleanup project");
}

fn test_params() -> ScoreParams {
    ScoreParams {
        fan_normalization: false,
        ..ScoreParams::default()
    }
}

#[tokio::test]
async fn access_batch_scores_direct_and_propagated_memories() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };
    let slug = mem_test_support::unique_project_slug("reinforce-batch");
    let project_id = Uuid::new_v4();
    let (m1, m2, m3, m4) = (
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    );
    insert_project(&pool, &slug, project_id).await;
    for (id, text) in [
        (m1, "Cited memory"),
        (m2, "One hop neighbour"),
        (m3, "Two hop neighbour"),
        (m4, "Superseded lineage"),
    ] {
        insert_memory(&pool, project_id, id, text).await;
    }
    insert_relation(&pool, m1, "related_to", m2).await;
    insert_relation(&pool, m2, "supports", m3).await;
    insert_relation(&pool, m1, "supersedes", m4).await;

    let batch = AccessBatch {
        operation_id: Some("test-query".to_string()),
        events: vec![(m1, AccessKind::Citation)],
    };
    let crossings = record_access_batch(&pool, &batch, &test_params(), 100.0)
        .await
        .expect("record batch");
    assert!(crossings.is_empty(), "threshold 100 must not be crossed");

    let scores = mem_reinforce::repository::fetch_scores(&pool, &[m1, m2, m3, m4])
        .await
        .expect("fetch scores");
    let score = |id: Uuid| scores.iter().find(|s| s.canonical_id == id);

    let s1 = score(m1).expect("cited memory scored");
    assert!((s1.activation - 1.5).abs() < 1e-9, "got {}", s1.activation);
    assert_eq!(s1.access_count, 1);
    assert_eq!(s1.citation_count, 1);
    assert_eq!(s1.propagated_count, 0);
    assert!(s1.last_access_at.is_some());

    let s2 = score(m2).expect("1-hop neighbour scored");
    assert!((s2.activation - 0.75).abs() < 1e-9, "got {}", s2.activation);
    assert_eq!(s2.access_count, 0);
    assert_eq!(s2.propagated_count, 1);
    assert!(s2.last_access_at.is_none(), "propagation is not an access");

    let s3 = score(m3).expect("2-hop neighbour scored");
    assert!(
        (s3.activation - 0.375).abs() < 1e-9,
        "got {}",
        s3.activation
    );

    assert!(
        score(m4).is_none(),
        "supersedes relations must not propagate"
    );

    let event_kinds: Vec<(String, i16)> = sqlx::query_as(
        "SELECT kind, hop_distance FROM memory_access_events WHERE project_id = $1 ORDER BY hop_distance",
    )
    .bind(project_id)
    .fetch_all(&pool)
    .await
    .expect("fetch access events");
    assert_eq!(
        event_kinds,
        vec![
            ("citation".to_string(), 0),
            ("propagated".to_string(), 1),
            ("propagated".to_string(), 2),
        ]
    );

    cleanup(&pool, &slug, project_id).await;
}

#[tokio::test]
async fn score_upsert_decays_before_boosting_across_calls() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };
    let slug = mem_test_support::unique_project_slug("reinforce-decay");
    let project_id = Uuid::new_v4();
    let memory_id = Uuid::new_v4();
    insert_project(&pool, &slug, project_id).await;
    insert_memory(&pool, project_id, memory_id, "Decaying memory").await;

    let params = test_params();
    let batch = AccessBatch {
        operation_id: None,
        events: vec![(memory_id, AccessKind::Retrieval)],
    };
    record_access_batch(&pool, &batch, &params, 100.0)
        .await
        .expect("first access");

    // Backdate the score by one half-life, then access again:
    // 1.0 decays to 0.5, plus 1.0 boost = 1.5.
    sqlx::query(
        "UPDATE memory_scores SET last_decay_at = now() - interval '30 days' WHERE canonical_id = $1",
    )
    .bind(memory_id)
    .execute(&pool)
    .await
    .expect("backdate last_decay_at");
    record_access_batch(&pool, &batch, &params, 100.0)
        .await
        .expect("second access");

    let scores = mem_reinforce::repository::fetch_scores(&pool, &[memory_id])
        .await
        .expect("fetch scores");
    let score = &scores[0];
    assert!(
        (score.activation - 1.5).abs() < 1e-3,
        "expected ~1.5, got {}",
        score.activation
    );
    assert_eq!(score.access_count, 2);

    cleanup(&pool, &slug, project_id).await;
}

#[tokio::test]
async fn threshold_crossing_is_reported_and_audited() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };
    let slug = mem_test_support::unique_project_slug("reinforce-threshold");
    let project_id = Uuid::new_v4();
    let memory_id = Uuid::new_v4();
    insert_project(&pool, &slug, project_id).await;
    insert_memory(&pool, project_id, memory_id, "Hot memory").await;

    let batch = AccessBatch {
        operation_id: Some("op-1".to_string()),
        events: vec![(memory_id, AccessKind::Citation)],
    };
    let crossings = record_access_batch(&pool, &batch, &test_params(), 1.0)
        .await
        .expect("record batch");
    assert_eq!(crossings.len(), 1);
    assert_eq!(crossings[0].canonical_id, memory_id);
    assert!(crossings[0].activation >= 1.0);

    // Second access does not re-cross.
    let crossings = record_access_batch(&pool, &batch, &test_params(), 1.0)
        .await
        .expect("record batch again");
    assert!(crossings.is_empty(), "already above threshold");

    let audit_reasons: Vec<(String,)> =
        sqlx::query_as("SELECT reason FROM memory_score_audit WHERE canonical_id = $1")
            .bind(memory_id)
            .fetch_all(&pool)
            .await
            .expect("fetch audit rows");
    assert_eq!(audit_reasons, vec![("threshold_crossed".to_string(),)]);

    cleanup(&pool, &slug, project_id).await;
}

#[tokio::test]
async fn unknown_memory_ids_are_skipped() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };
    let batch = AccessBatch {
        operation_id: None,
        events: vec![(Uuid::new_v4(), AccessKind::Retrieval)],
    };
    let crossings = record_access_batch(&pool, &batch, &test_params(), 1.0)
        .await
        .expect("record batch with unknown id");
    assert!(crossings.is_empty());
}
