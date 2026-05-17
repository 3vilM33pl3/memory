use anyhow::{Context, Result};
use sqlx::{PgPool, postgres::PgPoolOptions};
use uuid::Uuid;

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
    sqlx::migrate!("../../migrations")
        .run(pool)
        .await
        .context("run Memory Layer migrations")
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
