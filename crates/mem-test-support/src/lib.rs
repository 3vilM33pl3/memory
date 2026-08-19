use anyhow::{Context, Result};
use sqlx::{PgPool, postgres::PgPoolOptions};
use uuid::Uuid;

const PGVECTOR_MIGRATION_WITHOUT_LEGACY_HNSW: &str = r#"
CREATE EXTENSION IF NOT EXISTS vector;

DROP INDEX IF EXISTS idx_memory_chunks_embedding_hnsw;

ALTER TABLE memory_chunks
    DROP COLUMN IF EXISTS embedding;

ALTER TABLE memory_chunks
    ADD COLUMN IF NOT EXISTS embedding vector;
"#;

pub fn unique_project_slug(prefix: &str) -> String {
    format!("{prefix}-{}", Uuid::new_v4())
}

pub async fn migrated_pool() -> Option<PgPool> {
    let configured = std::env::var_os("MEMORY_LAYER_TEST_DATABASE_URL").is_some();
    match try_migrated_pool().await {
        Ok(pool) => Some(pool),
        Err(error) if configured || require_database() => panic!("{error:#}"),
        Err(_) => None,
    }
}

pub async fn try_migrated_pool() -> Result<PgPool> {
    let database_url = std::env::var("MEMORY_LAYER_TEST_DATABASE_URL").with_context(
        || "MEMORY_LAYER_TEST_DATABASE_URL must point at a PostgreSQL test database with pgvector",
    )?;
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .context("connect to MEMORY_LAYER_TEST_DATABASE_URL")?;
    run_migrations(&pool).await?;
    Ok(pool)
}

pub async fn run_migrations(pool: &PgPool) -> Result<()> {
    compatible_migrator()
        .run(pool)
        .await
        .context("run Memory Layer migrations")
}

fn compatible_migrator() -> sqlx::migrate::Migrator {
    let mut migrator = sqlx::migrate!("../../migrations");
    migrator.migrations = std::borrow::Cow::Owned(
        migrator
            .migrations
            .iter()
            .cloned()
            .map(|mut migration| {
                if migration.version == 4 {
                    migration.sql =
                        std::borrow::Cow::Borrowed(PGVECTOR_MIGRATION_WITHOUT_LEGACY_HNSW);
                }
                migration
            })
            .collect(),
    );
    migrator
}

pub async fn cleanup_project(pool: &PgPool, slug: &str) -> Result<()> {
    sqlx::query("DELETE FROM projects WHERE slug = $1")
        .bind(slug)
        .execute(pool)
        .await
        .with_context(|| format!("cleanup test project {slug}"))?;
    Ok(())
}

fn require_database() -> bool {
    std::env::var("MEMORY_LAYER_TEST_REQUIRE_DB").is_ok_and(|value| value == "1")
}

#[cfg(test)]
mod tests {
    #[test]
    fn compatible_pgvector_migration_preserves_checksum() {
        let original = sqlx::migrate!("../../migrations");
        let compatible = super::compatible_migrator();
        let original_migration = original
            .iter()
            .find(|migration| migration.version == 4)
            .expect("original migration 4");
        let compatible_migration = compatible
            .iter()
            .find(|migration| migration.version == 4)
            .expect("compatible migration 4");

        assert!(original_migration.sql.contains("CREATE INDEX"));
        assert!(!compatible_migration.sql.contains("CREATE INDEX"));
        assert_eq!(compatible_migration.checksum, original_migration.checksum);
    }
}
