use mem_api::{CaptureTaskResponse, CurateRequest, CurateResponse};
use mem_ingest::{extract_candidates, idempotency_key};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use mem_api::CaptureTaskRequest;

pub async fn store_capture(
    pool: &PgPool,
    request: &CaptureTaskRequest,
) -> Result<CaptureTaskResponse, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let project_id = upsert_project(&mut tx, &request.project).await?;
    let session_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO sessions (id, project_id, external_session_id, started_at, agent_name)
        VALUES ($1, $2, $3, now(), $4)
        "#,
    )
    .bind(session_id)
    .bind(project_id)
    .bind("local-session")
    .bind("codex")
    .execute(&mut *tx)
    .await?;

    let task_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO tasks (id, session_id, title, user_prompt, task_summary, status, created_at, completed_at)
        VALUES ($1, $2, $3, $4, $5, 'completed', now(), now())
        "#,
    )
    .bind(task_id)
    .bind(session_id)
    .bind(&request.task_title)
    .bind(&request.user_prompt)
    .bind(&request.agent_summary)
    .execute(&mut *tx)
    .await?;

    let computed_key = idempotency_key(request);
    if let Some(existing) =
        sqlx::query("SELECT id, task_id FROM raw_captures WHERE idempotency_key = $1 LIMIT 1")
            .bind(&computed_key)
            .fetch_optional(&mut *tx)
            .await?
    {
        tx.rollback().await?;
        return Ok(CaptureTaskResponse {
            project_id,
            session_id,
            task_id: existing.try_get("task_id")?,
            raw_capture_id: existing.try_get("id")?,
            idempotency_key: computed_key,
        });
    }

    let raw_capture_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO raw_captures (id, task_id, capture_type, payload_json, idempotency_key, created_at, curated_at)
        VALUES ($1, $2, 'task', $3, $4, now(), NULL)
        "#,
    )
    .bind(raw_capture_id)
    .bind(task_id)
    .bind(sqlx::types::Json(request))
    .bind(&computed_key)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(CaptureTaskResponse {
        project_id,
        session_id,
        task_id,
        raw_capture_id,
        idempotency_key: computed_key,
    })
}

pub async fn curate(pool: &PgPool, request: &CurateRequest) -> Result<CurateResponse, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let project_row = sqlx::query("SELECT id FROM projects WHERE slug = $1")
        .bind(&request.project)
        .fetch_one(&mut *tx)
        .await?;
    let project_id: Uuid = project_row.try_get("id")?;

    let limit = request.batch_size.unwrap_or(25);
    let captures = sqlx::query(
        r#"
        SELECT rc.id, rc.task_id, rc.payload_json
        FROM raw_captures rc
        JOIN tasks t ON t.id = rc.task_id
        JOIN sessions s ON s.id = t.session_id
        WHERE s.project_id = $1
          AND rc.curated_at IS NULL
        ORDER BY rc.created_at ASC
        LIMIT $2
        "#,
    )
    .bind(project_id)
    .bind(limit)
    .fetch_all(&mut *tx)
    .await?;

    let run_id = Uuid::new_v4();
    let mut output_count = 0_i64;

    for capture in &captures {
        let capture_id: Uuid = capture.try_get("id")?;
        let task_id: Uuid = capture.try_get("task_id")?;
        let payload: sqlx::types::Json<CaptureTaskRequest> = capture.try_get("payload_json")?;
        for candidate in extract_candidates(&payload.0) {
            let existing = sqlx::query(
                r#"
                SELECT id
                FROM memory_entries
                WHERE project_id = $1
                  AND lower(canonical_text) = lower($2)
                LIMIT 1
                "#,
            )
            .bind(project_id)
            .bind(&candidate.canonical_text)
            .fetch_optional(&mut *tx)
            .await?;

            let memory_id = if let Some(existing) = existing {
                let memory_id: Uuid = existing.try_get("id")?;
                sqlx::query(
                    r#"
                    UPDATE memory_entries
                    SET confidence = GREATEST(confidence, $2),
                        importance = GREATEST(importance, $3),
                        updated_at = now()
                    WHERE id = $1
                    "#,
                )
                .bind(memory_id)
                .bind(candidate.confidence)
                .bind(candidate.importance)
                .execute(&mut *tx)
                .await?;
                memory_id
            } else {
                let memory_id = Uuid::new_v4();
                sqlx::query(
                    r#"
                    INSERT INTO memory_entries
                        (id, project_id, canonical_text, summary, memory_type, scope, importance, confidence, status, created_at, updated_at, archived_at, search_document)
                    VALUES
                        ($1, $2, $3, $4, $5, 'project', $6, $7, 'active', now(), now(), NULL, to_tsvector('english', $3 || ' ' || $4))
                    "#,
                )
                .bind(memory_id)
                .bind(project_id)
                .bind(&candidate.canonical_text)
                .bind(&candidate.summary)
                .bind(candidate.memory_type.to_string())
                .bind(candidate.importance)
                .bind(candidate.confidence)
                .execute(&mut *tx)
                .await?;
                output_count += 1;
                memory_id
            };

            for tag in candidate.tags {
                sqlx::query(
                    "INSERT INTO memory_tags (memory_entry_id, tag) VALUES ($1, $2) ON CONFLICT DO NOTHING",
                )
                .bind(memory_id)
                .bind(tag)
                .execute(&mut *tx)
                .await?;
            }

            for source in candidate.sources {
                let source_kind = source.source_kind_to_string();
                sqlx::query(
                    r#"
                    INSERT INTO memory_sources
                        (id, memory_entry_id, task_id, file_path, git_commit, source_kind, excerpt, created_at)
                    VALUES
                        ($1, $2, $3, $4, NULL, $5, $6, now())
                    "#,
                )
                .bind(Uuid::new_v4())
                .bind(memory_id)
                .bind(task_id)
                .bind(source.file_path)
                .bind(source_kind)
                .bind(source.excerpt)
                .execute(&mut *tx)
                .await?;
            }

            sqlx::query("DELETE FROM memory_chunks WHERE memory_entry_id = $1")
                .bind(memory_id)
                .execute(&mut *tx)
                .await?;
            sqlx::query(
                r#"
                INSERT INTO memory_chunks (id, memory_entry_id, chunk_text, search_text, tsv)
                SELECT $1, id, canonical_text, summary || E'\n' || canonical_text, to_tsvector('english', summary || ' ' || canonical_text)
                FROM memory_entries
                WHERE id = $2
                "#,
            )
            .bind(Uuid::new_v4())
            .bind(memory_id)
            .execute(&mut *tx)
            .await?;
        }

        sqlx::query("UPDATE raw_captures SET curated_at = now() WHERE id = $1")
            .bind(capture_id)
            .execute(&mut *tx)
            .await?;
    }

    sqlx::query(
        r#"
        INSERT INTO curation_runs (id, project_id, trigger_type, input_count, output_count, model_name, created_at)
        VALUES ($1, $2, 'manual', $3, $4, NULL, now())
        "#,
    )
    .bind(run_id)
    .bind(project_id)
    .bind(captures.len() as i64)
    .bind(output_count)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(CurateResponse {
        project_id,
        run_id,
        input_count: captures.len() as i64,
        output_count,
    })
}

async fn upsert_project(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project: &str,
) -> Result<Uuid, sqlx::Error> {
    let existing = sqlx::query("SELECT id FROM projects WHERE slug = $1")
        .bind(project)
        .fetch_optional(&mut **tx)
        .await?;
    if let Some(existing) = existing {
        return existing.try_get("id");
    }

    let id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO projects (id, slug, name, root_path, created_at)
        VALUES ($1, $2, $3, $4, now())
        "#,
    )
    .bind(id)
    .bind(project)
    .bind(project)
    .bind(project)
    .execute(&mut **tx)
    .await?;
    Ok(id)
}

trait SourceKindSql {
    fn source_kind_to_string(&self) -> String;
}

impl SourceKindSql for mem_ingest::CandidateSource {
    fn source_kind_to_string(&self) -> String {
        match self.source_kind {
            mem_api::SourceKind::TaskPrompt => "task_prompt",
            mem_api::SourceKind::File => "file",
            mem_api::SourceKind::GitCommit => "git_commit",
            mem_api::SourceKind::CommandOutput => "command_output",
            mem_api::SourceKind::Test => "test",
            mem_api::SourceKind::Note => "note",
        }
        .to_string()
    }
}
