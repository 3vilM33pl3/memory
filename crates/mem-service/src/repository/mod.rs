pub(crate) mod events;
pub mod handlers;
pub(crate) mod stream;

use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
};

use mem_api::{
    CommitRecord, CommitSyncRequest, CommitSyncResponse, MemoryStatus, MemoryTypeCount, NamedCount,
    ProjectCommitsResponse, ProjectMemoriesResponse, ProjectMemoryGraphEdge,
    ProjectMemoryGraphEdgeKind, ProjectMemoryGraphNode, ProjectMemoryGraphNodeKind,
    ProjectMemoryGraphResponse, ProjectMemoryListItem, ProjectOverviewResponse, SourceKindCount,
    ValidationError,
};
use mem_search::{
    effective_embedding_base_url, parse_memory_type, parse_relation_type, parse_source_kind,
};
use mem_watch::{load_state, to_status};
use sqlx::{PgPool, Row};
use uuid::Uuid;

const LATEST_PROJECT_MEMORIES_CTE: &str = r#"
latest AS (
    SELECT DISTINCT ON (m.canonical_id) m.*
    FROM memory_entries m
    JOIN projects p ON p.id = m.project_id
    WHERE p.slug = $1
    ORDER BY m.canonical_id, m.version_no DESC
)
"#;

const LATEST_PROJECT_MEMORY_IDS_CTE: &str = r#"
latest AS (
    SELECT DISTINCT ON (m.canonical_id) m.id, m.is_tombstone
    FROM memory_entries m
    JOIN projects p ON p.id = m.project_id
    WHERE p.slug = $1
    ORDER BY m.canonical_id, m.version_no DESC
)
"#;

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
    // Both the count and the list restrict themselves to the latest
    // non-tombstone version per canonical_id so a canonical memory appears
    // once even after updates. Older versions live on in the table and are
    // reachable via history-aware queries.
    let total_query = format!(
        r#"
        WITH {LATEST_PROJECT_MEMORIES_CTE}
        SELECT COUNT(*) AS count FROM latest
        WHERE latest.is_tombstone = FALSE
          AND ($2::text IS NULL OR latest.status = $2)
        "#
    );
    let total_row = sqlx::query(&total_query)
        .bind(slug)
        .bind(status_filter)
        .fetch_one(pool)
        .await?;
    let total = total_row.try_get("count")?;

    let list_query = format!(
        r#"
        WITH {LATEST_PROJECT_MEMORIES_CTE}
        SELECT
            m.id,
            m.summary,
            left(m.canonical_text, 240) AS preview,
            m.memory_type,
            m.status,
            m.confidence,
            m.importance,
            m.updated_at,
            m.canonical_id,
            m.version_no,
            m.is_tombstone,
            ARRAY_REMOVE(ARRAY_AGG(DISTINCT mt.tag), NULL) AS tags,
            COUNT(DISTINCT mt.tag) AS tag_count,
            COUNT(DISTINCT ms.id) AS source_count
        FROM latest m
        LEFT JOIN memory_tags mt ON mt.memory_entry_id = m.id
        LEFT JOIN memory_sources ms ON ms.memory_entry_id = m.id
        WHERE m.is_tombstone = FALSE
          AND ($2::text IS NULL OR m.status = $2)
        GROUP BY m.id, m.summary, m.canonical_text, m.memory_type, m.status,
                 m.confidence, m.importance, m.updated_at, m.canonical_id,
                 m.version_no, m.is_tombstone
        ORDER BY m.updated_at DESC, m.id DESC
        LIMIT $3 OFFSET $4
        "#
    );
    let rows = sqlx::query(&list_query)
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
            canonical_id: row.try_get("canonical_id")?,
            version_no: row.try_get("version_no")?,
            is_tombstone: row.try_get("is_tombstone")?,
        });
    }

    Ok(ProjectMemoriesResponse {
        project: slug.to_string(),
        total,
        items,
    })
}

pub async fn fetch_project_memory_graph(
    pool: &PgPool,
    slug: &str,
    limit: i64,
    offset: i64,
) -> Result<ProjectMemoryGraphResponse, sqlx::Error> {
    let total_query = format!(
        r#"
        WITH {LATEST_PROJECT_MEMORIES_CTE}
        SELECT COUNT(*) AS count
        FROM latest
        WHERE is_tombstone = FALSE
          AND status = 'active'
        "#
    );
    let total = sqlx::query(&total_query)
        .bind(slug)
        .fetch_one(pool)
        .await?
        .try_get("count")?;

    let memory_query = format!(
        r#"
        WITH {LATEST_PROJECT_MEMORIES_CTE}
        SELECT
            m.id,
            m.summary,
            m.memory_type,
            m.confidence,
            m.importance,
            m.updated_at,
            ARRAY_REMOVE(ARRAY_AGG(DISTINCT mt.tag), NULL) AS tags
        FROM latest m
        LEFT JOIN memory_tags mt ON mt.memory_entry_id = m.id
        WHERE m.is_tombstone = FALSE
          AND m.status = 'active'
        GROUP BY m.id, m.summary, m.memory_type, m.confidence, m.importance, m.updated_at
        ORDER BY m.updated_at DESC, m.id DESC
        LIMIT $2 OFFSET $3
        "#
    );
    let memory_rows = sqlx::query(&memory_query)
        .bind(slug)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;

    let mut nodes = Vec::new();
    let mut memory_ids = Vec::with_capacity(memory_rows.len());
    for row in memory_rows {
        let memory_id: Uuid = row.try_get("id")?;
        memory_ids.push(memory_id);
        let summary: String = row.try_get("summary")?;
        nodes.push(ProjectMemoryGraphNode {
            id: memory_node_id(memory_id),
            label: summary.clone(),
            node_kind: ProjectMemoryGraphNodeKind::Memory,
            memory_id: Some(memory_id),
            source_id: None,
            memory_type: Some(parse_memory_type(&row.try_get::<String, _>("memory_type")?)),
            source_kind: None,
            confidence: Some(row.try_get("confidence")?),
            importance: Some(row.try_get("importance")?),
            tags: row.try_get("tags")?,
            file_path: None,
            git_commit: None,
            symbol_name: None,
            symbol_kind: None,
            provenance_status: None,
            summary: Some(summary),
        });
    }

    if memory_ids.is_empty() {
        return Ok(ProjectMemoryGraphResponse {
            project: slug.to_string(),
            total_memories: total,
            returned_memories: 0,
            nodes,
            edges: Vec::new(),
        });
    }

    let selected_memory_ids: HashSet<Uuid> = memory_ids.iter().copied().collect();
    let mut edges = Vec::new();
    let mut source_nodes = BTreeMap::new();
    let source_rows = sqlx::query(
        r#"
        SELECT
            ms.id,
            ms.memory_entry_id,
            ms.file_path,
            ms.git_commit,
            ms.symbol_name,
            ms.symbol_kind,
            ms.source_kind,
            v.status AS provenance_status
        FROM memory_sources ms
        LEFT JOIN memory_source_verifications v ON v.source_id = ms.id
        WHERE ms.memory_entry_id = ANY($1)
        ORDER BY ms.created_at ASC, ms.id ASC
        "#,
    )
    .bind(&memory_ids)
    .fetch_all(pool)
    .await?;

    for row in source_rows {
        let source_id: Uuid = row.try_get("id")?;
        let memory_id: Uuid = row.try_get("memory_entry_id")?;
        if !selected_memory_ids.contains(&memory_id) {
            continue;
        }
        let source_kind = parse_source_kind(&row.try_get::<String, _>("source_kind")?);
        let file_path: Option<String> = row.try_get("file_path")?;
        let git_commit: Option<String> = row.try_get("git_commit")?;
        let symbol_name: Option<String> = row.try_get("symbol_name")?;
        let symbol_kind: Option<String> = row.try_get("symbol_kind")?;
        let provenance_status =
            row.try_get::<Option<String>, _>("provenance_status")?
                .map(|status| {
                    crate::repository::handlers::memory::parse_source_provenance_status(&status)
                });
        let node_id = source_node_id(
            source_id,
            file_path.as_deref(),
            git_commit.as_deref(),
            symbol_name.as_deref(),
        );
        source_nodes
            .entry(node_id.clone())
            .or_insert_with(|| ProjectMemoryGraphNode {
                id: node_id.clone(),
                label: source_node_label(
                    &source_kind,
                    file_path.as_deref(),
                    git_commit.as_deref(),
                    symbol_name.as_deref(),
                ),
                node_kind: ProjectMemoryGraphNodeKind::Source,
                memory_id: None,
                source_id: Some(source_id),
                memory_type: None,
                source_kind: Some(source_kind.clone()),
                confidence: None,
                importance: None,
                tags: Vec::new(),
                file_path: file_path.clone(),
                git_commit: git_commit.clone(),
                symbol_name: symbol_name.clone(),
                symbol_kind: symbol_kind.clone(),
                provenance_status: provenance_status.clone(),
                summary: None,
            });
        edges.push(ProjectMemoryGraphEdge {
            id: format!("provenance:{memory_id}:{node_id}"),
            source: memory_node_id(memory_id),
            target: node_id,
            edge_kind: ProjectMemoryGraphEdgeKind::Provenance,
            relation_type: None,
            source_kind: Some(source_kind),
        });
    }

    nodes.extend(source_nodes.into_values());

    let relation_rows = sqlx::query(
        r#"
        SELECT src_memory_id, dst_memory_id, relation_type
        FROM memory_relations
        WHERE src_memory_id = ANY($1)
          AND dst_memory_id = ANY($1)
        ORDER BY relation_type ASC, src_memory_id ASC, dst_memory_id ASC
        "#,
    )
    .bind(&memory_ids)
    .fetch_all(pool)
    .await?;

    for row in relation_rows {
        let source_memory_id: Uuid = row.try_get("src_memory_id")?;
        let target_memory_id: Uuid = row.try_get("dst_memory_id")?;
        let relation_type = parse_relation_type(&row.try_get::<String, _>("relation_type")?);
        edges.push(ProjectMemoryGraphEdge {
            id: format!("relation:{source_memory_id}:{relation_type}:{target_memory_id}"),
            source: memory_node_id(source_memory_id),
            target: memory_node_id(target_memory_id),
            edge_kind: ProjectMemoryGraphEdgeKind::MemoryRelation,
            relation_type: Some(relation_type),
            source_kind: None,
        });
    }

    Ok(ProjectMemoryGraphResponse {
        project: slug.to_string(),
        total_memories: total,
        returned_memories: memory_ids.len(),
        nodes,
        edges,
    })
}

fn memory_node_id(memory_id: Uuid) -> String {
    format!("memory:{memory_id}")
}

fn source_node_id(
    source_id: Uuid,
    file_path: Option<&str>,
    git_commit: Option<&str>,
    symbol_name: Option<&str>,
) -> String {
    if let Some(file_path) = file_path.filter(|value| !value.trim().is_empty()) {
        if let Some(symbol_name) = symbol_name.filter(|value| !value.trim().is_empty()) {
            return format!("source:file:{file_path}::{symbol_name}");
        }
        return format!("source:file:{file_path}");
    }
    if let Some(git_commit) = git_commit.filter(|value| !value.trim().is_empty()) {
        return format!("source:git:{git_commit}");
    }
    if let Some(symbol_name) = symbol_name.filter(|value| !value.trim().is_empty()) {
        return format!("source:symbol:{symbol_name}");
    }
    format!("source:{source_id}")
}

fn source_node_label(
    source_kind: &mem_api::SourceKind,
    file_path: Option<&str>,
    git_commit: Option<&str>,
    symbol_name: Option<&str>,
) -> String {
    if let Some(file_path) = file_path.filter(|value| !value.trim().is_empty()) {
        if let Some(symbol_name) = symbol_name.filter(|value| !value.trim().is_empty()) {
            return format!("{file_path}::{symbol_name}");
        }
        return file_path.to_string();
    }
    if let Some(git_commit) = git_commit.filter(|value| !value.trim().is_empty()) {
        return format!("commit {}", git_commit.chars().take(12).collect::<String>());
    }
    if let Some(symbol_name) = symbol_name.filter(|value| !value.trim().is_empty()) {
        return symbol_name.to_string();
    }
    match source_kind {
        mem_api::SourceKind::TaskPrompt => "task prompt",
        mem_api::SourceKind::File => "file",
        mem_api::SourceKind::GitCommit => "git commit",
        mem_api::SourceKind::CommandOutput => "command output",
        mem_api::SourceKind::Test => "test",
        mem_api::SourceKind::Note => "note",
    }
    .to_string()
}

pub async fn fetch_project_overview(
    pool: &PgPool,
    slug: &str,
    automation: &mem_api::AutomationConfig,
    embeddings: Option<&mem_api::EmbeddingBackendConfig>,
) -> Result<ProjectOverviewResponse, sqlx::Error> {
    let project_exists_fut = project_exists(pool, slug);
    let memory_stats_fut = fetch_memory_stats(pool, slug);
    let capture_stats_fut = fetch_capture_stats(pool, slug);
    let curation_stats_fut = fetch_curation_stats(pool, slug);
    let memory_type_breakdown_fut = fetch_memory_type_breakdown(pool, slug);
    let source_kind_breakdown_fut = fetch_source_kind_breakdown(pool, slug);
    let top_tags_fut = fetch_top_tags(pool, slug);
    let top_files_fut = fetch_top_files(pool, slug);
    let automation_fut = load_automation_status(slug, automation);
    let embedding_health_fut = fetch_embedding_health(pool, slug, embeddings);
    let pending_replacement_proposals_fut = fetch_pending_replacement_proposals(pool, slug);

    let (
        project_exists_result,
        memory_stats_result,
        capture_stats_result,
        curation_stats_result,
        memory_type_breakdown_result,
        source_kind_breakdown_result,
        top_tags_result,
        top_files_result,
        automation,
        embedding_health_result,
        pending_replacement_proposals_result,
    ) = tokio::join!(
        project_exists_fut,
        memory_stats_fut,
        capture_stats_fut,
        curation_stats_fut,
        memory_type_breakdown_fut,
        source_kind_breakdown_fut,
        top_tags_fut,
        top_files_fut,
        automation_fut,
        embedding_health_fut,
        pending_replacement_proposals_fut,
    );
    let project_exists = project_exists_result?;
    let memory_stats = memory_stats_result?;
    let capture_stats = capture_stats_result?;
    let curation_stats = curation_stats_result?;
    let memory_type_breakdown = memory_type_breakdown_result?;
    let source_kind_breakdown = source_kind_breakdown_result?;
    let top_tags = top_tags_result?;
    let top_files = top_files_result?;
    let embedding_health = embedding_health_result?;
    let pending_replacement_proposals = pending_replacement_proposals_result?;

    if !project_exists {
        return Ok(empty_overview(
            slug,
            memory_type_breakdown,
            source_kind_breakdown,
            top_tags,
            top_files,
            embeddings,
            automation,
        ));
    }

    Ok(ProjectOverviewResponse {
        project: slug.to_string(),
        service_status: "ok".to_string(),
        database_status: "up".to_string(),
        memory_entries_total: memory_stats.memory_entries_total,
        active_memories: memory_stats.active_memories,
        archived_memories: memory_stats.archived_memories,
        raw_captures_total: capture_stats.raw_captures_total,
        uncurated_raw_captures: capture_stats.uncurated_raw_captures,
        tasks_total: capture_stats.tasks_total,
        sessions_total: capture_stats.sessions_total,
        curation_runs_total: curation_stats.curation_runs_total,
        recent_memories_7d: memory_stats.recent_memories_7d,
        recent_captures_7d: capture_stats.recent_captures_7d,
        high_confidence_memories: memory_stats.high_confidence_memories,
        medium_confidence_memories: memory_stats.medium_confidence_memories,
        low_confidence_memories: memory_stats.low_confidence_memories,
        last_memory_at: memory_stats.last_memory_at,
        last_capture_at: capture_stats.last_capture_at,
        last_curation_at: curation_stats.last_curation_at,
        oldest_uncurated_capture_age_hours: capture_stats.oldest_uncurated_capture_age_hours,
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
        pending_replacement_proposals,
        automation,
        watchers: None,
    })
}

async fn project_exists(pool: &PgPool, slug: &str) -> Result<bool, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM projects WHERE slug = $1
        ) AS project_exists
        "#,
    )
    .bind(slug)
    .fetch_one(pool)
    .await?;
    row.try_get("project_exists")
}

struct MemoryStats {
    memory_entries_total: i64,
    active_memories: i64,
    archived_memories: i64,
    recent_memories_7d: i64,
    high_confidence_memories: i64,
    medium_confidence_memories: i64,
    low_confidence_memories: i64,
    last_memory_at: Option<chrono::DateTime<chrono::Utc>>,
}

async fn fetch_memory_stats(pool: &PgPool, slug: &str) -> Result<MemoryStats, sqlx::Error> {
    let query = format!(
        r#"
        WITH {LATEST_PROJECT_MEMORIES_CTE}
        SELECT
            COUNT(*) AS memory_entries_total,
            COUNT(*) FILTER (WHERE m.status = 'active') AS active_memories,
            COUNT(*) FILTER (WHERE m.status = 'archived') AS archived_memories,
            COUNT(*) FILTER (WHERE m.updated_at >= now() - interval '7 days') AS recent_memories_7d,
            COUNT(*) FILTER (WHERE m.confidence >= 0.8) AS high_confidence_memories,
            COUNT(*) FILTER (WHERE m.confidence >= 0.5 AND m.confidence < 0.8) AS medium_confidence_memories,
            COUNT(*) FILTER (WHERE m.confidence < 0.5) AS low_confidence_memories,
            MAX(m.updated_at) AS last_memory_at
        FROM latest m
        WHERE m.is_tombstone = FALSE
        "#
    );
    let row = sqlx::query(&query).bind(slug).fetch_one(pool).await?;

    Ok(MemoryStats {
        memory_entries_total: row.try_get("memory_entries_total")?,
        active_memories: row.try_get("active_memories")?,
        archived_memories: row.try_get("archived_memories")?,
        recent_memories_7d: row.try_get("recent_memories_7d")?,
        high_confidence_memories: row.try_get("high_confidence_memories")?,
        medium_confidence_memories: row.try_get("medium_confidence_memories")?,
        low_confidence_memories: row.try_get("low_confidence_memories")?,
        last_memory_at: row.try_get("last_memory_at")?,
    })
}

struct CaptureStats {
    raw_captures_total: i64,
    uncurated_raw_captures: i64,
    tasks_total: i64,
    sessions_total: i64,
    recent_captures_7d: i64,
    last_capture_at: Option<chrono::DateTime<chrono::Utc>>,
    oldest_uncurated_capture_age_hours: Option<i64>,
}

async fn fetch_capture_stats(pool: &PgPool, slug: &str) -> Result<CaptureStats, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT
            COUNT(DISTINCT s.id) AS sessions_total,
            COUNT(DISTINCT t.id) AS tasks_total,
            COUNT(DISTINCT rc.id) AS raw_captures_total,
            COUNT(DISTINCT rc.id) FILTER (WHERE rc.curated_at IS NULL) AS uncurated_raw_captures,
            COUNT(DISTINCT rc.id) FILTER (WHERE rc.created_at >= now() - interval '7 days') AS recent_captures_7d,
            MAX(rc.created_at) AS last_capture_at,
            CAST(FLOOR(EXTRACT(EPOCH FROM (now() - MIN(rc.created_at) FILTER (WHERE rc.curated_at IS NULL))) / 3600) AS BIGINT) AS oldest_uncurated_capture_age_hours
        FROM sessions s
        JOIN projects p ON p.id = s.project_id
        LEFT JOIN tasks t ON t.session_id = s.id
        LEFT JOIN raw_captures rc ON rc.task_id = t.id
        WHERE p.slug = $1
        "#,
    )
    .bind(slug)
    .fetch_one(pool)
    .await?;

    Ok(CaptureStats {
        raw_captures_total: row.try_get("raw_captures_total")?,
        uncurated_raw_captures: row.try_get("uncurated_raw_captures")?,
        tasks_total: row.try_get("tasks_total")?,
        sessions_total: row.try_get("sessions_total")?,
        recent_captures_7d: row.try_get("recent_captures_7d")?,
        last_capture_at: row.try_get("last_capture_at")?,
        oldest_uncurated_capture_age_hours: row.try_get("oldest_uncurated_capture_age_hours")?,
    })
}

struct CurationStats {
    curation_runs_total: i64,
    last_curation_at: Option<chrono::DateTime<chrono::Utc>>,
}

async fn fetch_curation_stats(pool: &PgPool, slug: &str) -> Result<CurationStats, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT
            COUNT(*) AS curation_runs_total,
            MAX(cr.created_at) AS last_curation_at
        FROM curation_runs cr
        JOIN projects p ON p.id = cr.project_id
        WHERE p.slug = $1
        "#,
    )
    .bind(slug)
    .fetch_one(pool)
    .await?;

    Ok(CurationStats {
        curation_runs_total: row.try_get("curation_runs_total")?,
        last_curation_at: row.try_get("last_curation_at")?,
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
        dry_run: false,
    })
}

pub async fn preview_project_commit_sync(
    pool: &PgPool,
    request: &CommitSyncRequest,
) -> Result<CommitSyncResponse, sqlx::Error> {
    let project_id = sqlx::query("SELECT id FROM projects WHERE slug = $1 LIMIT 1")
        .bind(&request.project)
        .fetch_optional(pool)
        .await?
        .and_then(|row| row.try_get("id").ok())
        .unwrap_or_else(Uuid::nil);

    let existing_hashes = if project_id.is_nil() {
        HashSet::new()
    } else {
        sqlx::query("SELECT commit_hash FROM project_commits WHERE project_id = $1")
            .bind(project_id)
            .fetch_all(pool)
            .await?
            .into_iter()
            .filter_map(|row| row.try_get::<String, _>("commit_hash").ok())
            .collect::<HashSet<_>>()
    };

    let updated_count = request
        .commits
        .iter()
        .filter(|commit| existing_hashes.contains(&commit.hash))
        .count();
    let imported_count = request.commits.len().saturating_sub(updated_count);
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
        dry_run: true,
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
    let query = format!(
        r#"
        WITH {LATEST_PROJECT_MEMORIES_CTE}
        SELECT m.memory_type, COUNT(*) AS count
        FROM latest m
        WHERE m.is_tombstone = FALSE
        GROUP BY m.memory_type
        ORDER BY count DESC, m.memory_type ASC
        "#
    );
    let rows = sqlx::query(&query).bind(slug).fetch_all(pool).await?;

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
    let query = format!(
        r#"
        WITH {LATEST_PROJECT_MEMORY_IDS_CTE}
        SELECT ms.source_kind, COUNT(*) AS count
        FROM memory_sources ms
        JOIN latest m ON m.id = ms.memory_entry_id
        WHERE m.is_tombstone = FALSE
        GROUP BY ms.source_kind
        ORDER BY count DESC, ms.source_kind ASC
        "#
    );
    let rows = sqlx::query(&query).bind(slug).fetch_all(pool).await?;

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
    let query = format!(
        r#"
        WITH {LATEST_PROJECT_MEMORY_IDS_CTE}
        SELECT mt.tag AS name, COUNT(*) AS count
        FROM memory_tags mt
        JOIN latest m ON m.id = mt.memory_entry_id
        WHERE m.is_tombstone = FALSE
        GROUP BY mt.tag
        ORDER BY count DESC, mt.tag ASC
        LIMIT 5
        "#
    );
    let rows = sqlx::query(&query).bind(slug).fetch_all(pool).await?;

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
    let query = format!(
        r#"
        WITH {LATEST_PROJECT_MEMORY_IDS_CTE}
        SELECT ms.file_path AS name, COUNT(*) AS count
        FROM memory_sources ms
        JOIN latest m ON m.id = ms.memory_entry_id
        WHERE m.is_tombstone = FALSE
          AND ms.file_path IS NOT NULL
        GROUP BY ms.file_path
        ORDER BY count DESC, ms.file_path ASC
        LIMIT 5
        "#
    );
    let rows = sqlx::query(&query).bind(slug).fetch_all(pool).await?;

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
    embeddings: Option<&mem_api::EmbeddingBackendConfig>,
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
        active_embedding_provider: embeddings
            .filter(|cfg| !cfg.provider.trim().is_empty() && !cfg.model.trim().is_empty())
            .map(|cfg| cfg.provider.clone()),
        active_embedding_model: embeddings
            .filter(|cfg| !cfg.provider.trim().is_empty() && !cfg.model.trim().is_empty())
            .map(|cfg| cfg.model.clone()),
        memory_type_breakdown,
        source_kind_breakdown,
        top_tags,
        top_files,
        pending_replacement_proposals: 0,
        automation,
        watchers: None,
    }
}

async fn fetch_pending_replacement_proposals(
    pool: &PgPool,
    slug: &str,
) -> Result<i64, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT COUNT(mrp.id) AS pending_replacement_proposals
        FROM memory_replacement_proposals mrp
        JOIN projects p ON p.id = mrp.project_id
        WHERE p.slug = $1
          AND mrp.status = 'pending'
        "#,
    )
    .bind(slug)
    .fetch_one(pool)
    .await?;
    row.try_get("pending_replacement_proposals")
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
    config: Option<&mem_api::EmbeddingBackendConfig>,
) -> Result<EmbeddingHealth, sqlx::Error> {
    let active_provider = config.map(|c| c.provider.trim()).unwrap_or("");
    let active_model = config.map(|c| c.model.trim()).unwrap_or("");
    let active_base_url = config
        .and_then(|c| effective_embedding_base_url(active_provider, &c.base_url))
        .unwrap_or_default();
    let active_space = if active_provider.is_empty() || active_model.is_empty() {
        None
    } else {
        Some(format!(
            "{active_provider}|{active_base_url}|{active_model}"
        ))
    };

    let row = sqlx::query(
        r#"
        WITH active_chunks AS (
            SELECT mc.id
            FROM memory_chunks mc
            JOIN memory_entries m ON m.id = mc.memory_entry_id
            JOIN projects p ON p.id = m.project_id
            WHERE p.slug = $1
              AND m.status = 'active'
        ),
        chunk_embedding_status AS (
            SELECT
                ac.id AS chunk_id,
                COUNT(mce.chunk_id) AS embedding_count,
                COALESCE(BOOL_OR(mce.embedding_space = $2), false) AS has_active_space
            FROM active_chunks ac
            LEFT JOIN memory_chunk_embeddings mce ON mce.chunk_id = ac.id
            GROUP BY ac.id
        ),
        space_count AS (
            SELECT COUNT(DISTINCT mce.embedding_space) AS embedding_spaces_total
            FROM active_chunks ac
            JOIN memory_chunk_embeddings mce ON mce.chunk_id = ac.id
        )
        SELECT
            COUNT(*) AS embedding_chunks_total,
            COUNT(*) FILTER (
                WHERE $2::text IS NOT NULL
                  AND has_active_space
            ) AS fresh_embedding_chunks,
            COUNT(*) FILTER (
                WHERE embedding_count = 0
            ) AS missing_embedding_chunks,
            COUNT(*) FILTER (
                WHERE $2::text IS NOT NULL
                  AND NOT has_active_space
                  AND embedding_count > 0
            ) AS stale_embedding_chunks,
            COALESCE((SELECT embedding_spaces_total FROM space_count), 0) AS embedding_spaces_total
        FROM chunk_embedding_status
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
        let Some(pool) = test_pool().await else {
            return;
        };
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
        let Some(pool) = test_pool().await else {
            return;
        };
        let slug = format!("test-{}", Uuid::new_v4());
        seed_project(&pool, &slug).await.unwrap();

        let overview =
            fetch_project_overview(&pool, &slug, &mem_api::AutomationConfig::default(), None)
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

    #[tokio::test]
    async fn project_views_use_latest_non_tombstone_versions() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let slug = format!("test-{}", Uuid::new_v4());
        let seed = seed_project(&pool, &slug).await.unwrap();
        let updated_id = Uuid::new_v4();
        let deleted_id = Uuid::new_v4();
        let tombstone_id = Uuid::new_v4();

        sqlx::query(
            r#"
            INSERT INTO memory_entries
                (id, project_id, canonical_id, version_no, is_tombstone, canonical_text, summary, memory_type, scope, importance, confidence, status, created_at, updated_at, archived_at, search_document)
            VALUES
                ($1, $2, $3, 2, FALSE, 'Updated canonical memory.', 'Updated memory', 'decision', 'project', 4, 0.9, 'active', now(), now(), NULL, to_tsvector('english', 'Updated canonical memory Updated memory'))
            "#,
        )
        .bind(updated_id)
        .bind(seed.project_id)
        .bind(seed.memory_id)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO memory_tags (memory_entry_id, tag) VALUES ($1, 'beta')")
            .bind(updated_id)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO memory_sources (id, memory_entry_id, task_id, file_path, git_commit, source_kind, excerpt, created_at) VALUES ($1, $2, $3, 'docs/updated.md', NULL, 'file', 'updated source', now())",
        )
        .bind(Uuid::new_v4())
        .bind(updated_id)
        .bind(seed.task_id)
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            r#"
            INSERT INTO memory_entries
                (id, project_id, canonical_id, version_no, is_tombstone, canonical_text, summary, memory_type, scope, importance, confidence, status, created_at, updated_at, archived_at, search_document)
            VALUES
                ($1, $2, $1, 1, FALSE, 'Deleted canonical memory.', 'Deleted memory', 'architecture', 'project', 1, 0.8, 'active', now(), now(), NULL, to_tsvector('english', 'Deleted canonical memory Deleted memory')),
                ($3, $2, $1, 2, TRUE, '', '', 'implementation', 'project', 0, 0.0, 'active', now(), now(), NULL, to_tsvector('english', ''))
            "#,
        )
        .bind(deleted_id)
        .bind(seed.project_id)
        .bind(tombstone_id)
        .execute(&pool)
        .await
        .unwrap();

        let memories = fetch_project_memories(&pool, &slug, Some("active"), 50, 0)
            .await
            .unwrap();
        assert_eq!(memories.total, 1);
        assert_eq!(memories.items.len(), 1);
        assert_eq!(memories.items[0].id, updated_id);
        assert_eq!(memories.items[0].summary, "Updated memory");
        assert!(memories.items[0].tags.contains(&"beta".to_string()));
        assert!(!memories.items[0].tags.contains(&"alpha".to_string()));

        let overview =
            fetch_project_overview(&pool, &slug, &mem_api::AutomationConfig::default(), None)
                .await
                .unwrap();
        assert_eq!(overview.memory_entries_total, 1);
        assert_eq!(overview.active_memories, 1);
        assert_eq!(overview.memory_type_breakdown.len(), 1);
        assert_eq!(
            overview.memory_type_breakdown[0].memory_type,
            mem_api::MemoryType::Decision
        );
        assert_eq!(overview.top_tags[0].name, "beta");
        assert_eq!(overview.top_files[0].name, "docs/updated.md");

        cleanup_project(&pool, &slug).await.unwrap();
    }

    async fn test_pool() -> Option<PgPool> {
        let url = std::env::var("MEMORY_LAYER_TEST_DATABASE_URL")
            .ok()
            .or_else(read_local_test_database_url)?;
        let pool = PgPool::connect(&url).await.unwrap();
        sqlx::migrate!("../../migrations").run(&pool).await.unwrap();
        Some(pool)
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

    struct SeededProject {
        project_id: Uuid,
        task_id: Uuid,
        memory_id: Uuid,
    }

    async fn seed_project(pool: &PgPool, slug: &str) -> Result<SeededProject, sqlx::Error> {
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
                "INSERT INTO memory_entries (id, project_id, canonical_id, version_no, is_tombstone, canonical_text, summary, memory_type, scope, importance, confidence, status, created_at, updated_at, archived_at, search_document) VALUES ($1, $2, $1, 1, FALSE, $3, $4, 'architecture', 'project', 3, 0.85, 'active', now(), now(), NULL, to_tsvector('english', $3 || ' ' || $4))",
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
        Ok(SeededProject {
            project_id,
            task_id,
            memory_id,
        })
    }
}
