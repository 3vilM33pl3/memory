use mem_api::{MemoryStatus, MemoryType};
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
