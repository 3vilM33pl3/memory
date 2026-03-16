use chrono::{DateTime, Utc};
use mem_api::{MemoryType, QueryRequest, QueryResponse, QueryResult, QuerySource, SourceKind};
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub async fn query_memory(
    pool: &PgPool,
    request: &QueryRequest,
) -> Result<QueryResponse, sqlx::Error> {
    let memory_type_filters = request
        .filters
        .types
        .iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>();

    let rows = sqlx::query(
        r#"
        WITH ranked AS (
            SELECT
                m.id,
                m.summary,
                m.memory_type,
                m.importance,
                m.confidence,
                m.updated_at,
                m.canonical_text,
                COALESCE(
                    ts_rank_cd(mc.tsv, websearch_to_tsquery('english', $2)),
                    0
                ) AS fts_score
            FROM memory_entries m
            JOIN projects p ON p.id = m.project_id
            LEFT JOIN memory_chunks mc ON mc.memory_entry_id = m.id
            WHERE p.slug = $1
              AND m.status = 'active'
              AND ($3::text[] IS NULL OR m.memory_type = ANY($3))
              AND (
                    cardinality($4::text[]) = 0
                    OR EXISTS (
                        SELECT 1
                        FROM memory_tags mt
                        WHERE mt.memory_entry_id = m.id
                          AND mt.tag = ANY($4)
                    )
              )
              AND (
                    mc.tsv @@ websearch_to_tsquery('english', $2)
                    OR m.search_document @@ websearch_to_tsquery('english', $2)
              )
        )
        SELECT DISTINCT ON (r.id)
            r.id,
            r.summary,
            r.memory_type,
            r.canonical_text,
            r.importance,
            r.confidence,
            r.updated_at,
            left(r.canonical_text, 240) AS snippet,
            (
                r.fts_score
                + (r.importance * 0.4)
                + (r.confidence * 2.0)
                + CASE WHEN r.canonical_text ILIKE '%' || $2 || '%' THEN 1.2 ELSE 0 END
                + (1.0 / (1.0 + EXTRACT(EPOCH FROM (now() - r.updated_at)) / 86400.0))
            ) AS final_score
        FROM ranked r
        ORDER BY r.id, final_score DESC
        LIMIT $5
        "#,
    )
    .bind(&request.project)
    .bind(&request.query)
    .bind(if memory_type_filters.is_empty() {
        None::<Vec<String>>
    } else {
        Some(memory_type_filters)
    })
    .bind(&request.filters.tags)
    .bind(request.top_k)
    .fetch_all(pool)
    .await?;

    let mut results = Vec::with_capacity(rows.len());
    for row in rows {
        let memory_id: Uuid = row.try_get("id")?;
        let sources = fetch_sources(pool, memory_id).await?;
        let tags = fetch_tags(pool, memory_id).await?;
        let confidence: f32 = row.try_get("confidence")?;
        if request
            .min_confidence
            .is_some_and(|threshold| confidence < threshold)
        {
            continue;
        }

        results.push(QueryResult {
            memory_id,
            summary: row.try_get("summary")?,
            memory_type: parse_memory_type(&row.try_get::<String, _>("memory_type")?),
            score: row.try_get("final_score")?,
            snippet: row.try_get("snippet")?,
            tags,
            sources,
        });
    }

    let top_confidence = results
        .iter()
        .map(|result| result.score)
        .fold(0.0_f64, f64::max)
        .clamp(0.0, 10.0) as f32
        / 10.0;
    let insufficient = results.is_empty() || top_confidence < 0.35;
    let answer = if insufficient {
        "I could not find enough project memory to answer confidently.".to_string()
    } else {
        let top = &results[0];
        format!("{} ({})", top.summary, top.snippet)
    };

    Ok(QueryResponse {
        answer,
        confidence: if insufficient {
            top_confidence.min(0.3)
        } else {
            top_confidence
        },
        results,
        insufficient_evidence: insufficient,
    })
}

pub async fn rebuild_chunks(pool: &PgPool, project: &str) -> Result<u64, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT m.id, m.canonical_text, m.summary
        FROM memory_entries m
        JOIN projects p ON p.id = m.project_id
        WHERE p.slug = $1
        "#,
    )
    .bind(project)
    .fetch_all(pool)
    .await?;

    let mut count = 0_u64;
    for row in rows {
        let memory_id: Uuid = row.try_get("id")?;
        let canonical_text: String = row.try_get("canonical_text")?;
        let summary: String = row.try_get("summary")?;
        sqlx::query("DELETE FROM memory_chunks WHERE memory_entry_id = $1")
            .bind(memory_id)
            .execute(pool)
            .await?;
        sqlx::query(
            r#"
            INSERT INTO memory_chunks (id, memory_entry_id, chunk_text, search_text, tsv)
            VALUES ($1, $2, $3, $4, to_tsvector('english', $4))
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(memory_id)
        .bind(&canonical_text)
        .bind(format!("{summary}\n{canonical_text}"))
        .execute(pool)
        .await?;
        count += 1;
    }
    Ok(count)
}

pub fn parse_memory_type(value: &str) -> MemoryType {
    match value {
        "architecture" => MemoryType::Architecture,
        "convention" => MemoryType::Convention,
        "decision" => MemoryType::Decision,
        "incident" => MemoryType::Incident,
        "debugging" => MemoryType::Debugging,
        "environment" => MemoryType::Environment,
        "domain_fact" => MemoryType::DomainFact,
        _ => MemoryType::Convention,
    }
}

async fn fetch_sources(pool: &PgPool, memory_id: Uuid) -> Result<Vec<QuerySource>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT task_id, file_path, source_kind, excerpt
        FROM memory_sources
        WHERE memory_entry_id = $1
        ORDER BY created_at ASC
        "#,
    )
    .bind(memory_id)
    .fetch_all(pool)
    .await?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let source_kind: String = row.try_get("source_kind")?;
        items.push(QuerySource {
            task_id: row.try_get("task_id")?,
            file_path: row.try_get("file_path")?,
            source_kind: parse_source_kind(&source_kind),
            excerpt: row.try_get("excerpt")?,
        });
    }
    Ok(items)
}

async fn fetch_tags(pool: &PgPool, memory_id: Uuid) -> Result<Vec<String>, sqlx::Error> {
    let rows = sqlx::query("SELECT tag FROM memory_tags WHERE memory_entry_id = $1 ORDER BY tag")
        .bind(memory_id)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|row| row.try_get::<String, _>("tag").ok())
        .collect())
}

pub fn parse_source_kind(value: &str) -> SourceKind {
    match value {
        "task_prompt" => SourceKind::TaskPrompt,
        "file" => SourceKind::File,
        "git_commit" => SourceKind::GitCommit,
        "command_output" => SourceKind::CommandOutput,
        "test" => SourceKind::Test,
        "note" => SourceKind::Note,
        _ => SourceKind::Note,
    }
}

pub fn score_explanation(updated_at: DateTime<Utc>, importance: i32, confidence: f32) -> f64 {
    let age_days = (Utc::now() - updated_at).num_days().max(0) as f64;
    importance as f64 * 0.4 + confidence as f64 * 2.0 + (1.0 / (1.0 + age_days))
}
