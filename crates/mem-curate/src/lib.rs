use mem_api::{
    AppliedMemoryReplacement, CaptureTaskResponse, CurateRequest, CurateResponse,
    MemoryRelationType, ReplacementPolicy, ReplacementProposalListResponse,
    ReplacementProposalRecord, ReplacementProposalResolutionResponse,
};
use mem_ingest::{CandidateAssertion, extract_candidates, idempotency_key};
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
        INSERT INTO sessions (id, project_id, external_session_id, started_at, writer_id, writer_name)
        VALUES ($1, $2, $3, now(), $4, $5)
        "#,
    )
    .bind(session_id)
    .bind(project_id)
    .bind(&request.writer_id)
    .bind(&request.writer_id)
    .bind(
        request
            .writer_name
            .clone()
            .unwrap_or_else(|| request.writer_id.clone()),
    )
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
            dry_run: false,
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
        dry_run: false,
    })
}

pub async fn preview_capture(
    pool: &PgPool,
    request: &CaptureTaskRequest,
) -> Result<CaptureTaskResponse, sqlx::Error> {
    let project_id = sqlx::query("SELECT id FROM projects WHERE slug = $1 LIMIT 1")
        .bind(&request.project)
        .fetch_optional(pool)
        .await?
        .and_then(|row| row.try_get("id").ok())
        .unwrap_or_else(Uuid::nil);
    Ok(CaptureTaskResponse {
        project_id,
        session_id: Uuid::nil(),
        task_id: Uuid::nil(),
        raw_capture_id: Uuid::nil(),
        idempotency_key: idempotency_key(request),
        dry_run: true,
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
    let mut replaced_count = 0_i64;
    let mut proposal_count = 0_i64;
    let mut replacements = Vec::new();
    let policy = request.replacement_policy.unwrap_or_default();

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
                match determine_replacement_decision(&mut tx, project_id, &candidate, policy)
                    .await?
                {
                    ReplacementDecision::InsertNew => {
                        let memory_id =
                            insert_candidate_memory(&mut tx, project_id, &candidate).await?;
                        output_count += 1;
                        memory_id
                    }
                    ReplacementDecision::Replace {
                        target,
                        score: _score,
                        reasons: _reasons,
                    } => {
                        let memory_id =
                            insert_candidate_memory(&mut tx, project_id, &candidate).await?;
                        sqlx::query("DELETE FROM memory_entries WHERE id = $1")
                            .bind(target.id)
                            .execute(&mut *tx)
                            .await?;
                        output_count += 1;
                        replaced_count += 1;
                        replacements.push(AppliedMemoryReplacement {
                            old_memory_id: target.id,
                            old_summary: target.summary,
                            new_memory_id: memory_id,
                            new_summary: candidate.summary.clone(),
                            automatic: true,
                            policy,
                        });
                        memory_id
                    }
                    ReplacementDecision::Queue {
                        target,
                        score,
                        reasons,
                    } => {
                        queue_replacement_proposal(
                            &mut tx, project_id, capture_id, task_id, &candidate, &target, policy,
                            score, &reasons,
                        )
                        .await?;
                        proposal_count += 1;
                        continue;
                    }
                }
            };

            attach_candidate_metadata(&mut tx, memory_id, task_id, &candidate).await?;
            rebuild_memory_chunks(&mut tx, memory_id).await?;

            refresh_relations(&mut tx, project_id, memory_id).await?;
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
        replaced_count,
        proposal_count,
        replacements,
        dry_run: false,
    })
}

pub async fn preview_curate(
    pool: &PgPool,
    request: &CurateRequest,
) -> Result<CurateResponse, sqlx::Error> {
    let project_row = sqlx::query("SELECT id FROM projects WHERE slug = $1")
        .bind(&request.project)
        .fetch_one(pool)
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
    .fetch_all(pool)
    .await?;

    let mut tx = pool.begin().await?;
    let mut output_count = 0_i64;
    let mut replaced_count = 0_i64;
    let mut proposal_count = 0_i64;
    let policy = request.replacement_policy.unwrap_or_default();

    for capture in &captures {
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

            if existing.is_some() {
                continue;
            }

            match determine_replacement_decision(&mut tx, project_id, &candidate, policy).await? {
                ReplacementDecision::InsertNew => output_count += 1,
                ReplacementDecision::Replace { .. } => {
                    output_count += 1;
                    replaced_count += 1;
                }
                ReplacementDecision::Queue { .. } => proposal_count += 1,
            }
        }
    }

    tx.rollback().await?;

    Ok(CurateResponse {
        project_id,
        run_id: Uuid::nil(),
        input_count: captures.len() as i64,
        output_count,
        replaced_count,
        proposal_count,
        replacements: Vec::new(),
        dry_run: true,
    })
}

#[derive(Debug)]
enum ReplacementDecision {
    InsertNew,
    Replace {
        target: MemoryProfile,
        score: i32,
        reasons: Vec<String>,
    },
    Queue {
        target: MemoryProfile,
        score: i32,
        reasons: Vec<String>,
    },
}

async fn determine_replacement_decision(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: Uuid,
    candidate: &CandidateAssertion,
    policy: ReplacementPolicy,
) -> Result<ReplacementDecision, sqlx::Error> {
    if candidate.memory_type == mem_api::MemoryType::Plan {
        if let Some(thread_tag) = plan_thread_tag(candidate) {
            if let Some(target) =
                load_existing_plan_for_thread(tx, project_id, thread_tag, candidate).await?
            {
                return Ok(ReplacementDecision::Replace {
                    target,
                    score: i32::MAX,
                    reasons: vec!["same plan thread".to_string()],
                });
            }
        }
    }

    let profiles = load_candidate_replacement_targets(tx, project_id, candidate).await?;
    let mut scored = profiles
        .into_iter()
        .filter_map(|target| score_replacement_candidate(candidate, target))
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| right.score.cmp(&left.score));
    let Some(best) = scored.first() else {
        return Ok(ReplacementDecision::InsertNew);
    };

    let margin = scored
        .get(1)
        .map(|next| best.score - next.score)
        .unwrap_or(i32::MAX);
    let explicit_update = best
        .reasons
        .iter()
        .any(|reason| reason == "explicit update language");

    let auto = match policy {
        ReplacementPolicy::Conservative => explicit_update && best.score >= 10,
        ReplacementPolicy::Balanced => explicit_update && best.score >= 9 && margin >= 2,
        ReplacementPolicy::Aggressive => best.score >= 8 && margin >= 2,
    };
    if auto {
        return Ok(ReplacementDecision::Replace {
            target: best.profile.clone(),
            score: best.score,
            reasons: best.reasons.clone(),
        });
    }

    let queue = match policy {
        ReplacementPolicy::Conservative => false,
        ReplacementPolicy::Balanced => {
            (7..=8).contains(&best.score) || (explicit_update && best.score >= 9 && margin < 2)
        }
        ReplacementPolicy::Aggressive => {
            (6..=7).contains(&best.score) || (best.score >= 8 && margin < 2)
        }
    };
    if queue {
        return Ok(ReplacementDecision::Queue {
            target: best.profile.clone(),
            score: best.score,
            reasons: best.reasons.clone(),
        });
    }

    Ok(ReplacementDecision::InsertNew)
}

fn plan_thread_tag(candidate: &CandidateAssertion) -> Option<&str> {
    candidate
        .tags
        .iter()
        .find_map(|tag| tag.strip_prefix("plan-thread:"))
        .filter(|tag| !tag.trim().is_empty())
}

async fn load_existing_plan_for_thread(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: Uuid,
    thread_tag: &str,
    candidate: &CandidateAssertion,
) -> Result<Option<MemoryProfile>, sqlx::Error> {
    let full_tag = format!("plan-thread:{thread_tag}");
    sqlx::query(
        r#"
        SELECT
            m.id,
            m.summary,
            m.canonical_text,
            m.memory_type,
            m.scope,
            COALESCE((
                SELECT ARRAY_AGG(mt.tag ORDER BY mt.tag)
                FROM memory_tags mt
                WHERE mt.memory_entry_id = m.id
            ), ARRAY[]::text[]) AS tags,
            COALESCE((
                SELECT ARRAY_AGG(ms.file_path ORDER BY ms.file_path)
                FROM memory_sources ms
                WHERE ms.memory_entry_id = m.id
                  AND ms.file_path IS NOT NULL
            ), ARRAY[]::text[]) AS files
        FROM memory_entries m
        JOIN memory_tags thread_tag
          ON thread_tag.memory_entry_id = m.id
         AND thread_tag.tag = $3
        LEFT JOIN imported_memory_entries ime ON ime.memory_entry_id = m.id
        WHERE m.project_id = $1
          AND m.status = 'active'
          AND ime.memory_entry_id IS NULL
          AND m.memory_type = $2
          AND m.scope = 'project'
          AND lower(m.canonical_text) <> lower($4)
        ORDER BY m.updated_at DESC
        LIMIT 1
        "#,
    )
    .bind(project_id)
    .bind(candidate.memory_type.to_string())
    .bind(full_tag)
    .bind(&candidate.canonical_text)
    .fetch_optional(&mut **tx)
    .await?
    .map(|row| {
        Ok(MemoryProfile {
            id: row.try_get("id")?,
            summary: row.try_get("summary")?,
            canonical_text: row.try_get("canonical_text")?,
            memory_type: row.try_get("memory_type")?,
            scope: row.try_get("scope")?,
            tags: row.try_get("tags")?,
            files: row.try_get("files")?,
        })
    })
    .transpose()
}

async fn load_candidate_replacement_targets(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: Uuid,
    candidate: &CandidateAssertion,
) -> Result<Vec<MemoryProfile>, sqlx::Error> {
    sqlx::query(
        r#"
        SELECT
            m.id,
            m.summary,
            m.canonical_text,
            m.memory_type,
            m.scope,
            COALESCE((
                SELECT ARRAY_AGG(mt.tag ORDER BY mt.tag)
                FROM memory_tags mt
                WHERE mt.memory_entry_id = m.id
            ), ARRAY[]::text[]) AS tags,
            COALESCE((
                SELECT ARRAY_AGG(ms.file_path ORDER BY ms.file_path)
                FROM memory_sources ms
                WHERE ms.memory_entry_id = m.id
                  AND ms.file_path IS NOT NULL
            ), ARRAY[]::text[]) AS files
        FROM memory_entries m
        LEFT JOIN imported_memory_entries ime ON ime.memory_entry_id = m.id
        WHERE m.project_id = $1
          AND m.status = 'active'
          AND ime.memory_entry_id IS NULL
          AND m.memory_type = $2
          AND m.scope = 'project'
        "#,
    )
    .bind(project_id)
    .bind(candidate.memory_type.to_string())
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .map(|row| {
        Ok(MemoryProfile {
            id: row.try_get("id")?,
            summary: row.try_get("summary")?,
            canonical_text: row.try_get("canonical_text")?,
            memory_type: row.try_get("memory_type")?,
            scope: row.try_get("scope")?,
            tags: row.try_get("tags")?,
            files: row.try_get("files")?,
        })
    })
    .collect()
}

async fn insert_candidate_memory(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: Uuid,
    candidate: &CandidateAssertion,
) -> Result<Uuid, sqlx::Error> {
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
    .execute(&mut **tx)
    .await?;
    Ok(memory_id)
}

async fn attach_candidate_metadata(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    memory_id: Uuid,
    task_id: Uuid,
    candidate: &CandidateAssertion,
) -> Result<(), sqlx::Error> {
    for tag in &candidate.tags {
        sqlx::query(
            "INSERT INTO memory_tags (memory_entry_id, tag) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(memory_id)
        .bind(tag)
        .execute(&mut **tx)
        .await?;
    }

    for source in &candidate.sources {
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
        .bind(&source.file_path)
        .bind(source_kind)
        .bind(&source.excerpt)
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

async fn rebuild_memory_chunks(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    memory_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM memory_chunks WHERE memory_entry_id = $1")
        .bind(memory_id)
        .execute(&mut **tx)
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
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn queue_replacement_proposal(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: Uuid,
    raw_capture_id: Uuid,
    task_id: Uuid,
    candidate: &CandidateAssertion,
    target: &MemoryProfile,
    policy: ReplacementPolicy,
    score: i32,
    reasons: &[String],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO memory_replacement_proposals
            (id, project_id, target_memory_id, task_id, raw_capture_id, candidate_json, policy, score, rationale_json, status, created_at, resolved_at)
        VALUES
            ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'pending', now(), NULL)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(target.id)
    .bind(task_id)
    .bind(raw_capture_id)
    .bind(sqlx::types::Json(candidate))
    .bind(policy.to_string())
    .bind(score)
    .bind(sqlx::types::Json(reasons))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn refresh_memory_relations(
    pool: &PgPool,
    project: &str,
    memory_id: Uuid,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let project_row = sqlx::query("SELECT id FROM projects WHERE slug = $1")
        .bind(project)
        .fetch_one(&mut *tx)
        .await?;
    let project_id: Uuid = project_row.try_get("id")?;
    refresh_relations(&mut tx, project_id, memory_id).await?;
    tx.commit().await?;
    Ok(())
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

#[derive(Debug, Clone)]
struct MemoryProfile {
    id: Uuid,
    summary: String,
    canonical_text: String,
    memory_type: String,
    scope: String,
    tags: Vec<String>,
    files: Vec<String>,
}

#[derive(Debug, Clone)]
struct ScoredReplacement {
    profile: MemoryProfile,
    score: i32,
    reasons: Vec<String>,
}

async fn refresh_relations(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: Uuid,
    memory_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM memory_relations WHERE src_memory_id = $1 OR dst_memory_id = $1")
        .bind(memory_id)
        .execute(&mut **tx)
        .await?;

    let current = load_memory_profile(tx, memory_id).await?;
    let Some(current) = current else {
        return Ok(());
    };

    let others = sqlx::query(
        r#"
        SELECT
            m.id,
            m.summary,
            m.canonical_text,
            COALESCE((
                SELECT ARRAY_AGG(mt.tag ORDER BY mt.tag)
                FROM memory_tags mt
                WHERE mt.memory_entry_id = m.id
            ), ARRAY[]::text[]) AS tags,
            COALESCE((
                SELECT ARRAY_AGG(ms.file_path ORDER BY ms.file_path)
                FROM memory_sources ms
                WHERE ms.memory_entry_id = m.id
                  AND ms.file_path IS NOT NULL
            ), ARRAY[]::text[]) AS files
        FROM memory_entries m
        WHERE m.project_id = $1
          AND m.id <> $2
          AND m.status = 'active'
        "#,
    )
    .bind(project_id)
    .bind(memory_id)
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .map(|row| {
        Ok(MemoryProfile {
            id: row.try_get("id")?,
            summary: row.try_get("summary")?,
            canonical_text: row.try_get("canonical_text")?,
            memory_type: "unknown".to_string(),
            scope: "project".to_string(),
            tags: row.try_get("tags")?,
            files: row.try_get("files")?,
        })
    })
    .collect::<Result<Vec<_>, sqlx::Error>>()?;

    for other in others {
        if let Some(relation) = classify_relation(&current, &other) {
            insert_relation(tx, current.id, other.id, relation.clone()).await?;
            if matches!(
                relation,
                MemoryRelationType::Duplicates
                    | MemoryRelationType::RelatedTo
                    | MemoryRelationType::Supports
            ) {
                insert_relation(tx, other.id, current.id, relation).await?;
            }
        }
    }

    Ok(())
}

async fn load_memory_profile(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    memory_id: Uuid,
) -> Result<Option<MemoryProfile>, sqlx::Error> {
    sqlx::query(
        r#"
        SELECT
            m.id,
            m.summary,
            m.canonical_text,
            COALESCE((
                SELECT ARRAY_AGG(mt.tag ORDER BY mt.tag)
                FROM memory_tags mt
                WHERE mt.memory_entry_id = m.id
            ), ARRAY[]::text[]) AS tags,
            COALESCE((
                SELECT ARRAY_AGG(ms.file_path ORDER BY ms.file_path)
                FROM memory_sources ms
                WHERE ms.memory_entry_id = m.id
                  AND ms.file_path IS NOT NULL
            ), ARRAY[]::text[]) AS files
        FROM memory_entries m
        WHERE m.id = $1
        "#,
    )
    .bind(memory_id)
    .fetch_optional(&mut **tx)
    .await?
    .map(|row| {
        Ok(MemoryProfile {
            id: row.try_get("id")?,
            summary: row.try_get("summary")?,
            canonical_text: row.try_get("canonical_text")?,
            memory_type: "unknown".to_string(),
            scope: "project".to_string(),
            tags: row.try_get("tags")?,
            files: row.try_get("files")?,
        })
    })
    .transpose()
}

async fn insert_relation(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    src_memory_id: Uuid,
    dst_memory_id: Uuid,
    relation_type: MemoryRelationType,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO memory_relations (id, src_memory_id, relation_type, dst_memory_id)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(src_memory_id)
    .bind(relation_type.to_string())
    .bind(dst_memory_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn classify_relation(current: &MemoryProfile, other: &MemoryProfile) -> Option<MemoryRelationType> {
    let current_text = normalize_text(&current.canonical_text);
    let other_text = normalize_text(&other.canonical_text);
    let similarity = token_overlap_ratio(&current_text, &other_text);
    let shared_tags = overlap_count(&current.tags, &other.tags);
    let shared_files = overlap_count(&current.files, &other.files);

    if similarity >= 0.92 {
        return Some(MemoryRelationType::Duplicates);
    }

    if has_supersedes_language(&current.canonical_text)
        && (similarity >= 0.45 || shared_tags > 0 || shared_files > 0)
    {
        return Some(MemoryRelationType::Supersedes);
    }

    if has_dependency_language(&current.canonical_text)
        && (similarity >= 0.30 || shared_tags > 0 || shared_files > 0)
    {
        return Some(MemoryRelationType::DependsOn);
    }

    if shared_files > 0 || (shared_tags >= 2 && similarity >= 0.25) {
        return Some(MemoryRelationType::Supports);
    }

    if similarity >= 0.28 || shared_tags > 0 || shared_files > 0 {
        return Some(MemoryRelationType::RelatedTo);
    }

    None
}

fn score_replacement_candidate(
    candidate: &CandidateAssertion,
    target: MemoryProfile,
) -> Option<ScoredReplacement> {
    if target.memory_type != candidate.memory_type.to_string() || target.scope != "project" {
        return None;
    }

    let candidate_tokens = normalize_text(&candidate.canonical_text);
    let target_tokens = normalize_text(&target.canonical_text);
    let canonical_overlap = token_overlap_ratio(&candidate_tokens, &target_tokens);
    let candidate_summary_tokens = normalize_text(&candidate.summary);
    let target_summary_tokens = normalize_text(&target.summary);
    let summary_overlap = token_overlap_ratio(&candidate_summary_tokens, &target_summary_tokens);
    let candidate_files = candidate
        .sources
        .iter()
        .filter_map(|source| source.file_path.clone())
        .collect::<Vec<_>>();
    let shared_tags = overlap_count(&candidate.tags, &target.tags);
    let shared_files = overlap_count(&candidate_files, &target.files);

    if canonical_overlap < 0.45 && shared_files == 0 && shared_tags == 0 {
        return None;
    }

    let mut score = 0;
    let mut reasons = Vec::new();

    if canonical_overlap >= 0.75 {
        score += 4;
        reasons.push("canonical overlap >= 0.75".to_string());
    } else if canonical_overlap >= 0.60 {
        score += 3;
        reasons.push("canonical overlap >= 0.60".to_string());
    } else if canonical_overlap >= 0.45 {
        score += 2;
        reasons.push("canonical overlap >= 0.45".to_string());
    }

    if summary_overlap >= 0.70 {
        score += 2;
        reasons.push("summary overlap >= 0.70".to_string());
    } else if summary_overlap >= 0.55 {
        score += 1;
        reasons.push("summary overlap >= 0.55".to_string());
    }

    if shared_files > 0 {
        score += 3;
        reasons.push("shared source files".to_string());
    }

    if shared_tags >= 2 {
        score += 2;
        reasons.push("shared tags >= 2".to_string());
    } else if shared_tags == 1 {
        score += 1;
        reasons.push("shared tag".to_string());
    }

    if has_explicit_update_language(&candidate.summary)
        || has_explicit_update_language(&candidate.canonical_text)
    {
        score += 3;
        reasons.push("explicit update language".to_string());
    }

    Some(ScoredReplacement {
        profile: target,
        score,
        reasons,
    })
}

fn normalize_text(text: &str) -> Vec<String> {
    text.split(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '/')))
        .filter_map(|segment| {
            let value = segment.trim().to_ascii_lowercase();
            (value.len() >= 3).then_some(value)
        })
        .collect()
}

fn token_overlap_ratio(left: &[String], right: &[String]) -> f32 {
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let left = left
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let right = right
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let shared = left.intersection(&right).count() as f32;
    let total = left.union(&right).count() as f32;
    if total == 0.0 { 0.0 } else { shared / total }
}

fn overlap_count(left: &[String], right: &[String]) -> usize {
    let left = left
        .iter()
        .map(|value| value.to_ascii_lowercase())
        .collect::<std::collections::BTreeSet<_>>();
    let right = right
        .iter()
        .map(|value| value.to_ascii_lowercase())
        .collect::<std::collections::BTreeSet<_>>();
    left.intersection(&right).count()
}

fn has_supersedes_language(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    [
        "replace",
        "replaces",
        "supersede",
        "supersedes",
        "deprecate",
        "deprecated",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
}

fn has_dependency_language(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    ["depends on", "requires", "relies on"]
        .iter()
        .any(|needle| lowered.contains(needle))
}

fn has_explicit_update_language(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    [
        "replace",
        "replaced",
        "supersede",
        "superseded",
        "migrate",
        "migrated",
        "now uses",
        "no longer",
        "deprecated",
        "instead of",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
}

pub async fn list_replacement_proposals(
    pool: &PgPool,
    project: &str,
) -> Result<ReplacementProposalListResponse, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT
            mrp.id,
            m.id AS target_memory_id,
            m.summary AS target_summary,
            mrp.candidate_json,
            mrp.policy,
            mrp.score,
            mrp.rationale_json,
            mrp.created_at
        FROM memory_replacement_proposals mrp
        JOIN projects p ON p.id = mrp.project_id
        JOIN memory_entries m ON m.id = mrp.target_memory_id
        WHERE p.slug = $1
          AND mrp.status = 'pending'
        ORDER BY mrp.created_at DESC
        "#,
    )
    .bind(project)
    .fetch_all(pool)
    .await?;

    let proposals = rows
        .into_iter()
        .map(|row| {
            let candidate: sqlx::types::Json<CandidateAssertion> = row.try_get("candidate_json")?;
            let policy_value: String = row.try_get("policy")?;
            let policy = match policy_value.as_str() {
                "conservative" => ReplacementPolicy::Conservative,
                "aggressive" => ReplacementPolicy::Aggressive,
                _ => ReplacementPolicy::Balanced,
            };
            let reasons_json: sqlx::types::Json<Vec<String>> = row.try_get("rationale_json")?;
            Ok(ReplacementProposalRecord {
                id: row.try_get("id")?,
                project: project.to_string(),
                target_memory_id: row.try_get("target_memory_id")?,
                target_summary: row.try_get("target_summary")?,
                candidate_summary: candidate.0.summary.clone(),
                candidate_canonical_text: candidate.0.canonical_text.clone(),
                candidate_memory_type: candidate.0.memory_type.clone(),
                score: row.try_get("score")?,
                policy,
                reasons: reasons_json.0,
                created_at: row.try_get("created_at")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;

    Ok(ReplacementProposalListResponse {
        project: project.to_string(),
        proposals,
    })
}

pub async fn approve_replacement_proposal(
    pool: &PgPool,
    project: &str,
    proposal_id: Uuid,
) -> Result<ReplacementProposalResolutionResponse, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let row = sqlx::query(
        r#"
        SELECT
            mrp.target_memory_id,
            m.summary AS target_summary,
            mrp.task_id,
            mrp.candidate_json,
            mrp.policy
        FROM memory_replacement_proposals mrp
        JOIN projects p ON p.id = mrp.project_id
        JOIN memory_entries m ON m.id = mrp.target_memory_id
        WHERE p.slug = $1
          AND mrp.id = $2
          AND mrp.status = 'pending'
        "#,
    )
    .bind(project)
    .bind(proposal_id)
    .fetch_one(&mut *tx)
    .await?;
    let target_memory_id: Uuid = row.try_get("target_memory_id")?;
    let target_summary: String = row.try_get("target_summary")?;
    let task_id: Uuid = row.try_get("task_id")?;
    let candidate: sqlx::types::Json<CandidateAssertion> = row.try_get("candidate_json")?;
    let policy = match row.try_get::<String, _>("policy")?.as_str() {
        "conservative" => ReplacementPolicy::Conservative,
        "aggressive" => ReplacementPolicy::Aggressive,
        _ => ReplacementPolicy::Balanced,
    };

    let project_row = sqlx::query("SELECT id FROM projects WHERE slug = $1")
        .bind(project)
        .fetch_one(&mut *tx)
        .await?;
    let project_id: Uuid = project_row.try_get("id")?;

    let new_memory_id = insert_candidate_memory(&mut tx, project_id, &candidate.0).await?;
    attach_candidate_metadata(&mut tx, new_memory_id, task_id, &candidate.0).await?;
    rebuild_memory_chunks(&mut tx, new_memory_id).await?;
    sqlx::query(
        "UPDATE memory_replacement_proposals SET status = 'approved', resolved_at = now() WHERE id = $1",
    )
    .bind(proposal_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM memory_entries WHERE id = $1")
        .bind(target_memory_id)
        .execute(&mut *tx)
        .await?;
    refresh_relations(&mut tx, project_id, new_memory_id).await?;
    tx.commit().await?;

    Ok(ReplacementProposalResolutionResponse {
        project: project.to_string(),
        proposal_id,
        status: "approved".to_string(),
        policy,
        target_memory_id,
        target_summary,
        candidate_summary: candidate.0.summary.clone(),
        new_memory_id: Some(new_memory_id),
    })
}

pub async fn reject_replacement_proposal(
    pool: &PgPool,
    project: &str,
    proposal_id: Uuid,
) -> Result<ReplacementProposalResolutionResponse, sqlx::Error> {
    let row = sqlx::query(
        r#"
        UPDATE memory_replacement_proposals mrp
        SET status = 'rejected', resolved_at = now()
        FROM projects p, memory_entries m
        WHERE p.id = mrp.project_id
          AND m.id = mrp.target_memory_id
          AND p.slug = $1
          AND mrp.id = $2
          AND mrp.status = 'pending'
        RETURNING mrp.target_memory_id, m.summary AS target_summary, mrp.candidate_json, mrp.policy
        "#,
    )
    .bind(project)
    .bind(proposal_id)
    .fetch_one(pool)
    .await?;
    let candidate: sqlx::types::Json<CandidateAssertion> = row.try_get("candidate_json")?;
    let policy = match row.try_get::<String, _>("policy")?.as_str() {
        "conservative" => ReplacementPolicy::Conservative,
        "aggressive" => ReplacementPolicy::Aggressive,
        _ => ReplacementPolicy::Balanced,
    };
    Ok(ReplacementProposalResolutionResponse {
        project: project.to_string(),
        proposal_id,
        status: "rejected".to_string(),
        policy,
        target_memory_id: row.try_get("target_memory_id")?,
        target_summary: row.try_get("target_summary")?,
        candidate_summary: candidate.0.summary.clone(),
        new_memory_id: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(canonical_text: &str, tags: &[&str], files: &[&str]) -> MemoryProfile {
        MemoryProfile {
            id: Uuid::new_v4(),
            summary: canonical_text.to_string(),
            canonical_text: canonical_text.to_string(),
            memory_type: "convention".to_string(),
            scope: "project".to_string(),
            tags: tags.iter().map(|value| value.to_string()).collect(),
            files: files.iter().map(|value| value.to_string()).collect(),
        }
    }

    #[test]
    fn relation_classifier_detects_duplicates() {
        let left = profile(
            "Memory Layer stores canonical facts in PostgreSQL.",
            &["db"],
            &["src/lib.rs"],
        );
        let right = profile(
            "Memory Layer stores canonical facts in PostgreSQL.",
            &["db"],
            &["src/lib.rs"],
        );
        assert_eq!(
            classify_relation(&left, &right),
            Some(MemoryRelationType::Duplicates)
        );
    }

    #[test]
    fn relation_classifier_detects_related_by_shared_provenance() {
        let left = profile(
            "The query path uses ranked retrieval.",
            &["search", "query"],
            &["crates/mem-search/src/lib.rs"],
        );
        let right = profile(
            "Search ranking explains why each memory matched.",
            &["search"],
            &["crates/mem-search/src/lib.rs"],
        );
        assert_eq!(
            classify_relation(&left, &right),
            Some(MemoryRelationType::Supports)
        );
    }

    #[test]
    fn plan_thread_tag_extracts_thread_key() {
        let candidate = CandidateAssertion {
            canonical_text: "Approved plan".to_string(),
            summary: "Approved plan".to_string(),
            memory_type: mem_api::MemoryType::Plan,
            confidence: 0.95,
            importance: 4,
            tags: vec![
                "plan".to_string(),
                "plan-thread:resume-redesign".to_string(),
            ],
            sources: Vec::new(),
        };

        assert_eq!(plan_thread_tag(&candidate), Some("resume-redesign"));
    }
}
