use mem_api::{
    MemoryStatus, MemoryTypeCount, NamedCount, ProjectMemoriesResponse, ProjectMemoryListItem,
    ProjectOverviewResponse, SourceKindCount, ValidationError,
};
use mem_search::{parse_memory_type, parse_source_kind};
use sqlx::{PgPool, Row};

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

    let Some(row) = row else {
        return Ok(empty_overview(
            slug,
            memory_type_breakdown,
            source_kind_breakdown,
            top_tags,
            top_files,
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
        memory_type_breakdown,
        source_kind_breakdown,
        top_tags,
        top_files,
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
        memory_type_breakdown,
        source_kind_breakdown,
        top_tags,
        top_files,
    }
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

        let overview = fetch_project_overview(&pool, &slug).await.unwrap();
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
                "INSERT INTO sessions (id, project_id, external_session_id, started_at, agent_name) VALUES ($1, $2, $3, now(), $4)",
            )
            .bind(session_id)
            .bind(project_id)
            .bind("test-session")
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
