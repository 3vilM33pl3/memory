use std::path::PathBuf;

use mem_api::{
    CommitRecord, CommitSyncRequest, CommitSyncResponse, MemoryStatus, MemoryTypeCount, NamedCount,
    ProjectCommitsResponse, ProjectMemoriesResponse, ProjectMemoryListItem,
    ProjectOverviewResponse, SourceKindCount, ValidationError,
};
use mem_search::{parse_memory_type, parse_source_kind};
use mem_watch::{load_state, to_status};
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub fn parse_status_filter(input: &str) -> Result<String, ValidationError> {
    match input {
        "active" | "archived" => Ok(input.to_string()),
        _ => Err(ValidationError::new("status must be active or archived")),
    }
}

pub async fn fetch_project_memories(
    pool: &PgPool,
    slug: &str,
    status_filter: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<ProjectMemoriesResponse, sqlx::Error> {
    let total_row = sqlx::query(
        r#"
        SELECT COUNT(*) AS count
        FROM memory_entries m
        JOIN projects p ON p.id = m.project_id
        WHERE p.slug = $1
          AND ($2::text IS NULL OR m.status = $2)
        "#,
    )
    .bind(slug)
    .bind(status_filter)
    .fetch_one(pool)
    .await?;
    let total = total_row.try_get("count")?;

    let rows = sqlx::query(
        r#"
        SELECT
            m.id,
            m.summary,
            left(m.canonical_text, 240) AS preview,
            m.memory_type,
            m.status,
            m.confidence,
            m.importance,
            m.updated_at,
            ARRAY_REMOVE(ARRAY_AGG(DISTINCT mt.tag), NULL) AS tags,
            COUNT(DISTINCT mt.tag) AS tag_count,
            COUNT(DISTINCT ms.id) AS source_count
        FROM memory_entries m
        JOIN projects p ON p.id = m.project_id
        LEFT JOIN memory_tags mt ON mt.memory_entry_id = m.id
        LEFT JOIN memory_sources ms ON ms.memory_entry_id = m.id
        WHERE p.slug = $1
          AND ($2::text IS NULL OR m.status = $2)
        GROUP BY m.id
        ORDER BY m.updated_at DESC, m.id DESC
        LIMIT $3 OFFSET $4
        "#,
    )
    .bind(slug)
    .bind(status_filter)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(ProjectMemoryListItem {
            id: row.try_get("id")?,
            summary: row.try_get("summary")?,
            preview: row.try_get("preview")?,
            memory_type: parse_memory_type(&row.try_get::<String, _>("memory_type")?),
            status: match row.try_get::<String, _>("status")?.as_str() {
                "archived" => MemoryStatus::Archived,
                _ => MemoryStatus::Active,
            },
            confidence: row.try_get("confidence")?,
            importance: row.try_get("importance")?,
            updated_at: row.try_get("updated_at")?,
            tags: row.try_get("tags")?,
            tag_count: row.try_get("tag_count")?,
            source_count: row.try_get("source_count")?,
        });
    }

    Ok(ProjectMemoriesResponse {
        project: slug.to_string(),
        total,
        items,
    })
}

pub async fn fetch_project_overview(
    pool: &PgPool,
    slug: &str,
    automation: &mem_api::AutomationConfig,
    embeddings: &mem_api::EmbeddingConfig,
) -> Result<ProjectOverviewResponse, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT
            p.slug,
            COUNT(DISTINCT m.id) AS memory_entries_total,
            COUNT(DISTINCT m.id) FILTER (WHERE m.status = 'active') AS active_memories,
            COUNT(DISTINCT m.id) FILTER (WHERE m.status = 'archived') AS archived_memories,
            COUNT(DISTINCT rc.id) AS raw_captures_total,
            COUNT(DISTINCT rc.id) FILTER (WHERE rc.curated_at IS NULL) AS uncurated_raw_captures,
            COUNT(DISTINCT t.id) AS tasks_total,
            COUNT(DISTINCT s.id) AS sessions_total,
            COUNT(DISTINCT cr.id) AS curation_runs_total,
            COUNT(DISTINCT m.id) FILTER (WHERE m.updated_at >= now() - interval '7 days') AS recent_memories_7d,
            COUNT(DISTINCT rc.id) FILTER (WHERE rc.created_at >= now() - interval '7 days') AS recent_captures_7d,
            COUNT(DISTINCT m.id) FILTER (WHERE m.confidence >= 0.8) AS high_confidence_memories,
            COUNT(DISTINCT m.id) FILTER (WHERE m.confidence >= 0.5 AND m.confidence < 0.8) AS medium_confidence_memories,
            COUNT(DISTINCT m.id) FILTER (WHERE m.confidence < 0.5) AS low_confidence_memories,
            MAX(m.updated_at) AS last_memory_at,
            MAX(rc.created_at) AS last_capture_at,
            MAX(cr.created_at) AS last_curation_at,
            CAST(FLOOR(EXTRACT(EPOCH FROM (now() - MIN(rc.created_at) FILTER (WHERE rc.curated_at IS NULL))) / 3600) AS BIGINT) AS oldest_uncurated_capture_age_hours
        FROM projects p
        LEFT JOIN memory_entries m ON m.project_id = p.id
        LEFT JOIN sessions s ON s.project_id = p.id
        LEFT JOIN tasks t ON t.session_id = s.id
        LEFT JOIN raw_captures rc ON rc.task_id = t.id
        LEFT JOIN curation_runs cr ON cr.project_id = p.id
        WHERE p.slug = $1
        GROUP BY p.slug
        "#,
    )
    .bind(slug)
    .fetch_optional(pool)
    .await?;

    let memory_type_breakdown = fetch_memory_type_breakdown(pool, slug).await?;
    let source_kind_breakdown = fetch_source_kind_breakdown(pool, slug).await?;
    let top_tags = fetch_top_tags(pool, slug).await?;
    let top_files = fetch_top_files(pool, slug).await?;
    let automation = load_automation_status(slug, automation).await;
    let embedding_health = fetch_embedding_health(pool, slug, embeddings).await?;

    let Some(row) = row else {
        return Ok(empty_overview(
            slug,
            memory_type_breakdown,
            source_kind_breakdown,
            top_tags,
            top_files,
            embeddings,
            automation,
        ));
    };

    Ok(ProjectOverviewResponse {
        project: slug.to_string(),
        service_status: "ok".to_string(),
        database_status: "up".to_string(),
        memory_entries_total: row.try_get("memory_entries_total")?,
        active_memories: row.try_get("active_memories")?,
        archived_memories: row.try_get("archived_memories")?,
        raw_captures_total: row.try_get("raw_captures_total")?,
        uncurated_raw_captures: row.try_get("uncurated_raw_captures")?,
        tasks_total: row.try_get("tasks_total")?,
        sessions_total: row.try_get("sessions_total")?,
        curation_runs_total: row.try_get("curation_runs_total")?,
        recent_memories_7d: row.try_get("recent_memories_7d")?,
        recent_captures_7d: row.try_get("recent_captures_7d")?,
        high_confidence_memories: row.try_get("high_confidence_memories")?,
        medium_confidence_memories: row.try_get("medium_confidence_memories")?,
        low_confidence_memories: row.try_get("low_confidence_memories")?,
        last_memory_at: row.try_get("last_memory_at")?,
        last_capture_at: row.try_get("last_capture_at")?,
        last_curation_at: row.try_get("last_curation_at")?,
        oldest_uncurated_capture_age_hours: row.try_get("oldest_uncurated_capture_age_hours")?,
        embedding_chunks_total: embedding_health.embedding_chunks_total,
        fresh_embedding_chunks: embedding_health.fresh_embedding_chunks,
        stale_embedding_chunks: embedding_health.stale_embedding_chunks,
        missing_embedding_chunks: embedding_health.missing_embedding_chunks,
        embedding_spaces_total: embedding_health.embedding_spaces_total,
        active_embedding_provider: embedding_health.active_embedding_provider,
        active_embedding_model: embedding_health.active_embedding_model,
        memory_type_breakdown,
        source_kind_breakdown,
        top_tags,
        top_files,
        automation,
        watchers: None,
    })
}

pub async fn sync_project_commits(
    pool: &PgPool,
    request: &CommitSyncRequest,
) -> Result<CommitSyncResponse, sqlx::Error> {
    let project_row = sqlx::query(
        r#"
        INSERT INTO projects (id, slug, name, root_path)
        VALUES (gen_random_uuid(), $1, $1, $2)
        ON CONFLICT (slug) DO UPDATE SET name = EXCLUDED.name, root_path = EXCLUDED.root_path
        RETURNING id
        "#,
    )
    .bind(&request.project)
    .bind(&request.repo_root)
    .fetch_one(pool)
    .await?;
    let project_id: Uuid = project_row.try_get("id")?;

    let mut imported_count = 0usize;
    let mut updated_count = 0usize;
    for commit in &request.commits {
        let row = sqlx::query(
            r#"
            INSERT INTO project_commits (
                project_id,
                commit_hash,
                short_hash,
                subject,
                body,
                author_name,
                author_email,
                committed_at,
                parent_hashes,
                changed_paths,
                imported_at,
                search_document
            )
            VALUES (
                $1,
                $2,
                $3,
                $4,
                $5,
                $6,
                $7,
                $8,
                $9,
                $10,
                now(),
                to_tsvector('english', concat_ws(' ', $3, $4, $5, array_to_string($10::text[], ' ')))
            )
            ON CONFLICT (project_id, commit_hash) DO UPDATE
            SET short_hash = EXCLUDED.short_hash,
                subject = EXCLUDED.subject,
                body = EXCLUDED.body,
                author_name = EXCLUDED.author_name,
                author_email = EXCLUDED.author_email,
                committed_at = EXCLUDED.committed_at,
                parent_hashes = EXCLUDED.parent_hashes,
                changed_paths = EXCLUDED.changed_paths,
                imported_at = now(),
                search_document = EXCLUDED.search_document
            RETURNING (xmax = 0) AS inserted
            "#,
        )
        .bind(project_id)
        .bind(&commit.hash)
        .bind(&commit.short_hash)
        .bind(&commit.subject)
        .bind(&commit.body)
        .bind(&commit.author_name)
        .bind(&commit.author_email)
        .bind(commit.committed_at)
        .bind(&commit.parent_hashes)
        .bind(&commit.changed_paths)
        .fetch_one(pool)
        .await?;
        let inserted: bool = row.try_get("inserted")?;
        if inserted {
            imported_count += 1;
        } else {
            updated_count += 1;
        }
    }

    let newest_commit = request
        .commits
        .iter()
        .max_by_key(|commit| commit.committed_at)
        .map(|commit| commit.hash.clone());
    let oldest_commit = request
        .commits
        .iter()
        .min_by_key(|commit| commit.committed_at)
        .map(|commit| commit.hash.clone());

    Ok(CommitSyncResponse {
        project_id,
        imported_count,
        updated_count,
        total_received: request.commits.len(),
        newest_commit,
        oldest_commit,
    })
}

pub async fn fetch_project_commits(
    pool: &PgPool,
    slug: &str,
    limit: i64,
    offset: i64,
) -> Result<ProjectCommitsResponse, sqlx::Error> {
    let total_row = sqlx::query(
        r#"
        SELECT COUNT(*) AS count
        FROM project_commits pc
        JOIN projects p ON p.id = pc.project_id
        WHERE p.slug = $1
        "#,
    )
    .bind(slug)
    .fetch_one(pool)
    .await?;
    let total = total_row.try_get("count")?;

    let rows = sqlx::query(
        r#"
        SELECT
            pc.commit_hash,
            pc.short_hash,
            pc.subject,
            pc.body,
            pc.author_name,
            pc.author_email,
            pc.committed_at,
            pc.parent_hashes,
            pc.changed_paths,
            pc.imported_at
        FROM project_commits pc
        JOIN projects p ON p.id = pc.project_id
        WHERE p.slug = $1
        ORDER BY pc.committed_at DESC, pc.commit_hash DESC
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(slug)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    Ok(ProjectCommitsResponse {
        project: slug.to_string(),
        total,
        items: rows
            .into_iter()
            .map(row_to_commit_record)
            .collect::<Result<Vec<_>, _>>()?,
    })
}

pub async fn fetch_project_commit(
    pool: &PgPool,
    slug: &str,
    hash: &str,
) -> Result<Option<CommitRecord>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT
            pc.commit_hash,
            pc.short_hash,
            pc.subject,
            pc.body,
            pc.author_name,
            pc.author_email,
            pc.committed_at,
            pc.parent_hashes,
            pc.changed_paths,
            pc.imported_at
        FROM project_commits pc
        JOIN projects p ON p.id = pc.project_id
        WHERE p.slug = $1
          AND (pc.commit_hash = $2 OR pc.short_hash = $2)
        ORDER BY pc.committed_at DESC
        LIMIT 1
        "#,
    )
    .bind(slug)
    .bind(hash)
    .fetch_optional(pool)
    .await?;

    row.map(row_to_commit_record).transpose()
}

pub fn row_to_commit_record(row: sqlx::postgres::PgRow) -> Result<CommitRecord, sqlx::Error> {
    Ok(CommitRecord {
        hash: row.try_get("commit_hash")?,
        short_hash: row.try_get("short_hash")?,
        subject: row.try_get("subject")?,
        body: row.try_get("body")?,
        author_name: row.try_get("author_name")?,
        author_email: row.try_get("author_email")?,
        committed_at: row.try_get("committed_at")?,
        parent_hashes: row.try_get("parent_hashes")?,
        changed_paths: row.try_get("changed_paths")?,
        imported_at: row.try_get("imported_at")?,
    })
}

async fn fetch_memory_type_breakdown(
    pool: &PgPool,
    slug: &str,
) -> Result<Vec<MemoryTypeCount>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT m.memory_type, COUNT(*) AS count
        FROM memory_entries m
        JOIN projects p ON p.id = m.project_id
        WHERE p.slug = $1
        GROUP BY m.memory_type
        ORDER BY count DESC, m.memory_type ASC
        "#,
    )
    .bind(slug)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(MemoryTypeCount {
                memory_type: parse_memory_type(&row.try_get::<String, _>("memory_type")?),
                count: row.try_get("count")?,
            })
        })
        .collect()
}

async fn fetch_source_kind_breakdown(
    pool: &PgPool,
    slug: &str,
) -> Result<Vec<SourceKindCount>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT ms.source_kind, COUNT(*) AS count
        FROM memory_sources ms
        JOIN memory_entries m ON m.id = ms.memory_entry_id
        JOIN projects p ON p.id = m.project_id
        WHERE p.slug = $1
        GROUP BY ms.source_kind
        ORDER BY count DESC, ms.source_kind ASC
        "#,
    )
    .bind(slug)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(SourceKindCount {
                source_kind: parse_source_kind(&row.try_get::<String, _>("source_kind")?),
                count: row.try_get("count")?,
            })
        })
        .collect()
}

async fn fetch_top_tags(pool: &PgPool, slug: &str) -> Result<Vec<NamedCount>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT mt.tag AS name, COUNT(*) AS count
        FROM memory_tags mt
        JOIN memory_entries m ON m.id = mt.memory_entry_id
        JOIN projects p ON p.id = m.project_id
        WHERE p.slug = $1
        GROUP BY mt.tag
        ORDER BY count DESC, mt.tag ASC
        LIMIT 5
        "#,
    )
    .bind(slug)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(NamedCount {
                name: row.try_get("name")?,
                count: row.try_get("count")?,
            })
        })
        .collect()
}

async fn fetch_top_files(pool: &PgPool, slug: &str) -> Result<Vec<NamedCount>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT ms.file_path AS name, COUNT(*) AS count
        FROM memory_sources ms
        JOIN memory_entries m ON m.id = ms.memory_entry_id
        JOIN projects p ON p.id = m.project_id
        WHERE p.slug = $1
          AND ms.file_path IS NOT NULL
        GROUP BY ms.file_path
        ORDER BY count DESC, ms.file_path ASC
        LIMIT 5
        "#,
    )
    .bind(slug)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(NamedCount {
                name: row.try_get("name")?,
                count: row.try_get("count")?,
            })
        })
        .collect()
}

fn empty_overview(
    slug: &str,
    memory_type_breakdown: Vec<MemoryTypeCount>,
    source_kind_breakdown: Vec<SourceKindCount>,
    top_tags: Vec<NamedCount>,
    top_files: Vec<NamedCount>,
    embeddings: &mem_api::EmbeddingConfig,
    automation: Option<mem_api::AutomationStatus>,
) -> ProjectOverviewResponse {
    ProjectOverviewResponse {
        project: slug.to_string(),
        service_status: "ok".to_string(),
        database_status: "up".to_string(),
        memory_entries_total: 0,
        active_memories: 0,
        archived_memories: 0,
        raw_captures_total: 0,
        uncurated_raw_captures: 0,
        tasks_total: 0,
        sessions_total: 0,
        curation_runs_total: 0,
        recent_memories_7d: 0,
        recent_captures_7d: 0,
        high_confidence_memories: 0,
        medium_confidence_memories: 0,
        low_confidence_memories: 0,
        last_memory_at: None,
        last_capture_at: None,
        last_curation_at: None,
        oldest_uncurated_capture_age_hours: None,
        embedding_chunks_total: 0,
        fresh_embedding_chunks: 0,
        stale_embedding_chunks: 0,
        missing_embedding_chunks: 0,
        embedding_spaces_total: 0,
        active_embedding_provider: if embeddings.provider.trim().is_empty()
            || embeddings.model.trim().is_empty()
        {
            None
        } else {
            Some(embeddings.provider.clone())
        },
        active_embedding_model: if embeddings.provider.trim().is_empty()
            || embeddings.model.trim().is_empty()
        {
            None
        } else {
            Some(embeddings.model.clone())
        },
        memory_type_breakdown,
        source_kind_breakdown,
        top_tags,
        top_files,
        automation,
        watchers: None,
    }
}

#[derive(Default)]
struct EmbeddingHealth {
    embedding_chunks_total: i64,
    fresh_embedding_chunks: i64,
    stale_embedding_chunks: i64,
    missing_embedding_chunks: i64,
    embedding_spaces_total: i64,
    active_embedding_provider: Option<String>,
    active_embedding_model: Option<String>,
}

async fn fetch_embedding_health(
    pool: &PgPool,
    slug: &str,
    config: &mem_api::EmbeddingConfig,
) -> Result<EmbeddingHealth, sqlx::Error> {
    let active_provider = config.provider.trim();
    let active_model = config.model.trim();
    let active_base_url = config.base_url.trim_end_matches('/');
    let active_space = if active_provider.is_empty() || active_model.is_empty() {
        None
    } else {
        Some(format!(
            "{active_provider}|{active_base_url}|{active_model}"
        ))
    };

    let row = sqlx::query(
        r#"
        SELECT
            COUNT(mc.id) AS embedding_chunks_total,
            COUNT(mc.id) FILTER (
                WHERE $2::text IS NOT NULL
                  AND EXISTS (
                    SELECT 1
                    FROM memory_chunk_embeddings mce
                    WHERE mce.chunk_id = mc.id
                      AND mce.embedding_space = $2
                  )
            ) AS fresh_embedding_chunks,
            COUNT(mc.id) FILTER (
                WHERE NOT EXISTS (
                    SELECT 1
                    FROM memory_chunk_embeddings mce
                    WHERE mce.chunk_id = mc.id
                )
            ) AS missing_embedding_chunks,
            COUNT(mc.id) FILTER (
                WHERE $2::text IS NOT NULL
                  AND NOT EXISTS (
                    SELECT 1
                    FROM memory_chunk_embeddings mce
                    WHERE mce.chunk_id = mc.id
                      AND $2::text IS NOT NULL
                      AND mce.embedding_space = $2
                  )
                  AND EXISTS (
                    SELECT 1
                    FROM memory_chunk_embeddings mce
                    WHERE mce.chunk_id = mc.id
                  )
            ) AS stale_embedding_chunks,
            COALESCE((
                SELECT COUNT(DISTINCT mce.embedding_space)
                FROM memory_chunk_embeddings mce
                JOIN memory_chunks mc2 ON mc2.id = mce.chunk_id
                JOIN memory_entries m2 ON m2.id = mc2.memory_entry_id
                JOIN projects p2 ON p2.id = m2.project_id
                WHERE p2.slug = $1
                  AND m2.status = 'active'
            ), 0) AS embedding_spaces_total
        FROM memory_chunks mc
        JOIN memory_entries m ON m.id = mc.memory_entry_id
        JOIN projects p ON p.id = m.project_id
        WHERE p.slug = $1
          AND m.status = 'active'
        "#,
    )
    .bind(slug)
    .bind(active_space.as_deref())
    .fetch_one(pool)
    .await?;

    Ok(EmbeddingHealth {
        embedding_chunks_total: row.try_get("embedding_chunks_total")?,
        fresh_embedding_chunks: row.try_get("fresh_embedding_chunks")?,
        stale_embedding_chunks: row.try_get("stale_embedding_chunks")?,
        missing_embedding_chunks: row.try_get("missing_embedding_chunks")?,
        embedding_spaces_total: row.try_get("embedding_spaces_total")?,
        active_embedding_provider: if active_space.is_some() {
            Some(active_provider.to_string())
        } else {
            None
        },
        active_embedding_model: if active_space.is_some() {
            Some(active_model.to_string())
        } else {
            None
        },
    })
}

async fn load_automation_status(
    slug: &str,
    config: &mem_api::AutomationConfig,
) -> Option<mem_api::AutomationStatus> {
    let repo_root = config.repo_root.as_ref().map(PathBuf::from)?;
    let state = load_state(slug, &repo_root, config).await.ok()?;
    Some(to_status(&state))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use sqlx::{Executor, PgPool};
    use uuid::Uuid;

    use super::{fetch_project_memories, fetch_project_overview, parse_status_filter};

    #[tokio::test]
    async fn status_filter_validation_rejects_unknown_value() {
        assert!(parse_status_filter("weird").is_err());
    }

    #[tokio::test]
    async fn project_views_return_project_scoped_data() {
        let pool = test_pool().await;
        let slug = format!("test-{}", Uuid::new_v4());
        seed_project(&pool, &slug).await.unwrap();

        let memories = fetch_project_memories(&pool, &slug, Some("active"), 50, 0)
            .await
            .unwrap();
        assert_eq!(memories.project, slug);
        assert_eq!(memories.total, 1);
        assert_eq!(memories.items.len(), 1);
        assert_eq!(memories.items[0].summary, "Test memory");
        assert!(memories.items[0].tags.contains(&"alpha".to_string()));

        cleanup_project(&pool, &memories.project).await.unwrap();
    }

    #[tokio::test]
    async fn project_overview_returns_aggregates() {
        let pool = test_pool().await;
        let slug = format!("test-{}", Uuid::new_v4());
        seed_project(&pool, &slug).await.unwrap();

        let overview = fetch_project_overview(
            &pool,
            &slug,
            &mem_api::AutomationConfig::default(),
            &mem_api::EmbeddingConfig::default(),
        )
        .await
        .unwrap();
        assert_eq!(overview.project, slug);
        assert_eq!(overview.memory_entries_total, 1);
        assert_eq!(overview.active_memories, 1);
        assert_eq!(overview.raw_captures_total, 1);
        assert_eq!(overview.uncurated_raw_captures, 1);
        assert_eq!(overview.tasks_total, 1);
        assert_eq!(overview.sessions_total, 1);
        assert!(!overview.memory_type_breakdown.is_empty());
        assert!(!overview.source_kind_breakdown.is_empty());
        assert!(!overview.top_tags.is_empty());
        assert!(!overview.top_files.is_empty());

        cleanup_project(&pool, &overview.project).await.unwrap();
    }

    async fn test_pool() -> PgPool {
        let url = std::env::var("MEMORY_LAYER_TEST_DATABASE_URL")
            .ok()
            .or_else(|| read_local_test_database_url())
            .expect("test database url");
        let pool = PgPool::connect(&url).await.unwrap();
        sqlx::migrate!("../../migrations").run(&pool).await.unwrap();
        pool
    }

    fn read_local_test_database_url() -> Option<String> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("memory-postgres-connection.txt");
        let contents = fs::read_to_string(path).ok()?;
        let mut lines = contents.lines();
        while let Some(line) = lines.next() {
            if line.trim() == "Connection string (localhost):" {
                return lines.next().map(|line| line.trim().to_string());
            }
        }
        None
    }

    async fn cleanup_project(pool: &PgPool, slug: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM projects WHERE slug = $1")
            .bind(slug)
            .execute(pool)
            .await?;
        Ok(())
    }

    async fn seed_project(pool: &PgPool, slug: &str) -> Result<(), sqlx::Error> {
        let project_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();
        let memory_id = Uuid::new_v4();
        let source_id = Uuid::new_v4();
        let chunk_id = Uuid::new_v4();
        let capture_id = Uuid::new_v4();

        let mut tx = pool.begin().await?;
        tx.execute(sqlx::query("DELETE FROM projects WHERE slug = $1").bind(slug))
            .await?;
        tx.execute(
            sqlx::query(
                "INSERT INTO projects (id, slug, name, root_path, created_at) VALUES ($1, $2, $3, $4, now())",
            )
            .bind(project_id)
            .bind(slug)
            .bind(slug)
            .bind(slug),
        )
        .await?;
        tx.execute(
            sqlx::query(
                "INSERT INTO sessions (id, project_id, external_session_id, started_at, writer_id, writer_name) VALUES ($1, $2, $3, now(), $4, $5)",
            )
            .bind(session_id)
            .bind(project_id)
            .bind("test-session")
            .bind("codex-writer")
            .bind("codex"),
        )
        .await?;
        tx.execute(
            sqlx::query(
                "INSERT INTO tasks (id, session_id, title, user_prompt, task_summary, status, created_at, completed_at) VALUES ($1, $2, $3, $4, $5, 'completed', now(), now())",
            )
            .bind(task_id)
            .bind(session_id)
            .bind("Seed task")
            .bind("Prompt")
            .bind("Summary"),
        )
        .await?;
        tx.execute(
            sqlx::query(
                "INSERT INTO raw_captures (id, task_id, capture_type, payload_json, idempotency_key, created_at, curated_at) VALUES ($1, $2, 'task', '{}'::jsonb, $3, now(), NULL)",
            )
            .bind(capture_id)
            .bind(task_id)
            .bind(format!("seed-{slug}")),
        )
        .await?;
        tx.execute(
            sqlx::query(
                "INSERT INTO memory_entries (id, project_id, canonical_text, summary, memory_type, scope, importance, confidence, status, created_at, updated_at, archived_at, search_document) VALUES ($1, $2, $3, $4, 'architecture', 'project', 3, 0.85, 'active', now(), now(), NULL, to_tsvector('english', $3 || ' ' || $4))",
            )
            .bind(memory_id)
            .bind(project_id)
            .bind("Test canonical memory.")
            .bind("Test memory"),
        )
        .await?;
        tx.execute(
            sqlx::query("INSERT INTO memory_tags (memory_entry_id, tag) VALUES ($1, 'alpha')")
                .bind(memory_id),
        )
        .await?;
        tx.execute(
            sqlx::query(
                "INSERT INTO memory_sources (id, memory_entry_id, task_id, file_path, git_commit, source_kind, excerpt, created_at) VALUES ($1, $2, $3, $4, NULL, 'file', $5, now())",
            )
            .bind(source_id)
            .bind(memory_id)
            .bind(task_id)
            .bind("src/lib.rs")
            .bind("seed source"),
        )
        .await?;
        tx.execute(
            sqlx::query(
                "INSERT INTO memory_chunks (id, memory_entry_id, chunk_text, search_text, tsv) VALUES ($1, $2, $3, $4, to_tsvector('english', $4))",
            )
            .bind(chunk_id)
            .bind(memory_id)
            .bind("Test canonical memory.")
            .bind("Test memory Test canonical memory."),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
}
