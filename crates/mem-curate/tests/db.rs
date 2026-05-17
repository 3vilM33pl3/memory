use mem_api::{
    CaptureCandidateInput, CaptureCandidateSourceInput, CaptureTaskRequest, CurateRequest,
    MemoryType, ReplacementPolicy, SourceKind,
};
use mem_curate::{curate, preview_curate, store_capture};
use sqlx::Row;

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
                source_kind: SourceKind::File,
                excerpt: Some("query provenance".to_string()),
            }],
        }],
        command_output: None,
        idempotency_key: None,
        dry_run: false,
    }
}
