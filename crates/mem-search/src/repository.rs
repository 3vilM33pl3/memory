use super::*;
use sqlx::Row;

pub(super) async fn fetch_lexical_candidates(
    pool: &PgPool,
    request: &QueryExecution<'_>,
    normalized: &QueryIntent,
    candidate_limit: i64,
) -> Result<Vec<CandidateRecord>, sqlx::Error> {
    let memory_type_filters = request
        .filters
        .types
        .iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>();
    let lexical_like_terms = normalized
        .lexical_terms
        .iter()
        .map(|term| format!("%{term}%"))
        .collect::<Vec<_>>();
    let path_like_terms = normalized
        .path_terms
        .iter()
        .map(|term| format!("%{term}%"))
        .collect::<Vec<_>>();
    let tag_like_terms = normalized
        .lexical_terms
        .iter()
        .map(|term| format!("%{term}%"))
        .collect::<Vec<_>>();

    let rows = sqlx::query(
        r#"
        WITH input AS (
            SELECT websearch_to_tsquery('english', $2) AS query
        )
        SELECT
            m.id,
            p.slug AS project,
            p.name AS project_name,
            p.root_path AS repo_root,
            m.summary,
            m.memory_type,
            m.canonical_text,
            m.importance,
            m.confidence,
            m.updated_at,
            COALESCE(ts_rank_cd(m.search_document, input.query), 0) AS entry_fts,
            COALESCE(best_chunk.chunk_fts, 0) AS chunk_fts,
            COALESCE(best_chunk.chunk_text, left(m.canonical_text, 320)) AS best_chunk_text,
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
            ), ARRAY[]::text[]) AS source_paths
        FROM memory_entries m
        JOIN projects p ON p.id = m.project_id
        CROSS JOIN input
        LEFT JOIN LATERAL (
            SELECT
                mc.chunk_text,
                ts_rank_cd(mc.tsv, input.query) AS chunk_fts
            FROM memory_chunks mc
            WHERE mc.memory_entry_id = m.id
              AND mc.tsv @@ input.query
            ORDER BY chunk_fts DESC, mc.id
            LIMIT 1
        ) best_chunk ON true
        WHERE ($1::text IS NULL OR p.slug = $1)
          AND m.status = 'active'
          AND (
                $9::boolean
                OR (
                    m.is_tombstone = FALSE
                    AND m.version_no = (
                        SELECT MAX(m2.version_no)
                        FROM memory_entries m2
                        WHERE m2.canonical_id = m.canonical_id
                    )
                )
          )
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
                m.search_document @@ input.query
                OR best_chunk.chunk_text IS NOT NULL
                OR (
                    cardinality($5::text[]) > 0
                    AND (
                        m.summary ILIKE ANY($5)
                        OR m.canonical_text ILIKE ANY($5)
                    )
                )
                OR (
                    cardinality($6::text[]) > 0
                    AND EXISTS (
                        SELECT 1
                        FROM memory_sources ms
                        WHERE ms.memory_entry_id = m.id
                          AND ms.file_path ILIKE ANY($6)
                    )
                )
                OR (
                    cardinality($7::text[]) > 0
                    AND EXISTS (
                        SELECT 1
                        FROM memory_tags mt
                        WHERE mt.memory_entry_id = m.id
                          AND mt.tag ILIKE ANY($7)
                    )
                )
            )
        ORDER BY GREATEST(
                COALESCE(ts_rank_cd(m.search_document, input.query), 0),
                COALESCE(best_chunk.chunk_fts, 0)
            ) DESC,
            updated_at DESC,
            id DESC
        LIMIT $8
        "#,
    )
    .bind(request.project)
    .bind(request.query)
    .bind(if memory_type_filters.is_empty() {
        None::<Vec<String>>
    } else {
        Some(memory_type_filters)
    })
    .bind(&request.filters.tags)
    .bind(&lexical_like_terms)
    .bind(&path_like_terms)
    .bind(&tag_like_terms)
    .bind(candidate_limit)
    .bind(request.history)
    .fetch_all(pool)
    .await?;

    rows.into_iter().map(candidate_from_lexical_row).collect()
}

fn candidate_from_lexical_row(row: sqlx::postgres::PgRow) -> Result<CandidateRecord, sqlx::Error> {
    Ok(CandidateRecord {
        memory_id: row.try_get("id")?,
        project: row.try_get("project")?,
        project_name: row.try_get("project_name")?,
        repo_root: row.try_get("repo_root")?,
        summary: row.try_get("summary")?,
        memory_type: parse_memory_type(&row.try_get::<String, _>("memory_type")?),
        canonical_text: row.try_get("canonical_text")?,
        importance: row.try_get("importance")?,
        confidence: row.try_get("confidence")?,
        updated_at: row.try_get("updated_at")?,
        entry_fts: f64::from(row.try_get::<f32, _>("entry_fts")?),
        chunk_fts: f64::from(row.try_get::<f32, _>("chunk_fts")?),
        semantic_similarity: 0.0,
        best_chunk_text: row.try_get("best_chunk_text")?,
        tags: row.try_get("tags")?,
        source_paths: row.try_get("source_paths")?,
        graph_boost: 0.0,
        graph_match_count: 0,
        graph_edge_count: 0,
        graph_connections: Vec::new(),
    })
}

pub(super) async fn fetch_semantic_candidates(
    pool: &PgPool,
    request: &QueryExecution<'_>,
    embedding_space: &EmbeddingSpace,
    embedding_dimension: i32,
    query_embedding: &Vector,
    candidate_limit: i64,
) -> Result<Vec<CandidateRecord>, sqlx::Error> {
    let memory_type_filters = request
        .filters
        .types
        .iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>();

    let rows = sqlx::query(
        r#"
        SELECT
            m.id,
            p.slug AS project,
            p.name AS project_name,
            p.root_path AS repo_root,
            m.summary,
            m.memory_type,
            m.canonical_text,
            m.importance,
            m.confidence,
            m.updated_at,
            mc.chunk_text,
            (mce.embedding <=> $6) AS cosine_distance,
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
            ), ARRAY[]::text[]) AS source_paths
        FROM memory_chunks mc
        JOIN memory_chunk_embeddings mce ON mce.chunk_id = mc.id
        JOIN memory_entries m ON m.id = mc.memory_entry_id
        JOIN projects p ON p.id = m.project_id
        WHERE ($1::text IS NULL OR p.slug = $1)
          AND m.status = 'active'
          AND (
                $8::boolean
                OR (
                    m.is_tombstone = FALSE
                    AND m.version_no = (
                        SELECT MAX(m2.version_no)
                        FROM memory_entries m2
                        WHERE m2.canonical_id = m.canonical_id
                    )
                )
          )
          AND mce.embedding_space = $4
          AND mce.embedding_dimension = $5
          AND ($2::text[] IS NULL OR m.memory_type = ANY($2))
          AND (
                cardinality($3::text[]) = 0
                OR EXISTS (
                    SELECT 1
                    FROM memory_tags mt
                    WHERE mt.memory_entry_id = m.id
                      AND mt.tag = ANY($3)
                )
          )
        ORDER BY cosine_distance ASC, m.updated_at DESC, m.id
        LIMIT $7
        "#,
    )
    .bind(request.project)
    .bind(if memory_type_filters.is_empty() {
        None::<Vec<String>>
    } else {
        Some(memory_type_filters)
    })
    .bind(&request.filters.tags)
    .bind(&embedding_space.space_key)
    .bind(embedding_dimension)
    .bind(query_embedding)
    .bind(candidate_limit)
    .bind(request.history)
    .fetch_all(pool)
    .await?;

    let mut by_memory = HashMap::<Uuid, CandidateRecord>::new();
    for row in rows {
        let cosine_distance: f64 = row.try_get("cosine_distance")?;
        if !cosine_distance.is_finite() {
            continue;
        }
        let similarity = (1.0 - cosine_distance).max(0.0);

        let memory_id: Uuid = row.try_get("id")?;
        let entry = by_memory
            .entry(memory_id)
            .or_insert_with(|| CandidateRecord {
                memory_id,
                project: row.try_get("project").ok(),
                project_name: row.try_get("project_name").ok(),
                repo_root: row.try_get("repo_root").ok(),
                summary: row.try_get("summary").unwrap_or_default(),
                memory_type: parse_memory_type(
                    &row.try_get::<String, _>("memory_type")
                        .unwrap_or_else(|_| "convention".to_string()),
                ),
                canonical_text: row.try_get("canonical_text").unwrap_or_default(),
                importance: row.try_get("importance").unwrap_or_default(),
                confidence: row.try_get("confidence").unwrap_or(0.0),
                updated_at: row.try_get("updated_at").unwrap_or_else(|_| Utc::now()),
                entry_fts: 0.0,
                chunk_fts: 0.0,
                semantic_similarity: similarity,
                best_chunk_text: row.try_get("chunk_text").unwrap_or_default(),
                tags: row.try_get("tags").unwrap_or_default(),
                source_paths: row.try_get("source_paths").unwrap_or_default(),
                graph_boost: 0.0,
                graph_match_count: 0,
                graph_edge_count: 0,
                graph_connections: Vec::new(),
            });

        if similarity > entry.semantic_similarity {
            entry.semantic_similarity = similarity;
            entry.best_chunk_text = row.try_get("chunk_text").unwrap_or_default();
        }
    }

    let mut candidates = by_memory.into_values().collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .semantic_similarity
            .total_cmp(&left.semantic_similarity)
            .then_with(|| right.updated_at.cmp(&left.updated_at))
    });
    candidates.truncate(candidate_limit as usize);
    Ok(candidates)
}

pub(super) async fn rebuild_chunks_selected(
    pool: &PgPool,
    project: &str,
    selected: Vec<(&str, &EmbeddingService)>,
) -> Result<u64> {
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
    .await
    .context("load memories for chunk rebuild")?;

    let mut count = 0_u64;
    for row in rows {
        rebuild_memory_chunks_from_row(pool, &selected, &row).await?;
        count += 1;
    }
    Ok(count)
}

pub(super) async fn rebuild_memory_chunks_selected(
    pool: &PgPool,
    project: &str,
    memory_ids: &[Uuid],
    selected: Vec<(&str, &EmbeddingService)>,
) -> Result<u64> {
    if memory_ids.is_empty() {
        return Ok(0);
    }

    let rows = sqlx::query(
        r#"
        SELECT m.id, m.canonical_text, m.summary
        FROM memory_entries m
        JOIN projects p ON p.id = m.project_id
        WHERE p.slug = $1
          AND m.id = ANY($2)
        "#,
    )
    .bind(project)
    .bind(memory_ids)
    .fetch_all(pool)
    .await
    .context("load targeted memories for chunk rebuild")?;

    let mut count = 0_u64;
    for row in rows {
        rebuild_memory_chunks_from_row(pool, &selected, &row).await?;
        count += 1;
    }
    Ok(count)
}

async fn rebuild_memory_chunks_from_row(
    pool: &PgPool,
    selected: &[(&str, &EmbeddingService)],
    row: &sqlx::postgres::PgRow,
) -> Result<()> {
    let memory_id: Uuid = row.try_get("id")?;
    let canonical_text: String = row.try_get("canonical_text")?;
    let summary: String = row.try_get("summary")?;
    sqlx::query("DELETE FROM memory_chunks WHERE memory_entry_id = $1")
        .bind(memory_id)
        .execute(pool)
        .await
        .context("delete old chunks")?;

    let chunks = split_search_chunks(&summary, &canonical_text);

    let mut batches: Vec<EmbeddingBatch> = Vec::with_capacity(selected.len());
    for (_, service) in selected {
        batches.push(
            service
                .embed_texts(&chunks, EmbeddingPurpose::Document)
                .await
                .context("embed rebuilt chunks")?,
        );
    }
    if selected.is_empty() {
        batches.push(empty_embedding_batch());
    }

    for (index, chunk_text) in chunks.iter().enumerate() {
        let chunk_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO memory_chunks
                (
                    id,
                    memory_entry_id,
                    chunk_text,
                    search_text,
                    tsv
                )
            VALUES
                ($1, $2, $3, $4, to_tsvector('english', $4))
            "#,
        )
        .bind(chunk_id)
        .bind(memory_id)
        .bind(chunk_text)
        .bind(format!("{summary}\n{chunk_text}"))
        .execute(pool)
        .await
        .context("insert rebuilt chunk")?;
        for batch in &batches {
            let Some(embedding) = batch.vectors.get(index).cloned() else {
                continue;
            };
            upsert_chunk_embedding(pool, chunk_id, &batch.space, batch.dimension, embedding)
                .await
                .context("upsert rebuilt chunk embedding")?;
        }
    }
    Ok(())
}

pub(super) async fn reembed_single_backend(
    pool: &PgPool,
    project: &str,
    embedder: &EmbeddingService,
) -> Result<u64> {
    ensure_chunks_for_reembedding(pool, project).await?;

    let target_space = embedder.embedding_space();
    let rows = sqlx::query(
        r#"
        SELECT mc.id, mc.search_text
        FROM memory_chunks mc
        JOIN memory_entries m ON m.id = mc.memory_entry_id
        JOIN projects p ON p.id = m.project_id
        LEFT JOIN memory_chunk_embeddings mce
          ON mce.chunk_id = mc.id
         AND mce.embedding_space = $2
        WHERE p.slug = $1
          AND m.status = 'active'
          AND m.is_tombstone = FALSE
          AND (
                mce.chunk_id IS NULL
                OR mce.embedding_dimension IS NULL
              )
        ORDER BY mc.id
        "#,
    )
    .bind(project)
    .bind(&target_space.space_key)
    .fetch_all(pool)
    .await
    .context("load stale chunks for re-embedding")?;

    if rows.is_empty() {
        return Ok(0);
    }

    let mut reembedded_chunks = 0u64;
    for batch in rows.chunks(embedder.batch_size()) {
        let chunk_ids = batch
            .iter()
            .map(|row| row.try_get::<Uuid, _>("id"))
            .collect::<Result<Vec<_>, _>>()
            .context("decode stale chunk ids")?;
        let texts = batch
            .iter()
            .map(|row| row.try_get::<String, _>("search_text"))
            .collect::<Result<Vec<_>, _>>()
            .context("decode stale chunk texts")?;
        let embeddings = embedder
            .embed_texts(&texts, EmbeddingPurpose::Document)
            .await
            .context("embed stale chunks")?;

        for (index, chunk_id) in chunk_ids.iter().enumerate() {
            let embedding = embeddings
                .vectors
                .get(index)
                .cloned()
                .context("missing embedding for stale chunk batch item")?;
            upsert_chunk_embedding(
                pool,
                *chunk_id,
                &embeddings.space,
                embeddings.dimension,
                embedding,
            )
            .await
            .context("update active-space chunk embedding")?;
            reembedded_chunks += 1;
        }
    }

    Ok(reembedded_chunks)
}

pub(super) async fn prune_project_embeddings(
    pool: &PgPool,
    project: &str,
    keep: &[String],
) -> Result<u64> {
    let result = sqlx::query(
        r#"
        DELETE FROM memory_chunk_embeddings mce
        USING memory_chunks mc, memory_entries m, projects p
        WHERE mce.chunk_id = mc.id
          AND mc.memory_entry_id = m.id
          AND m.project_id = p.id
          AND p.slug = $1
          AND m.status = 'active'
          AND m.is_tombstone = FALSE
          AND mce.embedding_space <> ALL($2)
        "#,
    )
    .bind(project)
    .bind(keep)
    .execute(pool)
    .await
    .context("delete inactive embedding spaces")?;
    Ok(result.rows_affected())
}

async fn ensure_chunks_for_reembedding(pool: &PgPool, project: &str) -> Result<u64> {
    let rows = sqlx::query(
        r#"
        SELECT m.id, m.canonical_text, m.summary
        FROM memory_entries m
        JOIN projects p ON p.id = m.project_id
        WHERE p.slug = $1
          AND m.status = 'active'
          AND m.is_tombstone = FALSE
          AND NOT EXISTS (
              SELECT 1
              FROM memory_chunks mc
              WHERE mc.memory_entry_id = m.id
          )
        "#,
    )
    .bind(project)
    .fetch_all(pool)
    .await
    .context("load memories missing chunks for re-embedding")?;

    let mut inserted = 0u64;
    for row in rows {
        let memory_id: Uuid = row.try_get("id")?;
        let canonical_text: String = row.try_get("canonical_text")?;
        let summary: String = row.try_get("summary")?;
        for chunk_text in split_search_chunks(&summary, &canonical_text) {
            sqlx::query(
                r#"
                INSERT INTO memory_chunks
                    (
                        id,
                        memory_entry_id,
                        chunk_text,
                        search_text,
                        tsv
                    )
                VALUES
                    ($1, $2, $3, $4, to_tsvector('english', $4))
                "#,
            )
            .bind(Uuid::new_v4())
            .bind(memory_id)
            .bind(&chunk_text)
            .bind(format!("{summary}\n{chunk_text}"))
            .execute(pool)
            .await
            .context("insert missing chunk for re-embedding")?;
            inserted += 1;
        }
    }

    Ok(inserted)
}

async fn upsert_chunk_embedding(
    pool: &PgPool,
    chunk_id: Uuid,
    space: &EmbeddingSpace,
    dimension: i32,
    embedding: Vector,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO memory_chunk_embeddings (
            chunk_id,
            embedding_space,
            embedding,
            embedding_provider,
            embedding_base_url,
            embedding_model,
            embedding_dimension,
            embedding_updated_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        ON CONFLICT (chunk_id, embedding_space) DO UPDATE
        SET embedding = EXCLUDED.embedding,
            embedding_provider = EXCLUDED.embedding_provider,
            embedding_base_url = EXCLUDED.embedding_base_url,
            embedding_model = EXCLUDED.embedding_model,
            embedding_dimension = EXCLUDED.embedding_dimension,
            embedding_updated_at = EXCLUDED.embedding_updated_at
        "#,
    )
    .bind(chunk_id)
    .bind(&space.space_key)
    .bind(embedding)
    .bind(&space.provider)
    .bind(&space.base_url)
    .bind(&space.model)
    .bind(dimension)
    .bind(Utc::now())
    .execute(pool)
    .await
    .context("upsert chunk embedding")?;
    Ok(())
}

pub(super) async fn scope_has_active_embedding_space(
    pool: &PgPool,
    project: Option<&str>,
    embedding_space: &str,
    embedding_dimension: i32,
) -> Result<bool> {
    let row = sqlx::query(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM memory_chunk_embeddings mce
            JOIN memory_chunks mc ON mc.id = mce.chunk_id
            JOIN memory_entries m ON m.id = mc.memory_entry_id
            JOIN projects p ON p.id = m.project_id
            WHERE ($1::text IS NULL OR p.slug = $1)
              AND m.status = 'active'
              AND m.is_tombstone = FALSE
              AND mce.embedding_space = $2
              AND mce.embedding_dimension = $3
        ) AS present
        "#,
    )
    .bind(project)
    .bind(embedding_space)
    .bind(embedding_dimension)
    .fetch_one(pool)
    .await
    .context("check active embedding space")?;
    Ok(row.try_get("present")?)
}

pub(super) struct GraphCandidateOutcome {
    pub(super) status: String,
    pub(super) candidates: Vec<CandidateRecord>,
}

pub(super) async fn fetch_graph_candidates(
    pool: &PgPool,
    request: &QueryExecution<'_>,
    intent: &QueryIntent,
    candidate_limit: i64,
) -> Result<GraphCandidateOutcome, sqlx::Error> {
    let graph_like_terms = graph_like_terms(intent);
    let path_like_terms = intent
        .path_terms
        .iter()
        .map(|term| format!("%{term}%"))
        .collect::<Vec<_>>();
    if graph_like_terms.is_empty() && path_like_terms.is_empty() {
        return Ok(GraphCandidateOutcome {
            status: "no_terms".to_string(),
            candidates: Vec::new(),
        });
    }

    let graph_present_row = sqlx::query(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM graph_extraction_runs ger
            JOIN projects p ON p.id = ger.project_id
            WHERE ($1::text IS NULL OR p.slug = $1)
              AND ger.status = 'completed'
        ) AS present
        "#,
    )
    .bind(request.project)
    .fetch_one(pool)
    .await?;
    if !graph_present_row.try_get::<bool, _>("present")? {
        return Ok(GraphCandidateOutcome {
            status: "no_graph".to_string(),
            candidates: Vec::new(),
        });
    }

    let memory_type_filters = request
        .filters
        .types
        .iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>();

    let rows = sqlx::query(
        r#"
        WITH latest_runs AS (
            SELECT DISTINCT ON (ger.project_id)
                ger.id,
                ger.project_id
            FROM graph_extraction_runs ger
            JOIN projects p ON p.id = ger.project_id
            WHERE ($1::text IS NULL OR p.slug = $1)
              AND ger.status = 'completed'
            ORDER BY ger.project_id, ger.completed_at DESC NULLS LAST, ger.started_at DESC
        ),
        direct_symbol_hits AS (
            SELECT
                cs.extraction_run_id,
                cs.graph_node_id,
                cs.file_path,
                cs.display_name AS symbol,
                cs.symbol_kind,
                NULL::text AS edge_kind,
                NULL::text AS neighbor_symbol,
                'direct'::text AS direction,
                $7::float8 AS boost,
                'code symbol match'::text AS reason,
                FALSE AS edge_hit
            FROM code_symbols cs
            JOIN latest_runs lr ON lr.id = cs.extraction_run_id
            WHERE
              (
                    ($2::text[] IS NOT NULL AND (
                        cs.name ILIKE ANY($2)
                        OR COALESCE(cs.qualified_name, '') ILIKE ANY($2)
                    ))
                    OR (cardinality($3::text[]) > 0 AND cs.file_path ILIKE ANY($3))
              )
        ),
        direct_reference_hits AS (
            SELECT
                cr.extraction_run_id,
                cs.graph_node_id,
                cr.file_path,
                COALESCE(cs.display_name, cr.target_text) AS symbol,
                cs.symbol_kind,
                cr.reference_kind AS edge_kind,
                NULL::text AS neighbor_symbol,
                'direct'::text AS direction,
                $8::float8 AS boost,
                'code reference match'::text AS reason,
                FALSE AS edge_hit
            FROM code_references cr
            JOIN latest_runs lr ON lr.id = cr.extraction_run_id
            LEFT JOIN code_symbols cs
              ON cs.extraction_run_id = cr.extraction_run_id
             AND cs.stable_identity = cr.target_symbol_identity
            WHERE (
                    ($2::text[] IS NOT NULL AND (
                        cr.target_text ILIKE ANY($2)
                        OR COALESCE(cr.source_text, '') ILIKE ANY($2)
                    ))
                    OR (cardinality($3::text[]) > 0 AND cr.file_path ILIKE ANY($3))
              )
        ),
        direct_node_hits AS (
            SELECT extraction_run_id, graph_node_id FROM direct_symbol_hits WHERE graph_node_id IS NOT NULL
            UNION
            SELECT extraction_run_id, graph_node_id FROM direct_reference_hits WHERE graph_node_id IS NOT NULL
        ),
        neighbor_hits AS (
            SELECT
                neighbor.extraction_run_id,
                neighbor.graph_node_id,
                neighbor.file_path,
                neighbor.display_name AS symbol,
                neighbor.symbol_kind,
                ge.edge_kind,
                anchor.display_name AS neighbor_symbol,
                CASE
                    WHEN ge.source_node_id = direct_node_hits.graph_node_id THEN 'outgoing'
                    ELSE 'incoming'
                END AS direction,
                $9::float8 AS boost,
                'one-hop graph neighbor'::text AS reason,
                TRUE AS edge_hit
            FROM direct_node_hits
            JOIN graph_edges ge
              ON ge.extraction_run_id = direct_node_hits.extraction_run_id
             AND (ge.source_node_id = direct_node_hits.graph_node_id OR ge.target_node_id = direct_node_hits.graph_node_id)
            JOIN code_symbols neighbor
              ON neighbor.extraction_run_id = ge.extraction_run_id
             AND neighbor.graph_node_id = CASE
                    WHEN ge.source_node_id = direct_node_hits.graph_node_id THEN ge.target_node_id
                    ELSE ge.source_node_id
                END
            JOIN code_symbols anchor
              ON anchor.extraction_run_id = ge.extraction_run_id
             AND anchor.graph_node_id = direct_node_hits.graph_node_id
        ),
        graph_hits AS (
            SELECT * FROM direct_symbol_hits
            UNION ALL
            SELECT * FROM direct_reference_hits
            UNION ALL
            SELECT * FROM neighbor_hits
        )
        SELECT
            m.id,
            p.slug AS project,
            p.name AS project_name,
            p.root_path AS repo_root,
            m.summary,
            m.memory_type,
            m.canonical_text,
            m.importance,
            m.confidence,
            m.updated_at,
            left(m.canonical_text, 320) AS best_chunk_text,
            COALESCE((
                SELECT ARRAY_AGG(mt.tag ORDER BY mt.tag)
                FROM memory_tags mt
                WHERE mt.memory_entry_id = m.id
            ), ARRAY[]::text[]) AS tags,
            COALESCE((
                SELECT ARRAY_AGG(ms2.file_path ORDER BY ms2.file_path)
                FROM memory_sources ms2
                WHERE ms2.memory_entry_id = m.id
                  AND ms2.file_path IS NOT NULL
            ), ARRAY[]::text[]) AS source_paths,
            gh.file_path,
            gh.symbol,
            gh.symbol_kind,
            gh.edge_kind,
            gh.neighbor_symbol,
            gh.direction,
            gh.boost,
            gh.reason,
            gh.edge_hit
        FROM graph_hits gh
        JOIN memory_sources ms
          ON ms.file_path IS NOT NULL
         AND (
                ms.file_path = gh.file_path
                OR (right(ms.file_path, 1) = '/' AND gh.file_path LIKE ms.file_path || '%')
             )
        JOIN memory_entries m ON m.id = ms.memory_entry_id
        JOIN projects p ON p.id = m.project_id
        WHERE ($1::text IS NULL OR p.slug = $1)
          AND m.status = 'active'
          AND (
                $6::boolean
                OR (
                    m.is_tombstone = FALSE
                    AND m.version_no = (
                        SELECT MAX(m2.version_no)
                        FROM memory_entries m2
                        WHERE m2.canonical_id = m.canonical_id
                    )
                )
          )
          AND ($4::text[] IS NULL OR m.memory_type = ANY($4))
          AND (
                cardinality($5::text[]) = 0
                OR EXISTS (
                    SELECT 1
                    FROM memory_tags mt
                    WHERE mt.memory_entry_id = m.id
                      AND mt.tag = ANY($5)
                )
          )
        ORDER BY gh.boost DESC, m.updated_at DESC, m.id
        LIMIT $10
        "#,
    )
    .bind(request.project)
    .bind(if graph_like_terms.is_empty() {
        None::<Vec<String>>
    } else {
        Some(graph_like_terms)
    })
    .bind(&path_like_terms)
    .bind(if memory_type_filters.is_empty() {
        None::<Vec<String>>
    } else {
        Some(memory_type_filters)
    })
    .bind(&request.filters.tags)
    .bind(request.history)
    .bind(GRAPH_DIRECT_BOOST)
    .bind(GRAPH_REFERENCE_BOOST)
    .bind(GRAPH_NEIGHBOR_BOOST)
    .bind(candidate_limit * 6)
    .fetch_all(pool)
    .await?;

    let mut candidates = HashMap::<Uuid, CandidateRecord>::new();
    for row in rows {
        let memory_id: Uuid = row.try_get("id")?;
        let boost: f64 = row.try_get("boost")?;
        let edge_hit: bool = row.try_get("edge_hit")?;
        let connection = QueryGraphConnection {
            file_path: row.try_get("file_path")?,
            symbol: row.try_get("symbol")?,
            symbol_kind: row.try_get("symbol_kind")?,
            edge_kind: row.try_get("edge_kind")?,
            neighbor_symbol: row.try_get("neighbor_symbol")?,
            direction: row.try_get("direction")?,
            score_boost: boost,
            reason: row.try_get("reason")?,
        };
        let entry = candidates
            .entry(memory_id)
            .or_insert_with(|| CandidateRecord {
                memory_id,
                project: row.try_get("project").ok(),
                project_name: row.try_get("project_name").ok(),
                repo_root: row.try_get("repo_root").ok(),
                summary: row.try_get("summary").unwrap_or_default(),
                memory_type: parse_memory_type(
                    &row.try_get::<String, _>("memory_type")
                        .unwrap_or_else(|_| "reference".to_string()),
                ),
                canonical_text: row.try_get("canonical_text").unwrap_or_default(),
                importance: row.try_get("importance").unwrap_or_default(),
                confidence: row.try_get("confidence").unwrap_or(0.0),
                updated_at: row.try_get("updated_at").unwrap_or_else(|_| Utc::now()),
                entry_fts: 0.0,
                chunk_fts: 0.0,
                semantic_similarity: 0.0,
                best_chunk_text: row.try_get("best_chunk_text").unwrap_or_default(),
                tags: row.try_get("tags").unwrap_or_default(),
                source_paths: row.try_get("source_paths").unwrap_or_default(),
                graph_boost: 0.0,
                graph_match_count: 0,
                graph_edge_count: 0,
                graph_connections: Vec::new(),
            });
        entry.graph_boost = (entry.graph_boost + boost).min(GRAPH_BOOST_CAP);
        entry.graph_match_count += 1;
        if edge_hit {
            entry.graph_edge_count += 1;
        }
        if entry.graph_connections.len() < MAX_GRAPH_CONNECTIONS_PER_MEMORY {
            entry.graph_connections.push(connection);
        }
    }

    Ok(GraphCandidateOutcome {
        status: "active".to_string(),
        candidates: candidates.into_values().collect(),
    })
}

pub(super) async fn fetch_relation_map(
    pool: &PgPool,
    candidate_ids: &[Uuid],
) -> Result<HashMap<Uuid, Vec<MemoryRelationType>>, sqlx::Error> {
    if candidate_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = sqlx::query(
        r#"
        SELECT src_memory_id, relation_type
        FROM memory_relations
        WHERE src_memory_id = ANY($1)
          AND dst_memory_id = ANY($1)
        "#,
    )
    .bind(candidate_ids)
    .fetch_all(pool)
    .await?;

    let mut map = HashMap::<Uuid, Vec<MemoryRelationType>>::new();
    for row in rows {
        let src_memory_id: Uuid = row.try_get("src_memory_id")?;
        let relation_type = parse_relation_type(&row.try_get::<String, _>("relation_type")?);
        map.entry(src_memory_id).or_default().push(relation_type);
    }
    Ok(map)
}

pub(super) async fn fetch_provenance_rank_map(
    pool: &PgPool,
    candidate_ids: &[Uuid],
) -> Result<HashMap<Uuid, ProvenanceRankSignal>, sqlx::Error> {
    if candidate_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = sqlx::query(
        r#"
        SELECT ms.memory_entry_id, v.status
        FROM memory_sources ms
        LEFT JOIN memory_source_verifications v ON v.source_id = ms.id
        WHERE ms.memory_entry_id = ANY($1)
        "#,
    )
    .bind(candidate_ids)
    .fetch_all(pool)
    .await?;

    let mut map = HashMap::<Uuid, ProvenanceRankSignal>::new();
    for row in rows {
        let memory_id: Uuid = row.try_get("memory_entry_id")?;
        let status: Option<String> = row.try_get("status")?;
        let signal = map.entry(memory_id).or_default();
        let Some(status) = status else {
            signal.unverified_count += 1;
            continue;
        };
        let status = parse_source_provenance_status(&status);
        if provenance_status_rank(&status)
            > signal
                .decay_status
                .as_ref()
                .map_or(0, provenance_status_rank)
        {
            signal.decay_status = match &status {
                SourceProvenanceStatus::MissingFile
                | SourceProvenanceStatus::MissingSymbol
                | SourceProvenanceStatus::Stale => Some(status.clone()),
                SourceProvenanceStatus::Verified | SourceProvenanceStatus::Unverifiable => {
                    signal.decay_status.take()
                }
            };
        }
        if status == SourceProvenanceStatus::Unverifiable {
            signal.unverified_count += 1;
        }
    }
    Ok(map)
}

pub(super) async fn fetch_reinforcement_rank_map(
    pool: &PgPool,
    candidate_ids: &[Uuid],
    half_life_secs: f64,
) -> Result<HashMap<Uuid, ReinforcementRankSignal>, sqlx::Error> {
    if candidate_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = sqlx::query(
        r#"
        SELECT me.id AS memory_id,
               (s.activation * power(
                   0.5,
                   GREATEST(EXTRACT(EPOCH FROM (now() - s.last_decay_at)), 0) / $2
               ))::float8 AS activation,
               s.needs_review
        FROM memory_entries me
        JOIN memory_scores s ON s.canonical_id = me.canonical_id
        WHERE me.id = ANY($1)
        "#,
    )
    .bind(candidate_ids)
    .bind(half_life_secs.max(1.0))
    .fetch_all(pool)
    .await?;

    let mut map = HashMap::new();
    for row in rows {
        let memory_id: Uuid = row.try_get("memory_id")?;
        let activation: f64 = row.try_get("activation")?;
        let needs_review: bool = row.try_get("needs_review")?;
        map.insert(
            memory_id,
            ReinforcementRankSignal {
                activation,
                needs_review,
            },
        );
    }
    Ok(map)
}

fn provenance_status_rank(status: &SourceProvenanceStatus) -> u8 {
    match status {
        SourceProvenanceStatus::MissingFile => 4,
        SourceProvenanceStatus::MissingSymbol => 3,
        SourceProvenanceStatus::Stale => 2,
        SourceProvenanceStatus::Unverifiable => 1,
        SourceProvenanceStatus::Verified => 0,
    }
}

pub(super) async fn fetch_sources(pool: &PgPool, memory_id: Uuid) -> Result<Vec<QuerySource>> {
    let rows = sqlx::query(
        r#"
        SELECT ms.task_id, ms.file_path, ms.symbol_name, ms.symbol_kind, ms.source_kind, ms.excerpt,
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
    .bind(memory_id)
    .fetch_all(pool)
    .await
    .context("query sources")?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let source_kind: String = row.try_get("source_kind")?;
        items.push(QuerySource {
            task_id: row.try_get("task_id")?,
            file_path: row.try_get("file_path")?,
            symbol_name: row.try_get("symbol_name")?,
            symbol_kind: row.try_get("symbol_kind")?,
            source_kind: parse_source_kind(&source_kind),
            excerpt: row.try_get("excerpt")?,
            provenance: source_provenance_from_row(&row)?,
        });
    }
    Ok(items)
}

fn source_provenance_from_row(
    row: &sqlx::postgres::PgRow,
) -> Result<Option<SourceProvenanceRecord>, sqlx::Error> {
    let Some(status) = row.try_get::<Option<String>, _>("provenance_status")? else {
        return Ok(None);
    };
    let checked_at = row.try_get("provenance_checked_at")?;
    Ok(Some(SourceProvenanceRecord {
        status: parse_source_provenance_status(&status),
        checked_at,
        reason: row.try_get("provenance_reason")?,
        resolved_path: row.try_get("provenance_resolved_path")?,
    }))
}
