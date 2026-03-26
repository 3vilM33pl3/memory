use std::{
    collections::{HashMap, HashSet},
    time::Instant,
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use mem_api::{
    AppConfig, EmbeddingConfig, MemoryRelationType, MemoryType, QueryDiagnostics, QueryMatchKind,
    QueryRequest, QueryResponse, QueryResult, QueryResultDebug, QuerySource, SourceKind,
    resolve_secret_value,
};
use pgvector::Vector;
use reqwest::header;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use uuid::Uuid;

const MAX_CANDIDATES: i64 = 64;
const CHUNK_TARGET_SIZE: usize = 320;
const CHUNK_OVERLAP: usize = 80;

#[derive(Clone)]
pub struct EmbeddingService {
    client: reqwest::Client,
    config: EmbeddingConfig,
    api_key: String,
}

#[derive(Debug, Clone)]
struct EmbeddingSpace {
    provider: String,
    base_url: String,
    model: String,
    space_key: String,
}

#[derive(Debug, Clone)]
struct EmbeddingBatch {
    space: EmbeddingSpace,
    dimension: i32,
    vectors: Vec<Vector>,
}

impl EmbeddingService {
    pub fn from_config(config: &AppConfig) -> Option<Self> {
        if config.embeddings.provider.trim() != "openai_compatible"
            || config.embeddings.model.trim().is_empty()
        {
            return None;
        }
        let api_key = resolve_secret_value(&config.embeddings.api_key_env)?;
        if api_key.trim().is_empty() {
            return None;
        }
        Some(Self {
            client: reqwest::Client::new(),
            config: config.embeddings.clone(),
            api_key,
        })
    }

    async fn embed_texts(&self, input: &[String]) -> Result<EmbeddingBatch> {
        if input.is_empty() {
            return Ok(EmbeddingBatch {
                space: self.embedding_space(),
                dimension: 0,
                vectors: Vec::new(),
            });
        }

        let request = EmbeddingRequest {
            model: self.config.model.clone(),
            input: input.to_vec(),
        };
        let response = self
            .client
            .post(format!(
                "{}/embeddings",
                self.config.base_url.trim_end_matches('/')
            ))
            .header(header::AUTHORIZATION, format!("Bearer {}", self.api_key))
            .header(header::CONTENT_TYPE, "application/json")
            .json(&request)
            .send()
            .await
            .context("send embedding request")?;

        let status = response.status();
        let body = response.text().await.context("read embedding response")?;
        if !status.is_success() {
            anyhow::bail!("embedding request failed: {status} {body}");
        }

        let parsed: EmbeddingResponse =
            serde_json::from_str(&body).context("parse embedding response")?;
        let mut data = parsed.data;
        data.sort_by_key(|item| item.index);
        let vectors =
            data.into_iter().map(|item| Vector::from(item.embedding)).collect::<Vec<_>>();
        let dimension = vectors.first().map(vector_dimension).unwrap_or(0);
        Ok(EmbeddingBatch {
            space: self.embedding_space(),
            dimension,
            vectors,
        })
    }

    fn embedding_space(&self) -> EmbeddingSpace {
        let base_url = self.config.base_url.trim_end_matches('/').to_string();
        let provider = self.config.provider.trim().to_string();
        let model = self.config.model.trim().to_string();
        let space_key = format!("{provider}|{base_url}|{model}");
        EmbeddingSpace {
            provider,
            base_url,
            model,
            space_key,
        }
    }
}

fn empty_embedding_batch() -> EmbeddingBatch {
    EmbeddingBatch {
        space: EmbeddingSpace {
            provider: String::new(),
            base_url: String::new(),
            model: String::new(),
            space_key: String::new(),
        },
        dimension: 0,
        vectors: Vec::new(),
    }
}

#[derive(Debug, Serialize)]
struct EmbeddingRequest {
    model: String,
    input: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingItem>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingItem {
    index: usize,
    embedding: Vec<f32>,
}

pub async fn query_memory(
    pool: &PgPool,
    request: &QueryRequest,
    embedder: Option<&EmbeddingService>,
) -> Result<QueryResponse> {
    let total_started = Instant::now();
    let normalized = QueryIntent::from_query(&request.query);
    let candidate_limit = (request.top_k * 8).clamp(request.top_k, MAX_CANDIDATES);

    let lexical_started = Instant::now();
    let lexical_candidates = fetch_lexical_candidates(pool, request, &normalized, candidate_limit)
        .await
        .context("fetch lexical candidates")?;
    let lexical_duration_ms = lexical_started.elapsed().as_millis() as u64;

    let semantic_started = Instant::now();
    let (semantic_candidates, semantic_status) = if let Some(embedder) = embedder {
        match embedder
            .embed_texts(std::slice::from_ref(&request.query))
            .await
        {
            Ok(embedding_batch) => {
                if let Some(query_embedding) = embedding_batch.vectors.into_iter().next() {
                    let candidates = fetch_semantic_candidates(
                        pool,
                        request,
                        &embedding_batch.space,
                        embedding_batch.dimension,
                        &query_embedding,
                        candidate_limit,
                    )
                        .await
                        .context("fetch semantic candidates")?;
                    let semantic_status = if candidates.is_empty()
                        && !project_has_active_embedding_space(
                            pool,
                            &request.project,
                            &embedding_batch.space.space_key,
                            embedding_batch.dimension,
                        )
                        .await
                        .context("check active embedding space coverage")?
                    {
                        "active_space_missing".to_string()
                    } else {
                        "active_space_ok".to_string()
                    };
                    (candidates, semantic_status)
                } else {
                    (Vec::new(), "embedding_probe_empty".to_string())
                }
            }
            Err(_) => (Vec::new(), "embedding_error".to_string()),
        }
    } else {
        (Vec::new(), "disabled".to_string())
    };
    let semantic_duration_ms = semantic_started.elapsed().as_millis() as u64;

    let rerank_started = Instant::now();
    let lexical_count = lexical_candidates.len();
    let semantic_count = semantic_candidates.len();
    let mut candidates = merge_candidates(lexical_candidates, semantic_candidates);
    let merged_candidate_count = candidates.len();
    let relation_map = fetch_relation_map(pool, &candidates.keys().copied().collect::<Vec<_>>())
        .await
        .context("fetch relation map")?;

    let mut ranked = candidates
        .drain()
        .map(|(_, candidate)| rank_candidate(candidate, &normalized, &relation_map))
        .collect::<Vec<_>>();

    ranked.sort_by(|left, right| {
        right
            .final_score
            .total_cmp(&left.final_score)
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| left.memory_id.cmp(&right.memory_id))
    });
    let rerank_duration_ms = rerank_started.elapsed().as_millis() as u64;

    let mut results = Vec::new();
    for candidate in ranked.into_iter().take(request.top_k as usize) {
        if request
            .min_confidence
            .is_some_and(|threshold| candidate.confidence < threshold)
        {
            continue;
        }

        let sources = fetch_sources(pool, candidate.memory_id)
            .await
            .context("fetch query result sources")?;
        results.push(QueryResult {
            memory_id: candidate.memory_id,
            summary: candidate.summary,
            memory_type: candidate.memory_type,
            score: candidate.final_score,
            snippet: candidate.snippet,
            match_kind: candidate.match_kind,
            score_explanation: candidate.score_explanation,
            debug: candidate.debug,
            tags: candidate.tags,
            sources,
        });
    }
    let returned_results = results.len();

    let (answer, confidence, insufficient_evidence) = synthesize_answer(&results);

    Ok(QueryResponse {
        answer,
        confidence,
        results,
        insufficient_evidence,
        diagnostics: QueryDiagnostics {
            lexical_candidates: lexical_count,
            semantic_candidates: semantic_count,
            merged_candidates: merged_candidate_count,
            returned_results,
            relation_augmented_candidates: relation_map.len(),
            lexical_duration_ms,
            semantic_duration_ms,
            rerank_duration_ms,
            total_duration_ms: total_started.elapsed().as_millis() as u64,
            semantic_status,
        },
    })
}

pub async fn rebuild_chunks(
    pool: &PgPool,
    project: &str,
    embedder: Option<&EmbeddingService>,
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
        let memory_id: Uuid = row.try_get("id")?;
        let canonical_text: String = row.try_get("canonical_text")?;
        let summary: String = row.try_get("summary")?;
        sqlx::query("DELETE FROM memory_chunks WHERE memory_entry_id = $1")
            .bind(memory_id)
            .execute(pool)
            .await
            .context("delete old chunks")?;

        let chunks = split_search_chunks(&summary, &canonical_text);
        let embedding_batch = if let Some(embedder) = embedder {
            embedder
                .embed_texts(&chunks)
                .await
                .context("embed rebuilt chunks")?
        } else {
            empty_embedding_batch()
        };

        for (index, chunk_text) in chunks.iter().enumerate() {
            let embedding = embedding_batch.vectors.get(index).cloned();
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
            if let Some(embedding) = embedding {
                upsert_chunk_embedding(pool, chunk_id, &embedding_batch.space, embedding_batch.dimension, embedding)
                    .await
                    .context("upsert rebuilt chunk embedding")?;
            }
        }
        count += 1;
    }
    Ok(count)
}

pub async fn reembed_project_chunks(
    pool: &PgPool,
    project: &str,
    embedder: &EmbeddingService,
) -> Result<u64> {
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
    for batch in rows.chunks(embedder.config.batch_size.max(1)) {
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
            .embed_texts(&texts)
            .await
            .context("embed stale chunks")?;

        for (index, chunk_id) in chunk_ids.iter().enumerate() {
            let embedding = embeddings
                .vectors
                .get(index)
                .cloned()
                .context("missing embedding for stale chunk batch item")?;
            upsert_chunk_embedding(pool, *chunk_id, &embeddings.space, embeddings.dimension, embedding)
                .await
                .context("update active-space chunk embedding")?;
            reembedded_chunks += 1;
        }
    }

    Ok(reembedded_chunks)
}

pub async fn prune_project_embeddings(
    pool: &PgPool,
    project: &str,
    embedder: &EmbeddingService,
) -> Result<u64> {
    let target_space = embedder.embedding_space();
    let result = sqlx::query(
        r#"
        DELETE FROM memory_chunk_embeddings mce
        USING memory_chunks mc, memory_entries m, projects p
        WHERE mce.chunk_id = mc.id
          AND mc.memory_entry_id = m.id
          AND m.project_id = p.id
          AND p.slug = $1
          AND m.status = 'active'
          AND mce.embedding_space <> $2
        "#,
    )
    .bind(project)
    .bind(&target_space.space_key)
    .execute(pool)
    .await
    .context("delete inactive embedding spaces")?;
    Ok(result.rows_affected())
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

pub fn parse_relation_type(value: &str) -> MemoryRelationType {
    match value {
        "duplicates" => MemoryRelationType::Duplicates,
        "supersedes" => MemoryRelationType::Supersedes,
        "supports" => MemoryRelationType::Supports,
        "depends_on" => MemoryRelationType::DependsOn,
        _ => MemoryRelationType::RelatedTo,
    }
}

async fn fetch_sources(pool: &PgPool, memory_id: Uuid) -> Result<Vec<QuerySource>> {
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
    .await
    .context("query sources")?;

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

#[derive(Debug, Clone)]
struct QueryIntent {
    normalized_query: String,
    lexical_terms: Vec<String>,
    exact_phrases: Vec<String>,
    path_terms: Vec<String>,
}

impl QueryIntent {
    fn from_query(query: &str) -> Self {
        let normalized_query = query.split_whitespace().collect::<Vec<_>>().join(" ");
        let exact_phrases = extract_quoted_phrases(&normalized_query);
        let lexical_terms = extract_lexical_terms(&normalized_query);
        let path_terms = lexical_terms
            .iter()
            .filter(|term| is_path_like(term))
            .cloned()
            .collect();
        Self {
            normalized_query,
            lexical_terms,
            exact_phrases,
            path_terms,
        }
    }
}

#[derive(Debug, Clone)]
struct CandidateRecord {
    memory_id: Uuid,
    summary: String,
    memory_type: MemoryType,
    canonical_text: String,
    importance: i32,
    confidence: f32,
    updated_at: DateTime<Utc>,
    entry_fts: f64,
    chunk_fts: f64,
    semantic_similarity: f64,
    best_chunk_text: String,
    tags: Vec<String>,
    source_paths: Vec<String>,
}

#[derive(Debug)]
struct RankedCandidate {
    memory_id: Uuid,
    summary: String,
    memory_type: MemoryType,
    confidence: f32,
    updated_at: DateTime<Utc>,
    tags: Vec<String>,
    snippet: String,
    final_score: f64,
    match_kind: QueryMatchKind,
    debug: QueryResultDebug,
    score_explanation: Vec<String>,
}

async fn fetch_lexical_candidates(
    pool: &PgPool,
    request: &QueryRequest,
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
    .bind(&request.project)
    .bind(&request.query)
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
    .fetch_all(pool)
    .await?;

    rows.into_iter().map(candidate_from_lexical_row).collect()
}

fn candidate_from_lexical_row(row: sqlx::postgres::PgRow) -> Result<CandidateRecord, sqlx::Error> {
    Ok(CandidateRecord {
        memory_id: row.try_get("id")?,
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
    })
}

async fn fetch_semantic_candidates(
    pool: &PgPool,
    request: &QueryRequest,
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
        WHERE p.slug = $1
          AND m.status = 'active'
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
    .bind(&request.project)
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

fn vector_dimension(vector: &Vector) -> i32 {
    vector.as_slice().len() as i32
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

async fn project_has_active_embedding_space(
    pool: &PgPool,
    project: &str,
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
            WHERE p.slug = $1
              AND m.status = 'active'
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

fn merge_candidates(
    lexical: Vec<CandidateRecord>,
    semantic: Vec<CandidateRecord>,
) -> HashMap<Uuid, CandidateRecord> {
    let mut merged = HashMap::new();
    for candidate in lexical.into_iter().chain(semantic) {
        merged
            .entry(candidate.memory_id)
            .and_modify(|existing: &mut CandidateRecord| {
                existing.entry_fts = existing.entry_fts.max(candidate.entry_fts);
                existing.chunk_fts = existing.chunk_fts.max(candidate.chunk_fts);
                if candidate.semantic_similarity > existing.semantic_similarity {
                    existing.semantic_similarity = candidate.semantic_similarity;
                    existing.best_chunk_text = candidate.best_chunk_text.clone();
                }
                if existing.tags.is_empty() {
                    existing.tags = candidate.tags.clone();
                }
                if existing.source_paths.is_empty() {
                    existing.source_paths = candidate.source_paths.clone();
                }
            })
            .or_insert(candidate);
    }
    merged
}

async fn fetch_relation_map(
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

fn rank_candidate(
    candidate: CandidateRecord,
    intent: &QueryIntent,
    relation_map: &HashMap<Uuid, Vec<MemoryRelationType>>,
) -> RankedCandidate {
    let query_lower = intent.normalized_query.to_lowercase();
    let summary_lower = candidate.summary.to_lowercase();
    let canonical_lower = candidate.canonical_text.to_lowercase();
    let snippet_lower = candidate.best_chunk_text.to_lowercase();
    let combined_text = format!("{summary_lower}\n{snippet_lower}\n{canonical_lower}");

    let exact_phrase_matches = if intent.exact_phrases.is_empty() {
        usize::from(
            !query_lower.is_empty()
                && (summary_lower.contains(&query_lower)
                    || canonical_lower.contains(&query_lower)
                    || snippet_lower.contains(&query_lower)),
        )
    } else {
        intent
            .exact_phrases
            .iter()
            .filter(|phrase| combined_text.contains(&phrase.to_lowercase()))
            .count()
    };

    let term_overlap = lexical_overlap_ratio(&combined_text, &intent.lexical_terms);
    let tag_match_count = candidate
        .tags
        .iter()
        .filter(|tag| lexical_match(tag, &intent.lexical_terms))
        .count();
    let path_match_count = candidate
        .source_paths
        .iter()
        .filter(|path| lexical_match(path, &intent.path_terms))
        .count();

    let age_days = (Utc::now() - candidate.updated_at).num_days().max(0) as f64;
    let recency_boost = 1.0 / (1.0 + (age_days / 14.0));
    let relation_boost = relation_map
        .get(&candidate.memory_id)
        .map(|relations| {
            relations
                .iter()
                .map(|relation| match relation {
                    MemoryRelationType::Duplicates => 0.22,
                    MemoryRelationType::Supersedes => 0.35,
                    MemoryRelationType::Supports => 0.28,
                    MemoryRelationType::RelatedTo => 0.18,
                    MemoryRelationType::DependsOn => 0.20,
                })
                .sum::<f64>()
        })
        .unwrap_or(0.0);

    let chunk_score = candidate.chunk_fts * 4.0;
    let entry_score = candidate.entry_fts * 2.5;
    let exact_phrase_boost = exact_phrase_matches as f64 * 1.4;
    let overlap_boost = term_overlap * 1.5;
    let tag_boost = tag_match_count as f64 * 0.9;
    let path_boost = path_match_count as f64 * 1.1;
    let semantic_boost = candidate.semantic_similarity.max(0.0) * 4.2;
    let importance_boost = candidate.importance as f64 * 0.35;
    let confidence_boost = candidate.confidence as f64 * 1.8;
    let recency_score = recency_boost * 0.6;

    let mut final_score = chunk_score
        + entry_score
        + exact_phrase_boost
        + overlap_boost
        + tag_boost
        + path_boost
        + semantic_boost
        + importance_boost
        + confidence_boost
        + recency_score
        + relation_boost;

    if exact_phrase_matches == 0
        && term_overlap < 0.15
        && candidate.chunk_fts == 0.0
        && candidate.entry_fts == 0.0
        && candidate.semantic_similarity < 0.25
    {
        final_score *= 0.65;
    }

    let snippet = summarize_snippet(
        &candidate.best_chunk_text,
        &intent.lexical_terms,
        &intent.exact_phrases,
    );
    let mut score_explanation = Vec::new();
    if candidate.chunk_fts > 0.0 {
        score_explanation.push(format!("strong chunk match {:.2}", candidate.chunk_fts));
    }
    if candidate.entry_fts > 0.0 {
        score_explanation.push(format!("entry search match {:.2}", candidate.entry_fts));
    }
    if candidate.semantic_similarity > 0.0 {
        score_explanation.push(format!(
            "semantic similarity {:.2}",
            candidate.semantic_similarity
        ));
    }
    if exact_phrase_matches > 0 {
        score_explanation.push(format!("exact phrase match x{}", exact_phrase_matches));
    }
    if tag_match_count > 0 {
        score_explanation.push(format!("tag match x{}", tag_match_count));
    }
    if path_match_count > 0 {
        score_explanation.push(format!("source path match x{}", path_match_count));
    }
    if relation_boost > 0.0 {
        score_explanation.push(format!("relation boost {:.2}", relation_boost));
    }
    score_explanation.push(format!("term overlap {:.0}%", term_overlap * 100.0));
    score_explanation.push(format!("importance {}", candidate.importance));
    score_explanation.push(format!("memory confidence {:.2}", candidate.confidence));
    score_explanation.push(format!("updated {}d ago", age_days as i64));

    let lexical_signal = candidate.chunk_fts > 0.0
        || candidate.entry_fts > 0.0
        || exact_phrase_matches > 0
        || tag_match_count > 0
        || path_match_count > 0
        || term_overlap > 0.0;
    let semantic_signal = candidate.semantic_similarity > 0.0;
    let match_kind = match (lexical_signal, semantic_signal) {
        (true, true) => QueryMatchKind::Hybrid,
        (false, true) => QueryMatchKind::Semantic,
        _ => QueryMatchKind::Lexical,
    };

    RankedCandidate {
        memory_id: candidate.memory_id,
        summary: candidate.summary,
        memory_type: candidate.memory_type,
        confidence: candidate.confidence,
        updated_at: candidate.updated_at,
        tags: candidate.tags,
        snippet,
        final_score,
        match_kind,
        debug: QueryResultDebug {
            chunk_fts: candidate.chunk_fts,
            entry_fts: candidate.entry_fts,
            semantic_similarity: candidate.semantic_similarity,
            exact_phrase_matches,
            term_overlap,
            tag_match_count,
            path_match_count,
            relation_boost,
            importance: candidate.importance,
            memory_confidence: candidate.confidence,
            recency_boost,
        },
        score_explanation,
    }
}

fn synthesize_answer(results: &[QueryResult]) -> (String, f32, bool) {
    let Some(top) = results.first() else {
        return (
            "I could not find enough project memory to answer confidently.".to_string(),
            0.0,
            true,
        );
    };

    let best_score = top.score;
    let normalized = (best_score / (best_score + 6.0)).clamp(0.0, 1.0) as f32;
    let strong_results = results
        .iter()
        .take(3)
        .filter(|result| result.score >= best_score * 0.72)
        .collect::<Vec<_>>();

    let insufficient = strong_results.is_empty()
        || normalized < 0.38
        || strong_results[0]
            .score_explanation
            .iter()
            .all(|item| item.starts_with("term overlap 0%"));

    if insufficient {
        return (
            "I could not find enough project memory to answer confidently.".to_string(),
            normalized.min(0.3),
            true,
        );
    }

    let mut summaries = Vec::new();
    let mut seen = HashSet::new();
    for result in strong_results {
        let normalized_summary = result.summary.to_lowercase();
        if seen.insert(normalized_summary) {
            summaries.push(result.summary.clone());
        }
    }

    let answer = match summaries.as_slice() {
        [] => "I could not find enough project memory to answer confidently.".to_string(),
        [only] => only.to_string(),
        [first, second] => format!("{first} Also relevant: {second}."),
        [first, second, third, ..] => {
            format!("{first} Also relevant: {second}. Supporting detail: {third}.")
        }
    };

    let confidence = (normalized + ((summaries.len().saturating_sub(1) as f32) * 0.08)).min(0.95);
    (answer, confidence, false)
}

fn extract_quoted_phrases(query: &str) -> Vec<String> {
    let mut phrases = Vec::new();
    let mut current = String::new();
    let mut quote_char = None;
    let mut escaped = false;
    for ch in query.chars() {
        if escaped {
            if quote_char.is_some() {
                current.push(ch);
            }
            escaped = false;
            continue;
        }

        match (quote_char, ch) {
            (_, '\\') => escaped = true,
            (None, '"' | '\'') => {
                quote_char = Some(ch);
                current.clear();
            }
            (Some(active), ch) if ch == active => {
                let phrase = current.split_whitespace().collect::<Vec<_>>().join(" ");
                if !phrase.is_empty() {
                    phrases.push(phrase);
                }
                current.clear();
                quote_char = None;
            }
            (Some(_), ch) => current.push(ch),
            _ => {}
        }
    }
    phrases
}

fn extract_lexical_terms(query: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    query
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-')))
        .filter_map(|raw| {
            let term = raw.trim().to_lowercase();
            if term.len() < 2 || !seen.insert(term.clone()) {
                None
            } else {
                Some(term)
            }
        })
        .collect()
}

fn lexical_overlap_ratio(text: &str, terms: &[String]) -> f64 {
    if terms.is_empty() {
        return 0.0;
    }
    let matched = terms
        .iter()
        .filter(|term| text.contains(term.as_str()))
        .count();
    matched as f64 / terms.len() as f64
}

fn lexical_match(text: &str, terms: &[String]) -> bool {
    if terms.is_empty() {
        return false;
    }
    let lowered = text.to_lowercase();
    terms.iter().any(|term| lowered.contains(term))
}

fn is_path_like(term: &str) -> bool {
    term.contains('/') || term.contains('.') || term.contains('_') || term.contains('-')
}

fn summarize_snippet(text: &str, lexical_terms: &[String], phrases: &[String]) -> String {
    let trimmed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if trimmed.len() <= 240 {
        return trimmed;
    }

    let lowered = trimmed.to_lowercase();
    let target = phrases
        .iter()
        .map(|value| value.to_lowercase())
        .chain(lexical_terms.iter().cloned())
        .find_map(|needle| lowered.find(&needle));

    if let Some(index) = target {
        let start = index.saturating_sub(80);
        let end = (start + 240).min(trimmed.len());
        let prefix = if start > 0 { "..." } else { "" };
        let suffix = if end < trimmed.len() { "..." } else { "" };
        return format!("{prefix}{}{suffix}", &trimmed[start..end]);
    }

    format!("{}...", &trimmed[..240])
}

pub fn split_search_chunks(summary: &str, canonical_text: &str) -> Vec<String> {
    let normalized_summary = summary.split_whitespace().collect::<Vec<_>>().join(" ");
    let normalized_text = canonical_text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let text = if normalized_summary.is_empty() {
        normalized_text
    } else if normalized_text.is_empty() {
        normalized_summary
    } else {
        format!("{normalized_summary}\n{normalized_text}")
    };
    if text.len() <= CHUNK_TARGET_SIZE {
        return vec![text];
    }

    let mut chunks = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + CHUNK_TARGET_SIZE).min(text.len());
        while end < text.len() && !text.is_char_boundary(end) {
            end -= 1;
        }
        if let Some(relative) = text[start..end].rfind(' ') {
            let candidate = start + relative;
            if candidate > start + (CHUNK_TARGET_SIZE / 2) {
                end = candidate;
            }
        }

        let chunk = text[start..end].trim();
        if !chunk.is_empty() {
            chunks.push(chunk.to_string());
        }

        if end >= text.len() {
            break;
        }

        start = end.saturating_sub(CHUNK_OVERLAP);
        while start < text.len() && !text.is_char_boundary(start) {
            start += 1;
        }
    }

    if chunks.is_empty() {
        vec![format!("{summary}\n{canonical_text}")]
    } else {
        chunks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_quoted_phrases() {
        assert_eq!(
            extract_quoted_phrases(r#"why \"repo root\" uses "memory watch""#),
            vec!["memory watch".to_string()]
        );
    }

    #[test]
    fn extracts_lexical_terms_and_paths() {
        let intent = QueryIntent::from_query("memory watch .mem/config.toml project");
        assert!(intent.lexical_terms.contains(&"memory".to_string()));
        assert!(
            intent.path_terms.contains(&".mem".to_string())
                || intent
                    .path_terms
                    .iter()
                    .any(|term| term.contains("config.toml"))
        );
    }

    #[test]
    fn chunking_splits_long_text() {
        let text = "alpha ".repeat(200);
        let chunks = split_search_chunks("summary", &text);
        assert!(chunks.len() > 1);
        assert!(
            chunks
                .iter()
                .all(|chunk| chunk.len() <= CHUNK_TARGET_SIZE + 16)
        );
        assert!(
            chunks
                .first()
                .is_some_and(|chunk| chunk.contains("summary"))
        );
    }

    #[test]
    fn snippet_prefers_matching_term() {
        let text = format!("{} needle {}", "alpha ".repeat(80), "beta ".repeat(80));
        let snippet = summarize_snippet(&text, &["needle".to_string()], &[]);
        assert!(snippet.contains("needle"));
    }

    #[test]
    fn vector_wrapper_preserves_embedding_length() {
        let vector = Vector::from(vec![1.0, 2.0, 3.0]);
        assert_eq!(vector, Vector::from(vec![1.0, 2.0, 3.0]));
    }

    #[test]
    fn synthesize_answer_prefers_multiple_strong_results() {
        let results = vec![
            QueryResult {
                memory_id: Uuid::new_v4(),
                summary: "Primary summary".to_string(),
                memory_type: MemoryType::Architecture,
                score: 7.0,
                snippet: "Primary snippet".to_string(),
                match_kind: QueryMatchKind::Lexical,
                score_explanation: vec![
                    "strong chunk match 1.20".to_string(),
                    "term overlap 100%".to_string(),
                ],
                debug: QueryResultDebug::default(),
                tags: vec![],
                sources: vec![],
            },
            QueryResult {
                memory_id: Uuid::new_v4(),
                summary: "Secondary summary".to_string(),
                memory_type: MemoryType::Convention,
                score: 5.5,
                snippet: "Secondary snippet".to_string(),
                match_kind: QueryMatchKind::Semantic,
                score_explanation: vec![
                    "semantic similarity 0.84".to_string(),
                    "term overlap 67%".to_string(),
                ],
                debug: QueryResultDebug::default(),
                tags: vec![],
                sources: vec![],
            },
        ];

        let (answer, confidence, insufficient) = synthesize_answer(&results);
        assert!(answer.contains("Primary summary"));
        assert!(answer.contains("Secondary summary"));
        assert!(confidence > 0.45);
        assert!(!insufficient);
    }
}
