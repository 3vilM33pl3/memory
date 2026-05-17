use sqlx::Row;

#[tokio::test]
async fn migrations_apply_to_configured_database() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };

    let extension = sqlx::query("SELECT extname FROM pg_extension WHERE extname = 'vector'")
        .fetch_optional(&pool)
        .await
        .expect("query pgvector extension");
    assert!(
        extension.is_some(),
        "pgvector extension should be installed"
    );

    let row = sqlx::query("SELECT COUNT(*)::bigint AS count FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await
        .expect("query sqlx migration ledger");
    let count: i64 = row.try_get("count").expect("decode migration count");
    assert!(count >= 17, "expected all Memory Layer migrations to run");
}
