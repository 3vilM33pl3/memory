use crate::prelude::*;
use crate::*;

pub async fn fetch_memory_entry(
    pool: &PgPool,
    id: Uuid,
) -> Result<Option<MemoryEntryResponse>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT p.slug, m.id, m.canonical_text, m.summary, m.memory_type, m.importance, m.confidence,
               m.status, m.created_at, m.updated_at,
               m.canonical_id, m.version_no, m.is_tombstone
        FROM memory_entries m
        JOIN projects p ON p.id = m.project_id
        WHERE m.id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    let tags = sqlx::query("SELECT tag FROM memory_tags WHERE memory_entry_id = $1 ORDER BY tag")
        .bind(id)
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|row| row.try_get::<String, _>("tag"))
        .collect::<Result<Vec<_>, _>>()?;

    let sources = sqlx::query(
        r#"
        SELECT ms.id, ms.task_id, ms.file_path, ms.git_commit, ms.symbol_name, ms.symbol_kind,
               ms.source_kind, ms.excerpt,
               v.status AS provenance_status,
               v.checked_at AS provenance_checked_at,
               v.reason AS provenance_reason,
               v.resolved_path AS provenance_resolved_path
        FROM memory_sources ms
        LEFT JOIN memory_source_verifications v ON v.source_id = ms.id
        WHERE ms.memory_entry_id = $1
        ORDER BY ms.created_at ASC
        "#,
    )
    .bind(id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        Ok(MemorySourceRecord {
            id: row.try_get("id")?,
            task_id: row.try_get("task_id")?,
            file_path: row.try_get("file_path")?,
            git_commit: row.try_get("git_commit")?,
            symbol_name: row.try_get("symbol_name")?,
            symbol_kind: row.try_get("symbol_kind")?,
            source_kind: parse_source_kind(&row.try_get::<String, _>("source_kind")?),
            excerpt: row.try_get("excerpt")?,
            provenance: source_provenance_from_row(&row)?,
        })
    })
    .collect::<Result<Vec<_>, sqlx::Error>>()?;

    let related_memories = sqlx::query(
        r#"
        SELECT mr.relation_type, m.id, m.summary, m.memory_type, m.confidence
        FROM memory_relations mr
        JOIN memory_entries m ON m.id = mr.dst_memory_id
        WHERE mr.src_memory_id = $1
        ORDER BY m.updated_at DESC, m.id
        LIMIT 12
        "#,
    )
    .bind(id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        Ok(RelatedMemorySummary {
            memory_id: row.try_get("id")?,
            relation_type: parse_relation_type(&row.try_get::<String, _>("relation_type")?),
            summary: row.try_get("summary")?,
            memory_type: parse_memory_type(&row.try_get::<String, _>("memory_type")?),
            confidence: row.try_get("confidence")?,
        })
    })
    .collect::<Result<Vec<_>, sqlx::Error>>()?;

    let embedding_spaces = fetch_memory_embedding_spaces(pool, id).await?;

    Ok(Some(MemoryEntryResponse {
        id,
        project: row.try_get("slug")?,
        canonical_text: row.try_get("canonical_text")?,
        summary: row.try_get("summary")?,
        memory_type: parse_memory_type(&row.try_get::<String, _>("memory_type")?),
        importance: row.try_get("importance")?,
        confidence: row.try_get("confidence")?,
        status: match row.try_get::<String, _>("status")?.as_str() {
            "archived" => mem_api::MemoryStatus::Archived,
            _ => mem_api::MemoryStatus::Active,
        },
        tags,
        sources,
        related_memories,
        embedding_spaces,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        canonical_id: row.try_get("canonical_id")?,
        version_no: row.try_get("version_no")?,
        is_tombstone: row.try_get("is_tombstone")?,
    }))
}

pub(crate) fn source_provenance_from_row(
    row: &sqlx::postgres::PgRow,
) -> Result<Option<SourceProvenanceRecord>, sqlx::Error> {
    let Some(status) = row.try_get::<Option<String>, _>("provenance_status")? else {
        return Ok(None);
    };
    Ok(Some(SourceProvenanceRecord {
        status: parse_source_provenance_status(&status),
        checked_at: row.try_get("provenance_checked_at")?,
        reason: row.try_get("provenance_reason")?,
        resolved_path: row.try_get("provenance_resolved_path")?,
    }))
}

pub(crate) fn parse_source_provenance_status(value: &str) -> SourceProvenanceStatus {
    match value {
        "verified" => SourceProvenanceStatus::Verified,
        "missing_file" => SourceProvenanceStatus::MissingFile,
        "missing_symbol" => SourceProvenanceStatus::MissingSymbol,
        "stale" => SourceProvenanceStatus::Stale,
        _ => SourceProvenanceStatus::Unverifiable,
    }
}

pub(crate) async fn fetch_memory_embedding_spaces(
    pool: &PgPool,
    memory_id: Uuid,
) -> Result<Vec<mem_api::MemoryEmbeddingSpace>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT mce.embedding_provider,
               mce.embedding_model,
               mce.embedding_base_url,
               COUNT(*)::bigint         AS chunk_count,
               MAX(mce.embedding_updated_at) AS last_updated
        FROM memory_chunk_embeddings mce
        JOIN memory_chunks mc ON mc.id = mce.chunk_id
        WHERE mc.memory_entry_id = $1
        GROUP BY mce.embedding_provider, mce.embedding_model, mce.embedding_base_url
        ORDER BY last_updated DESC NULLS LAST,
                 mce.embedding_provider,
                 mce.embedding_model
        "#,
    )
    .bind(memory_id)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(mem_api::MemoryEmbeddingSpace {
                provider: row.try_get("embedding_provider")?,
                model: row.try_get("embedding_model")?,
                base_url: row.try_get("embedding_base_url")?,
                chunk_count: row.try_get("chunk_count")?,
                last_updated: row.try_get("last_updated")?,
            })
        })
        .collect()
}

pub(crate) async fn get_memory(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<MemoryEntryResponse>, ApiError> {
    if !state.is_primary() {
        return Ok(Json(
            proxy_get_json(&state, &format!("/v1/memory/{id}")).await?,
        ));
    }
    let detail = fetch_memory_entry(state.pool()?, id)
        .await
        .map_err(ApiError::sql)?
        .ok_or_else(|| ApiError::not_found("memory entry not found"))?;
    Ok(Json(detail))
}

pub(crate) async fn get_memory_history(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<MemoryHistoryResponse>, ApiError> {
    if !state.is_primary() {
        return Ok(Json(
            proxy_get_json(&state, &format!("/v1/memory/{id}/history")).await?,
        ));
    }
    let pool = state.pool()?;
    // Walk back to the canonical_id of the provided version, then pull every
    // sibling version in chronological order. The caller can pass any
    // version's id (including a tombstone) and get the same chain.
    let anchor = sqlx::query(
        r#"
        SELECT m.canonical_id, p.slug
        FROM memory_entries m
        JOIN projects p ON p.id = m.project_id
        WHERE m.id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::sql)?
    .ok_or_else(|| ApiError::not_found("memory entry not found"))?;

    let canonical_id: Uuid = anchor.try_get("canonical_id").map_err(ApiError::sql)?;
    let project: String = anchor.try_get("slug").map_err(ApiError::sql)?;

    let version_ids: Vec<Uuid> = sqlx::query(
        r#"
        SELECT id
        FROM memory_entries
        WHERE canonical_id = $1
        ORDER BY version_no ASC
        "#,
    )
    .bind(canonical_id)
    .fetch_all(pool)
    .await
    .map_err(ApiError::sql)?
    .into_iter()
    .map(|row| row.try_get::<Uuid, _>("id"))
    .collect::<Result<Vec<_>, _>>()
    .map_err(ApiError::sql)?;

    let mut versions = Vec::with_capacity(version_ids.len());
    for version_id in version_ids {
        if let Some(entry) = fetch_memory_entry(pool, version_id)
            .await
            .map_err(ApiError::sql)?
        {
            versions.push(entry);
        }
    }

    Ok(Json(MemoryHistoryResponse {
        canonical_id,
        project,
        versions,
    }))
}

pub(crate) async fn archive(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ArchiveRequest>,
) -> Result<Json<ArchiveResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/archive", &request, true).await?,
        ));
    }
    let project = request.project.clone();
    let archived_count = if request.dry_run {
        sqlx::query(
            r#"
            SELECT COUNT(*) AS count
            FROM memory_entries m
            JOIN projects p ON p.id = m.project_id
            WHERE p.slug = $1
              AND m.status = 'active'
              AND m.confidence <= $2
              AND m.importance <= $3
            "#,
        )
        .bind(&request.project)
        .bind(request.max_confidence)
        .bind(request.max_importance)
        .fetch_one(state.pool()?)
        .await
        .map_err(ApiError::sql)?
        .try_get::<i64, _>("count")
        .map_err(ApiError::sql)? as u64
    } else {
        sqlx::query(
            r#"
            UPDATE memory_entries m
            SET status = 'archived', archived_at = now(), updated_at = now()
            FROM projects p
            WHERE p.id = m.project_id
              AND p.slug = $1
              AND m.status = 'active'
              AND m.confidence <= $2
              AND m.importance <= $3
            "#,
        )
        .bind(&request.project)
        .bind(request.max_confidence)
        .bind(request.max_importance)
        .execute(state.pool()?)
        .await
        .map_err(ApiError::sql)?
        .rows_affected()
    };
    if request.dry_run {
        return Ok(Json(ArchiveResponse {
            archived_count,
            dry_run: true,
        }));
    }
    notify_project_changed(
        &state,
        project,
        None,
        ActivityKind::Archive,
        format!(
            "Archived {} low-value memory entry/entries.",
            archived_count
        ),
        Some(ActivityDetails::Archive {
            archived_count,
            max_confidence: request.max_confidence,
            max_importance: request.max_importance,
        }),
    );

    Ok(Json(ArchiveResponse {
        archived_count,
        dry_run: false,
    }))
}

pub(crate) async fn delete_memory(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<DeleteMemoryRequest>,
) -> Result<Json<DeleteMemoryResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_delete_json(&state, "/v1/memory", &request).await?,
        ));
    }

    // Memories are immutable. Delete writes a tombstone version — a row with
    // the same canonical_id but empty content and is_tombstone=TRUE. Default
    // searches skip it; history-aware queries can still surface the prior
    // versions so nothing is truly lost.
    let pool = state.pool()?;
    let mut tx = pool.begin().await.map_err(ApiError::sql)?;
    let target = sqlx::query(
        r#"
        SELECT m.id, m.project_id, p.slug, m.canonical_id, m.summary,
               (
                   SELECT MAX(m2.version_no)
                   FROM memory_entries m2
                   WHERE m2.canonical_id = m.canonical_id
               ) AS latest_version
        FROM memory_entries m
        JOIN projects p ON p.id = m.project_id
        WHERE m.id = $1
        "#,
    )
    .bind(request.memory_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(ApiError::sql)?
    .ok_or_else(|| ApiError::not_found("memory entry not found"))?;

    let project_id: Uuid = target.try_get("project_id").map_err(ApiError::sql)?;
    let project: String = target.try_get("slug").map_err(ApiError::sql)?;
    let canonical_id: Uuid = target.try_get("canonical_id").map_err(ApiError::sql)?;
    let latest_version: i32 = target.try_get("latest_version").map_err(ApiError::sql)?;
    let summary: String = target.try_get("summary").map_err(ApiError::sql)?;

    let tombstone_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO memory_entries
            (id, project_id, canonical_id, version_no, is_tombstone,
             canonical_text, summary, memory_type, scope, importance,
             confidence, status, created_at, updated_at, archived_at,
             search_document)
        VALUES
            ($1, $2, $3, $4, TRUE, '', '', 'implementation', 'project', 0, 0.0,
             'active', now(), now(), NULL, to_tsvector('english', ''))
        "#,
    )
    .bind(tombstone_id)
    .bind(project_id)
    .bind(canonical_id)
    .bind(latest_version + 1)
    .execute(&mut *tx)
    .await
    .map_err(ApiError::sql)?;
    tx.commit().await.map_err(ApiError::sql)?;

    let memory_id = tombstone_id;
    notify_project_changed(
        &state,
        project.clone(),
        Some(memory_id),
        ActivityKind::DeleteMemory,
        format!("Deleted memory: {summary}"),
        Some(ActivityDetails::DeleteMemory {
            deleted: true,
            summary: summary.clone(),
        }),
    );

    Ok(Json(DeleteMemoryResponse {
        memory_id,
        project,
        summary,
        deleted: true,
    }))
}

pub(crate) async fn prune_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<PruneHistoryRequest>,
) -> Result<Json<PruneHistoryResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    // Fill missing thresholds from server config so the caller can rely on
    // either source without duplicating the logic in every client.
    let tombstone_after = request
        .tombstone_after
        .or(state.config.retention.tombstone_after);
    let superseded_after = request
        .superseded_after
        .or(state.config.retention.superseded_after);
    let effective = PruneHistoryRequest {
        project: request.project.clone(),
        tombstone_after,
        superseded_after,
        dry_run: request.dry_run,
    };
    effective.validate().map_err(ApiError::validation)?;

    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/prune-history", &effective, true).await?,
        ));
    }

    let pool = state.pool()?;
    let mut tx = pool.begin().await.map_err(ApiError::sql)?;

    let project_filter: Option<String> = effective.project.clone();
    let dry_run = effective.dry_run;

    let mut canonicals_tombstoned_deleted: u64 = 0;
    if let Some(threshold) = effective.tombstone_after {
        let seconds = threshold.as_secs_f64();
        let count_sql = r#"
            WITH latest AS (
                SELECT DISTINCT ON (m.canonical_id)
                       m.canonical_id, m.updated_at, m.is_tombstone
                FROM memory_entries m
                JOIN projects p ON p.id = m.project_id
                WHERE ($1::text IS NULL OR p.slug = $1)
                ORDER BY m.canonical_id, m.version_no DESC
            )
            SELECT COUNT(*) AS count
            FROM latest
            WHERE is_tombstone = TRUE
              AND updated_at < now() - make_interval(secs => $2)
        "#;
        let count: i64 = sqlx::query(count_sql)
            .bind(project_filter.as_deref())
            .bind(seconds)
            .fetch_one(&mut *tx)
            .await
            .map_err(ApiError::sql)?
            .try_get("count")
            .map_err(ApiError::sql)?;
        canonicals_tombstoned_deleted = count.max(0) as u64;

        if !dry_run && canonicals_tombstoned_deleted > 0 {
            let delete_sql = r#"
                WITH latest AS (
                    SELECT DISTINCT ON (m.canonical_id)
                           m.canonical_id, m.updated_at, m.is_tombstone
                    FROM memory_entries m
                    JOIN projects p ON p.id = m.project_id
                    WHERE ($1::text IS NULL OR p.slug = $1)
                    ORDER BY m.canonical_id, m.version_no DESC
                ),
                dead AS (
                    SELECT canonical_id FROM latest
                    WHERE is_tombstone = TRUE
                      AND updated_at < now() - make_interval(secs => $2)
                )
                DELETE FROM memory_entries
                WHERE canonical_id IN (SELECT canonical_id FROM dead)
            "#;
            sqlx::query(delete_sql)
                .bind(project_filter.as_deref())
                .bind(seconds)
                .execute(&mut *tx)
                .await
                .map_err(ApiError::sql)?;
        }
    }

    let mut superseded_versions_pruned: u64 = 0;
    if let Some(threshold) = effective.superseded_after {
        let seconds = threshold.as_secs_f64();
        let count_sql = r#"
            SELECT COUNT(*) AS count
            FROM memory_entries m
            JOIN projects p ON p.id = m.project_id
            WHERE ($1::text IS NULL OR p.slug = $1)
              AND m.is_tombstone = FALSE
              AND m.updated_at < now() - make_interval(secs => $2)
              AND m.version_no < (
                  SELECT MAX(m2.version_no)
                  FROM memory_entries m2
                  WHERE m2.canonical_id = m.canonical_id
              )
        "#;
        let count: i64 = sqlx::query(count_sql)
            .bind(project_filter.as_deref())
            .bind(seconds)
            .fetch_one(&mut *tx)
            .await
            .map_err(ApiError::sql)?
            .try_get("count")
            .map_err(ApiError::sql)?;
        superseded_versions_pruned = count.max(0) as u64;

        if !dry_run && superseded_versions_pruned > 0 {
            let delete_sql = r#"
                DELETE FROM memory_entries m
                USING projects p
                WHERE m.project_id = p.id
                  AND ($1::text IS NULL OR p.slug = $1)
                  AND m.is_tombstone = FALSE
                  AND m.updated_at < now() - make_interval(secs => $2)
                  AND m.version_no < (
                      SELECT MAX(m2.version_no)
                      FROM memory_entries m2
                      WHERE m2.canonical_id = m.canonical_id
                  )
            "#;
            sqlx::query(delete_sql)
                .bind(project_filter.as_deref())
                .bind(seconds)
                .execute(&mut *tx)
                .await
                .map_err(ApiError::sql)?;
        }
    }

    tx.commit().await.map_err(ApiError::sql)?;

    Ok(Json(PruneHistoryResponse {
        project: project_filter,
        canonicals_tombstoned_deleted,
        superseded_versions_pruned,
        dry_run,
    }))
}
