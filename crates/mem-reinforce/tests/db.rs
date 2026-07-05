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

#[tokio::test]
async fn due_candidate_scan_respects_threshold_cooldown_review_and_volatility() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };
    let slug = mem_test_support::unique_project_slug("reinforce-due");
    let project_id = Uuid::new_v4();
    let (hot, cold, flagged, cooling, validated, volatile) = (
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    );
    insert_project(&pool, &slug, project_id).await;
    for (id, text) in [
        (hot, "Hot candidate"),
        (cold, "Cold candidate"),
        (flagged, "Flagged candidate"),
        (cooling, "Cooling candidate"),
        (validated, "Recently validated"),
        (volatile, "Volatile validated"),
    ] {
        insert_memory(&pool, project_id, id, text).await;
    }
    for (id, activation, extra_column, extra_value) in [
        (hot, 10.0, "", ""),
        (cold, 2.0, "", ""),
        (flagged, 10.0, ", needs_review", ", TRUE"),
        (
            cooling,
            10.0,
            ", validation_cooldown_until",
            ", now() + interval '1 day'",
        ),
        (
            validated,
            10.0,
            ", validated_at",
            ", now() - interval '7 days'",
        ),
    ] {
        sqlx::query(&format!(
            "INSERT INTO memory_scores (canonical_id, project_id, activation{extra_column}) VALUES ($1, $2, $3{extra_value})",
        ))
        .bind(id)
        .bind(project_id)
        .bind(activation)
        .execute(&pool)
        .await
        .expect("seed score row");
    }
    // volatility 1.0 with factor 4.0 shortens the 14d interval to 2.8d,
    // so a 7-day-old validation is due again.
    sqlx::query(
        "INSERT INTO memory_scores (canonical_id, project_id, activation, validated_at, volatility) VALUES ($1, $2, 10.0, now() - interval '7 days', 1.0)",
    )
    .bind(volatile)
    .bind(project_id)
    .execute(&pool)
    .await
    .expect("seed volatile score row");

    let params = mem_reinforce::repository::SelectionParams {
        threshold: 8.0,
        half_life_secs: 30.0 * 86400.0,
        min_revalidation_secs: 14.0 * 86400.0,
        volatility_factor: 4.0,
    };
    let due = mem_reinforce::repository::fetch_due_candidates(&pool, Some(project_id), &params, 10)
        .await
        .expect("scan due candidates");
    let due_ids: Vec<Uuid> = due.iter().map(|c| c.canonical_id).collect();
    assert!(due_ids.contains(&hot), "over threshold, never validated");
    assert!(
        due_ids.contains(&volatile),
        "volatility must shorten the revalidation interval"
    );
    assert!(!due_ids.contains(&cold), "below threshold");
    assert!(!due_ids.contains(&flagged), "needs_review excluded");
    assert!(!due_ids.contains(&cooling), "cooldown excluded");
    assert!(
        !due_ids.contains(&validated),
        "validated 7d ago with 14d interval and no volatility"
    );

    cleanup(&pool, &slug, project_id).await;
}

#[tokio::test]
async fn compaction_removes_cold_and_orphaned_rows_with_audit() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };
    let slug = mem_test_support::unique_project_slug("reinforce-compact");
    let project_id = Uuid::new_v4();
    let live = Uuid::new_v4();
    let cold = Uuid::new_v4();
    let orphan = Uuid::new_v4();
    insert_project(&pool, &slug, project_id).await;
    insert_memory(&pool, project_id, live, "Live scored memory").await;
    insert_memory(&pool, project_id, cold, "Cold scored memory").await;
    // live: healthy activation. cold: negligible activation, stale. orphan:
    // score row without any memory_entries backing.
    sqlx::query(
        "INSERT INTO memory_scores (canonical_id, project_id, activation) VALUES ($1, $2, 5.0)",
    )
    .bind(live)
    .bind(project_id)
    .execute(&pool)
    .await
    .expect("seed live");
    sqlx::query(
        "INSERT INTO memory_scores (canonical_id, project_id, activation, last_access_at, created_at) VALUES ($1, $2, 0.001, now() - interval '120 days', now() - interval '120 days')",
    )
    .bind(cold)
    .bind(project_id)
    .execute(&pool)
    .await
    .expect("seed cold");
    sqlx::query(
        "INSERT INTO memory_scores (canonical_id, project_id, activation) VALUES ($1, $2, 5.0)",
    )
    .bind(orphan)
    .bind(project_id)
    .execute(&pool)
    .await
    .expect("seed orphan");

    let summary = mem_reinforce::repository::compact_scores(&pool, 30.0 * 86400.0)
        .await
        .expect("compact");
    assert!(summary.cold_rows_deleted >= 1);
    assert!(summary.orphan_rows_deleted >= 1);

    let remaining = mem_reinforce::repository::fetch_scores(&pool, &[live, cold, orphan])
        .await
        .expect("fetch remaining");
    let remaining_ids: Vec<Uuid> = remaining.iter().map(|s| s.canonical_id).collect();
    assert!(remaining_ids.contains(&live));
    assert!(!remaining_ids.contains(&cold));
    assert!(!remaining_ids.contains(&orphan));

    let (audit_count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM memory_score_audit WHERE reason = 'decay_compaction'")
            .fetch_one(&pool)
            .await
            .expect("count compaction audit");
    assert!(audit_count >= 1);

    cleanup(&pool, &slug, project_id).await;
}

#[tokio::test]
async fn volatility_fold_updates_ewma_for_scored_memories() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };
    let slug = mem_test_support::unique_project_slug("reinforce-volatility");
    let project_id = Uuid::new_v4();
    let memory_id = Uuid::new_v4();
    insert_project(&pool, &slug, project_id).await;
    insert_memory(&pool, project_id, memory_id, "Volatile memory").await;
    // volatility_updated_at 2 days ago -> 4 changes over 2 days = 2/day.
    sqlx::query(
        "INSERT INTO memory_scores (canonical_id, project_id, activation, volatility, volatility_updated_at) VALUES ($1, $2, 5.0, 0.0, now() - interval '2 days')",
    )
    .bind(memory_id)
    .bind(project_id)
    .execute(&pool)
    .await
    .expect("seed score row");

    let mut changes = std::collections::HashMap::new();
    changes.insert(memory_id, 4_u32);
    let shifts = mem_reinforce::repository::fold_volatility(&pool, &changes, 0.5)
        .await
        .expect("fold volatility");
    assert_eq!(shifts.len(), 1);
    assert_eq!(shifts[0].old_volatility, 0.0);
    assert!(
        (f64::from(shifts[0].new_volatility) - 1.0).abs() < 0.05,
        "0.5 * 2/day = ~1.0, got {}",
        shifts[0].new_volatility
    );

    cleanup(&pool, &slug, project_id).await;
}

#[tokio::test]
async fn access_event_pruning_respects_cutoff() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };
    let slug = mem_test_support::unique_project_slug("reinforce-prune");
    let project_id = Uuid::new_v4();
    let memory_id = Uuid::new_v4();
    insert_project(&pool, &slug, project_id).await;
    insert_memory(&pool, project_id, memory_id, "Pruned memory").await;
    sqlx::query(
        "INSERT INTO memory_access_events (canonical_id, project_id, accessed_at, kind, boost) VALUES ($1, $2, now() - interval '40 days', 'retrieval', 1.0), ($1, $2, now(), 'retrieval', 1.0)",
    )
    .bind(memory_id)
    .bind(project_id)
    .execute(&pool)
    .await
    .expect("seed access events");

    let pruned = mem_reinforce::repository::prune_access_events(
        &pool,
        chrono::Utc::now() - chrono::Duration::days(30),
    )
    .await
    .expect("prune");
    assert!(pruned >= 1);
    let (remaining,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM memory_access_events WHERE project_id = $1")
            .bind(project_id)
            .fetch_all(&pool)
            .await
            .expect("count remaining")
            .into_iter()
            .next()
            .unwrap();
    assert_eq!(remaining, 1);

    cleanup(&pool, &slug, project_id).await;
}
