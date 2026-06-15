use mem_api::{LoopRunRequest, LoopRunStatus, MemoryStatus, MemoryType};
use sqlx::PgPool;
use uuid::Uuid;

#[tokio::test]
async fn repository_handler_write_and_read_paths_roundtrip_memory() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };

    let project = mem_test_support::unique_project_slug("service-repository");
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup old test project");

    let project_id =
        mem_service::repository::handlers::bundle::upsert_project_slug(&pool, &project)
            .await
            .expect("upsert project through repository handler");
    let memory_id = insert_memory_fixture(&pool, project_id).await;

    let memory = mem_service::repository::handlers::memory::fetch_memory_entry(&pool, memory_id)
        .await
        .expect("fetch memory through repository handler")
        .expect("memory entry exists");

    assert_eq!(memory.id, memory_id);
    assert_eq!(memory.project, project);
    assert_eq!(memory.summary, "Repository DB test memory");
    assert_eq!(
        memory.canonical_text,
        "Repository handler tests cover a write path and a read path."
    );
    assert_eq!(memory.memory_type, MemoryType::Implementation);
    assert_eq!(memory.status, MemoryStatus::Active);
    assert_eq!(memory.version_no, 1);
    assert!(!memory.is_tombstone);

    mem_test_support::cleanup_project(&pool, &memory.project)
        .await
        .expect("cleanup test project");
}

#[tokio::test]
async fn loop_repository_registers_definitions_and_records_run() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };

    let project = mem_test_support::unique_project_slug("service-loop");
    let repo_root = format!("/tmp/{project}");
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup old test project");

    mem_service::repository::handlers::loops::register_builtin_loop_definitions(&pool)
        .await
        .expect("register builtin loops");
    let definitions =
        mem_service::repository::handlers::loops::list_registered_loop_definitions(&pool)
            .await
            .expect("fetch loop definitions");
    assert!(definitions.iter().any(|definition| {
        definition.loop_id == mem_loops::LOOP_CONTEXT_PACK_REFRESH && definition.version == 1
    }));

    let request = LoopRunRequest {
        project: Some(project.clone()),
        repo_root: Some(repo_root.clone()),
        scope_type: None,
        scope_id: None,
        dry_run: true,
        reason: Some("db repository integration test".to_string()),
        trigger_payload: None,
    };
    let run = mem_service::repository::handlers::loops::record_control_plane_loop_run(
        &pool,
        mem_loops::LOOP_CONTEXT_PACK_REFRESH,
        &request,
    )
    .await
    .expect("create loop run")
    .run;

    assert_eq!(run.summary.loop_id, mem_loops::LOOP_CONTEXT_PACK_REFRESH);
    assert_eq!(run.summary.project.as_deref(), Some(project.as_str()));
    assert_eq!(run.summary.repo_root.as_deref(), Some(repo_root.as_str()));
    assert_eq!(run.summary.status, LoopRunStatus::Blocked);
    assert!(
        run.summary
            .blocked_reasons
            .contains(&"loop_not_enabled".to_string())
    );
    assert_eq!(run.traces.len(), 2);

    let loaded =
        mem_service::repository::handlers::loops::read_loop_run_detail(&pool, run.summary.id)
            .await
            .expect("read loop run");
    assert_eq!(loaded.summary.id, run.summary.id);
    assert_eq!(loaded.summary.trace_count, 2);

    cleanup_loop_run(&pool, run.summary.id).await;
    cleanup_loop_triggers(&pool, &repo_root).await;
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup test project");
}

async fn insert_memory_fixture(pool: &PgPool, project_id: Uuid) -> Uuid {
    let memory_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO memory_entries
            (id, project_id, canonical_id, version_no, is_tombstone, canonical_text,
             summary, memory_type, scope, importance, confidence, status,
             created_at, updated_at, archived_at, search_document)
        VALUES ($1, $2, $1, 1, FALSE,
                'Repository handler tests cover a write path and a read path.',
                'Repository DB test memory', 'implementation', 'project', 3, 0.9,
                'active', now(), now(), NULL,
                to_tsvector('english', 'Repository handler tests cover a write path and a read path. Repository DB test memory'))
        "#,
    )
    .bind(memory_id)
    .bind(project_id)
    .execute(pool)
    .await
    .expect("insert memory fixture");
    memory_id
}

async fn cleanup_loop_run(pool: &PgPool, run_id: Uuid) {
    sqlx::query("DELETE FROM loop_runs WHERE id = $1")
        .bind(run_id)
        .execute(pool)
        .await
        .expect("cleanup loop run");
}

async fn cleanup_loop_triggers(pool: &PgPool, repo_root: &str) {
    sqlx::query("DELETE FROM trigger_events WHERE repo_root = $1")
        .bind(repo_root)
        .execute(pool)
        .await
        .expect("cleanup loop triggers");
}
