use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Instant,
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use mem_api::{
    DiagnosticInfo, DiagnosticSeverity, EmbeddingBackendConfig, EmbeddingsConfig,
    GlobalQueryRequest, MemoryRelationType, MemoryType, ProvenanceConfig, QueryAnswerCitation,
    QueryAnswerGeneration, QueryAnswerMethod, QueryDiagnostics, QueryFilters, QueryGraphConnection,
    QueryMatchKind, QueryRequest, QueryResponse, QueryResult, QueryResultDebug, QueryRetrievalMode,
    QuerySource, SourceKind, SourceProvenanceRecord, SourceProvenanceStatus,
};
use pgvector::Vector;
use sqlx::PgPool;
use uuid::Uuid;

mod embedding_backend;
mod repository;

use embedding_backend::{EmbeddingBackend, EmbeddingSpace};
pub use embedding_backend::{EmbeddingPurpose, effective_embedding_base_url};

const MAX_CANDIDATES: i64 = 64;
const GRAPH_DIRECT_BOOST: f64 = 1.25;
const GRAPH_REFERENCE_BOOST: f64 = 1.0;
const GRAPH_NEIGHBOR_BOOST: f64 = 0.65;
const GRAPH_BOOST_CAP: f64 = 2.5;
const MAX_GRAPH_CONNECTIONS_PER_MEMORY: usize = 5;
const CHUNK_TARGET_SIZE: usize = 320;
const CHUNK_OVERLAP: usize = 80;

pub(crate) struct QueryExecution<'a> {
    project: Option<&'a str>,
    query: &'a str,
    filters: &'a QueryFilters,
    top_k: i64,
    min_confidence: Option<f32>,
    include_stale: bool,
    history: bool,
    retrieval_mode: Option<QueryRetrievalMode>,
}

impl<'a> QueryExecution<'a> {
    fn from_project_request(request: &'a QueryRequest) -> Self {
        Self {
            project: Some(request.project.as_str()),
            query: request.query.as_str(),
            filters: &request.filters,
            top_k: request.top_k,
            min_confidence: request.min_confidence,
            include_stale: request.include_stale,
            history: request.history,
            retrieval_mode: request.retrieval_mode,
        }
    }

    fn from_global_request(request: &'a GlobalQueryRequest) -> Self {
        Self {
            project: None,
            query: request.query.as_str(),
            filters: &request.filters,
            top_k: request.top_k,
            min_confidence: request.min_confidence,
            include_stale: request.include_stale,
            history: request.history,
            retrieval_mode: request.retrieval_mode,
        }
    }
}

/// Wrapper around a single embedding backend. The trait lives in
/// `embedding_backend`; this struct exists so the rest of the crate keeps a
/// stable `EmbeddingService` type without depending on the trait object
/// directly.
#[derive(Clone)]
pub struct EmbeddingService {
    backend: Arc<dyn EmbeddingBackend>,
    batch_size: usize,
}

#[derive(Debug, Clone)]
struct EmbeddingBatch {
    space: EmbeddingSpace,
    dimension: i32,
    vectors: Vec<Vector>,
}

impl EmbeddingService {
    pub fn from_backend_config(config: &EmbeddingBackendConfig) -> Option<Self> {
        let backend = embedding_backend::build_backend(config)?;
        Some(Self {
            backend,
            batch_size: config.batch_size.max(1),
        })
    }

    pub fn batch_size(&self) -> usize {
        self.batch_size
    }

    async fn embed_texts(
        &self,
        input: &[String],
        purpose: EmbeddingPurpose,
    ) -> Result<EmbeddingBatch> {
        if input.is_empty() {
            return Ok(EmbeddingBatch {
                space: self.backend.space().clone(),
                dimension: 0,
                vectors: Vec::new(),
            });
        }
        let vectors = self
            .backend
            .embed(input, purpose)
            .await
            .context("embedding backend request")?;
        let dimension = vectors.first().map(vector_dimension).unwrap_or(0);
        Ok(EmbeddingBatch {
            space: self.backend.space().clone(),
            dimension,
            vectors,
        })
    }

    fn embedding_space(&self) -> EmbeddingSpace {
        self.backend.space().clone()
    }

    pub fn embedding_space_key(&self) -> String {
        self.backend.space().space_key.clone()
    }
}

/// Holds one `EmbeddingService` per configured backend plus which one is
/// currently active for search. Write paths iterate every entry so all
/// configured spaces stay populated.
#[derive(Clone, Default)]
pub struct EmbeddingRegistry {
    entries: Vec<EmbeddingRegistryEntry>,
    active: Option<String>,
}

#[derive(Clone)]
struct EmbeddingRegistryEntry {
    name: String,
    service: EmbeddingService,
    create_enabled: bool,
}

impl EmbeddingRegistry {
    /// Build a registry from a full `EmbeddingsConfig`, silently skipping
    /// any backend that fails to resolve (missing model, missing API key).
    /// Those are surfaced by `memory doctor`; the service should still
    /// start.
    pub fn from_config(config: &EmbeddingsConfig) -> Self {
        let mut entries = Vec::new();
        for backend in &config.backends {
            if let Some(service) = EmbeddingService::from_backend_config(backend) {
                entries.push(EmbeddingRegistryEntry {
                    name: backend.name.clone(),
                    service,
                    create_enabled: backend.create_enabled,
                });
            }
        }
        let active = config.active_backend().map(|backend| backend.name.clone());
        let active = active.filter(|name| entries.iter().any(|entry| &entry.name == name));
        Self { entries, active }
    }

    /// Backend currently used for search, or `None` when nothing is
    /// configured or the configured active name is not resolvable
    /// (e.g. missing API key).
    pub fn active(&self) -> Option<&EmbeddingService> {
        let name = self.active.as_deref()?;
        self.entries
            .iter()
            .find_map(|entry| (entry.name == name).then_some(&entry.service))
    }

    /// Name of the currently-active backend.
    pub fn active_name(&self) -> Option<&str> {
        self.active.as_deref()
    }

    /// Look up a backend by its configured name.
    pub fn get(&self, name: &str) -> Option<&EmbeddingService> {
        self.entries
            .iter()
            .find_map(|entry| (entry.name == name).then_some(&entry.service))
    }

    pub fn create_enabled(&self, name: &str) -> bool {
        self.entries
            .iter()
            .find_map(|entry| (entry.name == name).then_some(entry.create_enabled))
            .unwrap_or(true)
    }

    pub fn set_create_enabled(&mut self, name: &str, enabled: bool) -> Result<(), ActivateError> {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.name == name) {
            entry.create_enabled = enabled;
            Ok(())
        } else {
            Err(ActivateError::UnknownBackend(name.to_string()))
        }
    }

    /// Every (name, backend) pair, in config order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &EmbeddingService)> {
        self.entries
            .iter()
            .map(|entry| (entry.name.as_str(), &entry.service))
    }

    /// Backends that automatic curation/import writes should embed into.
    pub fn iter_create_enabled(&self) -> impl Iterator<Item = (&str, &EmbeddingService)> {
        self.entries
            .iter()
            .filter(|entry| entry.create_enabled)
            .map(|entry| (entry.name.as_str(), &entry.service))
    }

    /// Names of all configured backends.
    pub fn names(&self) -> Vec<String> {
        self.entries
            .iter()
            .map(|entry| entry.name.clone())
            .collect()
    }

    /// Whether any backends are configured (even if none are active).
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Switch the active backend in-memory. Returns an error when the
    /// requested name is not configured; persistence (config-file
    /// rewrite) is a caller responsibility.
    pub fn set_active(&mut self, name: &str) -> Result<(), ActivateError> {
        if self.entries.iter().any(|entry| entry.name == name) {
            self.active = Some(name.to_string());
            Ok(())
        } else {
            Err(ActivateError::UnknownBackend(name.to_string()))
        }
    }

    /// Disable semantic search without removing configured backends.
    pub fn clear_active(&mut self) {
        self.active = None;
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ActivateError {
    #[error("unknown embedding backend: {0}")]
    UnknownBackend(String),
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

pub async fn query_memory(
    pool: &PgPool,
    request: &QueryRequest,
    embedder: Option<&EmbeddingService>,
) -> Result<QueryResponse> {
    query_memory_with_provenance_config(pool, request, embedder, &ProvenanceConfig::default()).await
}

pub async fn query_memory_with_provenance_config(
    pool: &PgPool,
    request: &QueryRequest,
    embedder: Option<&EmbeddingService>,
    provenance_config: &ProvenanceConfig,
) -> Result<QueryResponse> {
    query_memory_with_configs(
        pool,
        request,
        embedder,
        provenance_config,
        &ReinforcementRankParams::default(),
    )
    .await
}

pub async fn query_memory_with_configs(
    pool: &PgPool,
    request: &QueryRequest,
    embedder: Option<&EmbeddingService>,
    provenance_config: &ProvenanceConfig,
    reinforcement: &ReinforcementRankParams,
) -> Result<QueryResponse> {
    let execution = QueryExecution::from_project_request(request);
    query_memory_execution(pool, &execution, embedder, provenance_config, reinforcement).await
}

pub async fn query_memory_global(
    pool: &PgPool,
    request: &GlobalQueryRequest,
    embedder: Option<&EmbeddingService>,
) -> Result<QueryResponse> {
    query_memory_global_with_provenance_config(
        pool,
        request,
        embedder,
        &ProvenanceConfig::default(),
    )
    .await
}

pub async fn query_memory_global_with_provenance_config(
    pool: &PgPool,
    request: &GlobalQueryRequest,
    embedder: Option<&EmbeddingService>,
    provenance_config: &ProvenanceConfig,
) -> Result<QueryResponse> {
    query_memory_global_with_configs(
        pool,
        request,
        embedder,
        provenance_config,
        &ReinforcementRankParams::default(),
    )
    .await
}

pub async fn query_memory_global_with_configs(
    pool: &PgPool,
    request: &GlobalQueryRequest,
    embedder: Option<&EmbeddingService>,
    provenance_config: &ProvenanceConfig,
    reinforcement: &ReinforcementRankParams,
) -> Result<QueryResponse> {
    let execution = QueryExecution::from_global_request(request);
    query_memory_execution(pool, &execution, embedder, provenance_config, reinforcement).await
}

async fn query_memory_execution(
    pool: &PgPool,
    request: &QueryExecution<'_>,
    embedder: Option<&EmbeddingService>,
    provenance_config: &ProvenanceConfig,
    reinforcement: &ReinforcementRankParams,
) -> Result<QueryResponse> {
    let total_started = Instant::now();
    let normalized = QueryIntent::from_query(request.query);
    let candidate_limit = (request.top_k * 8).clamp(request.top_k, MAX_CANDIDATES);
    let retrieval_mode = request.retrieval_mode.unwrap_or_default();
    let lexical_enabled = matches!(
        retrieval_mode,
        QueryRetrievalMode::Lexical | QueryRetrievalMode::FullMemory
    );
    let semantic_enabled = matches!(
        retrieval_mode,
        QueryRetrievalMode::Semantic | QueryRetrievalMode::FullMemory
    );
    let graph_enabled = matches!(
        retrieval_mode,
        QueryRetrievalMode::Graph | QueryRetrievalMode::FullMemory
    );
    let relation_boost_enabled = matches!(retrieval_mode, QueryRetrievalMode::FullMemory);

    let lexical_started = Instant::now();
    let lexical_candidates = if lexical_enabled {
        repository::fetch_lexical_candidates(pool, request, &normalized, candidate_limit)
            .await
            .context("fetch lexical candidates")?
    } else {
        Vec::new()
    };
    let lexical_duration_ms = lexical_started.elapsed().as_millis() as u64;

    let semantic_started = Instant::now();
    let (semantic_candidates, semantic_status) = if !semantic_enabled {
        (Vec::new(), "disabled_by_mode".to_string())
    } else if let Some(embedder) = embedder {
        let query_text = request.query.to_string();
        match embedder
            .embed_texts(std::slice::from_ref(&query_text), EmbeddingPurpose::Query)
            .await
        {
            Ok(embedding_batch) => {
                if let Some(query_embedding) = embedding_batch.vectors.into_iter().next() {
                    let candidates = repository::fetch_semantic_candidates(
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
                        && !repository::scope_has_active_embedding_space(
                            pool,
                            request.project,
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

    let graph_started = Instant::now();
    let (graph_candidates, graph_status) = if graph_enabled {
        // The graph channel is bounded so a slow code-graph scan can never
        // sink the whole query; on timeout it degrades to an empty channel
        // with a diagnostic status, exactly like an error.
        match tokio::time::timeout(
            graph_channel_timeout(),
            repository::fetch_graph_candidates(pool, request, &normalized, candidate_limit),
        )
        .await
        {
            Ok(Ok(outcome)) => (outcome.candidates, outcome.status),
            Ok(Err(_)) => (Vec::new(), "error".to_string()),
            Err(_) => (Vec::new(), "timed_out".to_string()),
        }
    } else {
        (Vec::new(), "disabled_by_mode".to_string())
    };
    let graph_duration_ms = graph_started.elapsed().as_millis() as u64;

    let rerank_started = Instant::now();
    let lexical_count = lexical_candidates.len();
    let semantic_count = semantic_candidates.len();
    let graph_count = graph_candidates.len();
    let mut candidates =
        merge_candidates(lexical_candidates, semantic_candidates, graph_candidates);
    let merged_candidate_count = candidates.len();
    let graph_augmented_candidates = candidates
        .values()
        .filter(|candidate| candidate.graph_match_count > 0)
        .count();
    let relation_map = if relation_boost_enabled {
        repository::fetch_relation_map(pool, &candidates.keys().copied().collect::<Vec<_>>())
            .await
            .context("fetch relation map")?
    } else {
        HashMap::new()
    };
    let provenance_map = repository::fetch_provenance_rank_map(
        pool,
        &candidates.keys().copied().collect::<Vec<_>>(),
    )
    .await
    .context("fetch provenance rank map")?;
    let provenance_decayed_candidates = provenance_map
        .values()
        .filter(|signal| signal.decay_status.is_some() && !request.include_stale)
        .count();
    let provenance_unverified_candidates = provenance_map
        .values()
        .filter(|signal| signal.unverified_count > 0)
        .count();
    let reinforcement_map = if reinforcement.active() {
        repository::fetch_reinforcement_rank_map(
            pool,
            &candidates.keys().copied().collect::<Vec<_>>(),
            reinforcement.half_life_secs,
        )
        .await
        .context("fetch reinforcement rank map")?
    } else {
        HashMap::new()
    };

    let mut ranked = candidates
        .drain()
        .map(|(_, candidate)| {
            let provenance = provenance_map.get(&candidate.memory_id);
            let reinforcement_signal = reinforcement_map.get(&candidate.memory_id);
            rank_candidate(
                candidate,
                &normalized,
                &relation_map,
                provenance,
                provenance_config,
                request.include_stale,
                reinforcement_signal,
                reinforcement,
            )
        })
        .collect::<Vec<_>>();

    ranked.sort_by(compare_ranked);
    let rerank_duration_ms = rerank_started.elapsed().as_millis() as u64;

    let mut results = Vec::new();
    for candidate in ranked.into_iter().take(request.top_k as usize) {
        if request
            .min_confidence
            .is_some_and(|threshold| candidate.confidence < threshold)
        {
            continue;
        }

        let sources = repository::fetch_sources(pool, candidate.memory_id)
            .await
            .context("fetch query result sources")?;
        results.push(QueryResult {
            memory_id: candidate.memory_id,
            project: candidate.project,
            project_name: candidate.project_name,
            repo_root: candidate.repo_root,
            summary: candidate.summary,
            memory_type: candidate.memory_type,
            score: candidate.final_score,
            snippet: candidate.snippet,
            match_kind: candidate.match_kind,
            score_explanation: candidate.score_explanation,
            debug: candidate.debug,
            tags: candidate.tags,
            sources,
            graph_connections: candidate.graph_connections,
            needs_review: candidate.needs_review,
        });
    }
    let returned_results = results.len();

    let provenance_warnings = provenance_warnings_for_results(&results);
    let synthesis = synthesize_answer(&results);

    Ok(QueryResponse {
        answer: synthesis.answer,
        confidence: synthesis.confidence,
        results,
        insufficient_evidence: synthesis.insufficient_evidence,
        answer_generation: synthesis.answer_generation,
        answer_citations: synthesis.answer_citations,
        diagnostics: QueryDiagnostics {
            retrieval_mode,
            lexical_enabled,
            semantic_enabled,
            graph_enabled,
            relation_boost_enabled,
            lexical_candidates: lexical_count,
            semantic_candidates: semantic_count,
            merged_candidates: merged_candidate_count,
            returned_results,
            relation_augmented_candidates: relation_map.len(),
            graph_candidates: graph_count,
            graph_augmented_candidates,
            provenance_decayed_candidates,
            provenance_unverified_candidates,
            lexical_duration_ms,
            semantic_duration_ms,
            rerank_duration_ms,
            graph_duration_ms,
            total_duration_ms: total_started.elapsed().as_millis() as u64,
            semantic_status,
            graph_status,
            provenance_warnings,
        },
    })
}

pub async fn rebuild_chunks(
    pool: &PgPool,
    project: &str,
    registry: &EmbeddingRegistry,
    target_backend: Option<&str>,
) -> Result<u64> {
    if let Some(name) = target_backend {
        // Chunks are shared by every embedding backend. Rebuilding chunks for a
        // single backend would delete the shared chunk rows and cascade-delete
        // embeddings for other backends. Treat a backend-scoped rebuild as a
        // safe backfill of missing vectors in that backend's space instead.
        return reembed_project_chunks(pool, project, registry, Some(name)).await;
    }

    let selected: Vec<(&str, &EmbeddingService)> = registry.iter().collect();
    repository::rebuild_chunks_selected(pool, project, selected).await
}

pub async fn rebuild_chunks_for_automatic_creation(
    pool: &PgPool,
    project: &str,
    registry: &EmbeddingRegistry,
    global_create_enabled: bool,
) -> Result<u64> {
    let selected: Vec<(&str, &EmbeddingService)> = if global_create_enabled {
        registry.iter_create_enabled().collect()
    } else {
        Vec::new()
    };

    repository::rebuild_chunks_selected(pool, project, selected).await
}

pub async fn rebuild_memory_chunks_for_automatic_creation(
    pool: &PgPool,
    project: &str,
    memory_ids: &[Uuid],
    registry: &EmbeddingRegistry,
    global_create_enabled: bool,
) -> Result<u64> {
    let selected: Vec<(&str, &EmbeddingService)> = if global_create_enabled {
        registry.iter_create_enabled().collect()
    } else {
        Vec::new()
    };

    repository::rebuild_memory_chunks_selected(pool, project, memory_ids, selected).await
}

pub async fn reembed_project_chunks(
    pool: &PgPool,
    project: &str,
    registry: &EmbeddingRegistry,
    target_backend: Option<&str>,
) -> Result<u64> {
    let selected: Vec<(&str, &EmbeddingService)> = match target_backend {
        Some(name) => registry
            .get(name)
            .map(|service| vec![(name, service)])
            .ok_or_else(|| anyhow::anyhow!("unknown embedding backend: {name}"))?,
        None => registry.iter().collect(),
    };

    let mut total_reembedded = 0u64;
    for (_, embedder) in &selected {
        total_reembedded += repository::reembed_single_backend(pool, project, embedder).await?;
    }
    Ok(total_reembedded)
}

pub async fn prune_project_embeddings(
    pool: &PgPool,
    project: &str,
    registry: &EmbeddingRegistry,
) -> Result<u64> {
    let keep: Vec<String> = registry
        .iter()
        .map(|(_, service)| service.embedding_space_key())
        .collect();
    repository::prune_project_embeddings(pool, project, &keep).await
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
        "documentation" => MemoryType::Documentation,
        "task" => MemoryType::Task,
        "plan" => MemoryType::Plan,
        "implementation" => MemoryType::Implementation,
        "refactor" => MemoryType::Refactor,
        "user" => MemoryType::User,
        "feedback" => MemoryType::Feedback,
        "project" => MemoryType::Project,
        "reference" => MemoryType::Reference,
        "insight" => MemoryType::Insight,
        _ => MemoryType::Convention,
    }
}

pub fn parse_relation_type(value: &str) -> MemoryRelationType {
    match value {
        "duplicates" => MemoryRelationType::Duplicates,
        "supersedes" => MemoryRelationType::Supersedes,
        "supports" => MemoryRelationType::Supports,
        "depends_on" => MemoryRelationType::DependsOn,
        "summarizes" => MemoryRelationType::Summarizes,
        _ => MemoryRelationType::RelatedTo,
    }
}

fn parse_source_provenance_status(value: &str) -> SourceProvenanceStatus {
    match value {
        "verified" => SourceProvenanceStatus::Verified,
        "missing_file" => SourceProvenanceStatus::MissingFile,
        "missing_symbol" => SourceProvenanceStatus::MissingSymbol,
        "stale" => SourceProvenanceStatus::Stale,
        _ => SourceProvenanceStatus::Unverifiable,
    }
}

fn provenance_warnings_for_results(results: &[QueryResult]) -> Vec<DiagnosticInfo> {
    let mut warnings = Vec::new();
    for result in results {
        for source in &result.sources {
            let Some(provenance) = &source.provenance else {
                warnings.push(DiagnosticInfo {
                    code: "provenance_unverified".to_string(),
                    source: "memory".to_string(),
                    component: "search".to_string(),
                    operation: "query".to_string(),
                    severity: DiagnosticSeverity::Info,
                    message: format!(
                        "Memory {} has an unverified source {}",
                        result.memory_id,
                        source
                            .file_path
                            .as_deref()
                            .unwrap_or("<unknown source path>")
                    ),
                    raw_error: None,
                    explanation: None,
                    fix_hint: Some(
                        "Run `memory verify-provenance --project <slug>` to verify source citations."
                            .to_string(),
                    ),
                    doctor_hint: None,
                    command_hint: Some("memory verify-provenance --project <slug>".to_string()),
                });
                continue;
            };
            if !matches!(
                provenance.status,
                SourceProvenanceStatus::MissingFile
                    | SourceProvenanceStatus::MissingSymbol
                    | SourceProvenanceStatus::Stale
            ) {
                continue;
            }
            let path = source
                .file_path
                .as_deref()
                .unwrap_or("<unknown source path>");
            warnings.push(DiagnosticInfo {
                code: "stale_memory_provenance".to_string(),
                source: "memory".to_string(),
                component: "search".to_string(),
                operation: "query".to_string(),
                severity: DiagnosticSeverity::Warning,
                message: format!(
                    "Memory {} cites {} with provenance status {}",
                    result.memory_id,
                    path,
                    provenance.status.as_str()
                ),
                raw_error: None,
                explanation: provenance.reason.clone(),
                fix_hint: Some(
                    "Run `memory verify-provenance --project <slug>` and review stale citations."
                        .to_string(),
                ),
                doctor_hint: None,
                command_hint: Some("memory verify-provenance --project <slug>".to_string()),
            });
        }
    }
    warnings
}

pub fn parse_source_kind(value: &str) -> SourceKind {
    match value {
        "task_prompt" => SourceKind::TaskPrompt,
        "file" => SourceKind::File,
        "git_commit" => SourceKind::GitCommit,
        "command_output" => SourceKind::CommandOutput,
        "test" => SourceKind::Test,
        "note" => SourceKind::Note,
        "memory" => SourceKind::Memory,
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
    project: Option<String>,
    project_name: Option<String>,
    repo_root: Option<String>,
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
    graph_boost: f64,
    graph_match_count: usize,
    graph_edge_count: usize,
    graph_connections: Vec<QueryGraphConnection>,
}

#[derive(Debug)]
struct RankedCandidate {
    memory_id: Uuid,
    project: Option<String>,
    project_name: Option<String>,
    repo_root: Option<String>,
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
    graph_connections: Vec<QueryGraphConnection>,
    needs_review: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ProvenanceRankSignal {
    pub(crate) decay_status: Option<SourceProvenanceStatus>,
    pub(crate) unverified_count: usize,
}

/// Decay-corrected activation state joined from memory_scores at query time.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ReinforcementRankSignal {
    pub(crate) activation: f64,
    pub(crate) needs_review: bool,
}

/// Ranking-side view of the reinforcement config: how strongly activation
/// boosts results and how hard flagged memories are penalized.
#[derive(Debug, Clone)]
pub struct ReinforcementRankParams {
    pub enabled: bool,
    pub weight: f64,
    pub cap: f64,
    pub needs_review_penalty: f64,
    pub half_life_secs: f64,
}

impl Default for ReinforcementRankParams {
    fn default() -> Self {
        Self::from(&mem_api::ReinforcementConfig::default())
    }
}

impl From<&mem_api::ReinforcementConfig> for ReinforcementRankParams {
    fn from(config: &mem_api::ReinforcementConfig) -> Self {
        Self {
            enabled: config.enabled,
            weight: config.activation_rank_weight,
            cap: config.activation_rank_cap,
            needs_review_penalty: config.needs_review_rank_penalty,
            half_life_secs: config.half_life.as_secs_f64().max(1.0),
        }
    }
}

impl ReinforcementRankParams {
    fn active(&self) -> bool {
        self.enabled && self.weight > 0.0
    }
}

fn vector_dimension(vector: &Vector) -> i32 {
    vector.as_slice().len() as i32
}

fn graph_like_terms(intent: &QueryIntent) -> Vec<String> {
    let mut terms = intent
        .lexical_terms
        .iter()
        .chain(intent.exact_phrases.iter())
        .filter(|term| is_graph_search_term(term))
        .filter(|term| term.len() >= 3)
        .map(|term| format!("%{term}%"))
        .collect::<Vec<_>>();
    terms.sort();
    terms.dedup();
    terms
}

fn is_graph_search_term(term: &str) -> bool {
    const GRAPH_STOP_TERMS: &[&str] = &[
        "about",
        "and",
        "are",
        "different",
        "does",
        "from",
        "how",
        "memory",
        "project",
        "query",
        "result",
        "results",
        "the",
        "type",
        "types",
        "what",
        "when",
        "where",
        "which",
        "why",
        "work",
        "works",
        "with",
    ];

    !GRAPH_STOP_TERMS.contains(&term)
}

fn merge_candidates(
    lexical: Vec<CandidateRecord>,
    semantic: Vec<CandidateRecord>,
    graph: Vec<CandidateRecord>,
) -> HashMap<Uuid, CandidateRecord> {
    let mut merged = HashMap::<Uuid, CandidateRecord>::new();
    for candidate in lexical.into_iter().chain(semantic).chain(graph) {
        if let Some(existing) = merged.get_mut(&candidate.memory_id) {
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
            existing.graph_boost =
                (existing.graph_boost + candidate.graph_boost).min(GRAPH_BOOST_CAP);
            existing.graph_match_count += candidate.graph_match_count;
            existing.graph_edge_count += candidate.graph_edge_count;
            for connection in candidate.graph_connections {
                if existing.graph_connections.len() >= MAX_GRAPH_CONNECTIONS_PER_MEMORY {
                    break;
                }
                existing.graph_connections.push(connection);
            }
        } else {
            merged.insert(candidate.memory_id, candidate);
        }
    }
    merged
}

#[allow(clippy::too_many_arguments)]
fn rank_candidate(
    candidate: CandidateRecord,
    intent: &QueryIntent,
    relation_map: &HashMap<Uuid, Vec<MemoryRelationType>>,
    provenance: Option<&ProvenanceRankSignal>,
    provenance_config: &ProvenanceConfig,
    include_stale: bool,
    reinforcement_signal: Option<&ReinforcementRankSignal>,
    reinforcement: &ReinforcementRankParams,
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
                    // An insight pulls its members' activation so the cluster
                    // warms together; weighted like Supports.
                    MemoryRelationType::Summarizes => 0.28,
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
    let graph_boost = candidate.graph_boost.min(GRAPH_BOOST_CAP);
    let importance_boost = candidate.importance as f64 * 0.35;
    let confidence_boost = candidate.confidence as f64 * 1.8;
    let recency_score = recency_boost * 0.6;
    let activation = reinforcement_signal.map_or(0.0, |signal| signal.activation);
    let activation_boost = if reinforcement.active() {
        mem_reinforce::activation_rank_boost(activation, reinforcement.weight, reinforcement.cap)
    } else {
        0.0
    };
    let needs_review = reinforcement_signal.is_some_and(|signal| signal.needs_review);

    let mut final_score = chunk_score
        + entry_score
        + exact_phrase_boost
        + overlap_boost
        + tag_boost
        + path_boost
        + semantic_boost
        + graph_boost
        + importance_boost
        + confidence_boost
        + recency_score
        + relation_boost
        + activation_boost;

    if exact_phrase_matches == 0
        && term_overlap < 0.15
        && candidate.chunk_fts == 0.0
        && candidate.entry_fts == 0.0
        && candidate.semantic_similarity < 0.25
        && graph_boost == 0.0
    {
        final_score *= 0.65;
    }
    if needs_review && reinforcement.active() {
        final_score *= reinforcement.needs_review_penalty.clamp(0.0, 1.0);
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
    if graph_boost > 0.0 {
        score_explanation.push(format!(
            "graph match x{} boost {:.2}",
            candidate.graph_match_count, graph_boost
        ));
    }
    if let Some(signal) = provenance {
        if let Some(status) = &signal.decay_status {
            let multiplier = provenance_decay_multiplier(status, provenance_config);
            if include_stale {
                score_explanation.push(format!("provenance stale bypassed ({})", status.as_str()));
            } else {
                final_score *= multiplier;
                score_explanation.push(format!(
                    "provenance decay x{multiplier:.2} ({})",
                    status.as_str()
                ));
            }
        }
        if signal.unverified_count > 0 {
            score_explanation.push(format!(
                "provenance unverified x{}",
                signal.unverified_count
            ));
        }
    }
    if activation_boost > 0.0 {
        score_explanation.push(format!(
            "activation {:.2} boost {:.2}",
            activation, activation_boost
        ));
    }
    if needs_review && reinforcement.active() {
        score_explanation.push(format!(
            "needs review x{:.2}",
            reinforcement.needs_review_penalty.clamp(0.0, 1.0)
        ));
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
        project: candidate.project,
        project_name: candidate.project_name,
        repo_root: candidate.repo_root,
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
            graph_boost,
            graph_match_count: candidate.graph_match_count,
            graph_edge_count: candidate.graph_edge_count,
            importance: candidate.importance,
            memory_confidence: candidate.confidence,
            recency_boost,
        },
        score_explanation,
        graph_connections: candidate.graph_connections,
        needs_review,
    }
}

/// Total ordering for ranked results: score desc, then recency desc, then
/// memory id as a deterministic final tie-break. `total_cmp` keeps the order
/// total even if a score ever degenerates to NaN.
fn compare_ranked(left: &RankedCandidate, right: &RankedCandidate) -> std::cmp::Ordering {
    right
        .final_score
        .total_cmp(&left.final_score)
        .then_with(|| right.updated_at.cmp(&left.updated_at))
        .then_with(|| left.memory_id.cmp(&right.memory_id))
}

/// Upper bound for the graph retrieval channel (default 8s). Override with
/// `MEMORY_LAYER_GRAPH_TIMEOUT_SECS`; values must be a positive integer
/// number of seconds.
fn graph_channel_timeout() -> std::time::Duration {
    let secs = std::env::var("MEMORY_LAYER_GRAPH_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(8);
    std::time::Duration::from_secs(secs)
}

fn provenance_decay_multiplier(status: &SourceProvenanceStatus, config: &ProvenanceConfig) -> f64 {
    match status {
        SourceProvenanceStatus::MissingFile => config.missing_file_decay,
        SourceProvenanceStatus::MissingSymbol => config.missing_symbol_decay,
        SourceProvenanceStatus::Stale => config.stale_decay,
        SourceProvenanceStatus::Verified | SourceProvenanceStatus::Unverifiable => 1.0,
    }
    .clamp(0.0, 1.0)
}

#[derive(Debug, Clone)]
struct QueryAnswerSynthesis {
    answer: String,
    confidence: f32,
    insufficient_evidence: bool,
    answer_generation: QueryAnswerGeneration,
    answer_citations: Vec<QueryAnswerCitation>,
}

/// Content-bearing tokens of a summary, for topic comparison between results.
fn topic_tokens(text: &str) -> HashSet<String> {
    const STOPWORDS: &[&str] = &[
        "the", "a", "an", "is", "are", "was", "were", "be", "for", "of", "on", "in", "to", "and",
        "or", "as", "at", "by", "with", "since", "after", "before", "it", "its", "that", "this",
        "not", "no",
    ];
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| token.len() >= 2 && !STOPWORDS.contains(token))
        .map(str::to_string)
        .collect()
}

/// How much of the smaller token set is contained in the other. Near 1.0 means
/// the two summaries state the same fact (possibly with different values).
fn topic_containment(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    let smaller = a.len().min(b.len());
    if smaller == 0 {
        return 0.0;
    }
    a.intersection(b).count() as f64 / smaller as f64
}

/// Memories below this confidence are treated as too weak to state as fact:
/// they neither justify an answer on their own nor earn a citation as
/// supporting context. The canary fixture's vague/superseded memories sit at
/// 0.3-0.5; real curated facts land at 0.8+.
const ANSWER_CONFIDENCE_FLOOR: f32 = 0.55;

/// A runner-up whose summary restates the top result's topic is either a
/// duplicate or a contradiction (same fact, different value — the stale-echo
/// failure); either way it must not be echoed as supporting context.
const RUNNER_UP_TOPIC_OVERLAP_LIMIT: f64 = 0.55;

fn synthesize_answer(results: &[QueryResult]) -> QueryAnswerSynthesis {
    let Some(top) = results.first() else {
        return QueryAnswerSynthesis {
            answer: "I could not find enough project memory to answer confidently.".to_string(),
            confidence: 0.0,
            insufficient_evidence: true,
            answer_generation: QueryAnswerGeneration {
                method: QueryAnswerMethod::Deterministic,
                ..QueryAnswerGeneration::default()
            },
            answer_citations: Vec::new(),
        };
    };

    let best_score = top.score;
    let normalized = (best_score / (best_score + 6.0)).clamp(0.0, 1.0) as f32;
    let strong_results = results
        .iter()
        .enumerate()
        .take(3)
        .filter(|(_, result)| result.score >= best_score * 0.72)
        .collect::<Vec<_>>();

    // Weak-match refusal: nothing anchors the top result to the question —
    // low term overlap AND low semantic similarity, with no exact-phrase
    // match. A high aggregate score alone (from confidence/importance boosts)
    // must not turn an off-topic memory into a confident answer. Tag matches
    // are deliberately not an anchor: they use substring matching and misfire
    // on short question terms. Thresholds tuned against memory-quality-v1:
    // legitimate answers sit at overlap >= 0.55 or similarity >= 0.60;
    // off-topic tops sit below both.
    let weakly_matched = top.debug.term_overlap < 0.55
        && top.debug.semantic_similarity < 0.60
        && top.debug.exact_phrase_matches == 0;

    let insufficient = strong_results.is_empty()
        || normalized < 0.38
        || weakly_matched
        || top.debug.memory_confidence < ANSWER_CONFIDENCE_FLOOR
        || strong_results[0]
            .1
            .score_explanation
            .iter()
            .all(|item| item.starts_with("term overlap 0%"));

    if insufficient {
        return QueryAnswerSynthesis {
            answer: "I could not find enough project memory to answer confidently.".to_string(),
            confidence: normalized.min(0.3),
            insufficient_evidence: true,
            answer_generation: QueryAnswerGeneration {
                method: QueryAnswerMethod::Deterministic,
                ..QueryAnswerGeneration::default()
            },
            answer_citations: Vec::new(),
        };
    }

    let top_topic = topic_tokens(&top.summary);
    let mut summaries = Vec::new();
    let mut citations = Vec::new();
    let mut seen = HashSet::new();
    for (index, result) in strong_results {
        if index > 0 {
            // Runner-ups must earn their place: skip low-confidence memories
            // (never cite weak evidence as support) and same-topic restatements
            // of the top result — a superseded sibling stating a different
            // value would otherwise be echoed right next to the fresh fact.
            if result.debug.memory_confidence < ANSWER_CONFIDENCE_FLOOR {
                continue;
            }
            let overlap = topic_containment(&top_topic, &topic_tokens(&result.summary));
            if overlap >= RUNNER_UP_TOPIC_OVERLAP_LIMIT {
                continue;
            }
        }
        let normalized_summary = result.summary.to_lowercase();
        if seen.insert(normalized_summary) {
            summaries.push(result.summary.clone());
            citations.push(QueryAnswerCitation {
                result_number: index + 1,
                memory_id: result.memory_id,
                project: result.project.clone(),
                project_name: result.project_name.clone(),
                repo_root: result.repo_root.clone(),
                memory_type: result.memory_type.clone(),
                summary: result.summary.clone(),
                snippet: result.snippet.clone(),
            });
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
    QueryAnswerSynthesis {
        answer,
        confidence,
        insufficient_evidence: false,
        answer_generation: QueryAnswerGeneration {
            method: QueryAnswerMethod::Deterministic,
            cited_result_numbers: citations
                .iter()
                .map(|citation| citation.result_number)
                .collect(),
            evidence_count: citations.len(),
            duration_ms: 0,
            fallback_reason: None,
            token_usage: None,
        },
        answer_citations: citations,
    }
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
        let start = floor_char_boundary(&trimmed, index.saturating_sub(80));
        let end = floor_char_boundary(&trimmed, (start + 240).min(trimmed.len()));
        let prefix = if start > 0 { "..." } else { "" };
        let suffix = if end < trimmed.len() { "..." } else { "" };
        return format!("{prefix}{}{suffix}", &trimmed[start..end]);
    }

    let end = floor_char_boundary(&trimmed, 240);
    format!("{}...", &trimmed[..end])
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
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

#[doc(hidden)]
pub mod test_support {
    use std::sync::Arc;

    use anyhow::Result;
    use pgvector::Vector;

    use super::{
        EmbeddingRegistry, EmbeddingRegistryEntry, EmbeddingService,
        embedding_backend::{EmbeddingBackend, EmbeddingSpace},
    };
    use crate::EmbeddingPurpose;

    #[derive(Clone)]
    struct StaticEmbeddingBackend {
        space: EmbeddingSpace,
        value: f32,
    }

    #[async_trait::async_trait]
    impl EmbeddingBackend for StaticEmbeddingBackend {
        fn space(&self) -> &EmbeddingSpace {
            &self.space
        }

        async fn embed(&self, input: &[String], _purpose: EmbeddingPurpose) -> Result<Vec<Vector>> {
            Ok(input
                .iter()
                .enumerate()
                .map(|(index, _)| Vector::from(vec![self.value, index as f32]))
                .collect())
        }
    }

    fn static_embedding_service(name: &str, value: f32) -> EmbeddingService {
        EmbeddingService {
            backend: Arc::new(StaticEmbeddingBackend {
                space: EmbeddingSpace::new("test", "http://127.0.0.1", name),
                value,
            }),
            batch_size: 16,
        }
    }

    pub fn static_embedding_registry(backends: &[(&str, f32)]) -> EmbeddingRegistry {
        EmbeddingRegistry {
            entries: backends
                .iter()
                .map(|(name, value)| EmbeddingRegistryEntry {
                    name: (*name).to_string(),
                    service: static_embedding_service(name, *value),
                    create_enabled: true,
                })
                .collect(),
            active: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_memory_type_accepts_newer_types() {
        assert_eq!(parse_memory_type("task"), MemoryType::Task);
        assert_eq!(
            parse_memory_type("documentation"),
            MemoryType::Documentation
        );
        assert_eq!(parse_memory_type("refactor"), MemoryType::Refactor);
    }

    #[test]
    fn every_memory_type_round_trips_display_and_parse() {
        // Parity guard: every canonical type's Display string must parse back
        // to itself, so `parse_memory_type` cannot silently coerce a real type
        // to the `Convention` fallback (this is how the docs' nonexistent
        // `fact` type was masking `domain_fact`). `MemoryType::ALL` is
        // exhaustiveness-checked, so a new variant forces this test to cover it.
        for memory_type in MemoryType::ALL {
            let rendered = memory_type.to_string();
            assert_eq!(
                parse_memory_type(&rendered),
                memory_type,
                "`{rendered}` did not round-trip through parse_memory_type"
            );
        }
        assert_eq!(MemoryType::ALL.len(), 17);
    }

    #[test]
    fn embedding_registry_is_empty_when_no_backends_ready() {
        // An EmbeddingBackendConfig without a model never resolves to a
        // concrete backend; the registry should be empty.
        let mut cfg = EmbeddingsConfig {
            enabled: true,
            create_enabled: true,
            active: None,
            backends: vec![EmbeddingBackendConfig {
                name: "openai".to_string(),
                provider: "openai_compatible".to_string(),
                base_url: String::new(),
                api_key_env: "MISSING_ENV_VAR_FOR_TEST".to_string(),
                model: String::new(),
                batch_size: 16,
                dimensions: None,
                create_enabled: true,
            }],
        };
        cfg.normalize_backend_names();
        let registry = EmbeddingRegistry::from_config(&cfg);
        assert!(registry.is_empty());
        assert!(registry.active().is_none());
        assert!(registry.active_name().is_none());
    }

    #[test]
    fn embedding_registry_clear_active_disables_search() {
        let service = EmbeddingService::from_backend_config(&EmbeddingBackendConfig {
            name: "openai".to_string(),
            provider: "openai_compatible".to_string(),
            base_url: String::new(),
            api_key_env: "OPENAI_API_KEY".to_string(),
            model: "text-embedding-3-small".to_string(),
            batch_size: 16,
            dimensions: None,
            create_enabled: true,
        });
        if service.is_none() {
            return;
        }
        let mut registry = EmbeddingRegistry {
            entries: vec![EmbeddingRegistryEntry {
                name: "openai".to_string(),
                service: service.unwrap(),
                create_enabled: true,
            }],
            active: Some("openai".to_string()),
        };

        registry.clear_active();

        assert!(registry.active().is_none());
        assert!(registry.active_name().is_none());
        assert!(registry.get("openai").is_some());
    }

    #[test]
    fn embedding_registry_set_active_rejects_unknown() {
        let mut registry = EmbeddingRegistry::default();
        let err = registry
            .set_active("does-not-exist")
            .expect_err("missing backend must error");
        match err {
            ActivateError::UnknownBackend(name) => assert_eq!(name, "does-not-exist"),
        }
        assert!(registry.active_name().is_none());
    }

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
    fn graph_terms_skip_broad_question_words() {
        let intent = QueryIntent::from_query("What are the different memory types?");

        assert!(graph_like_terms(&intent).is_empty());
    }

    #[test]
    fn graph_terms_keep_code_specific_words() {
        let intent = QueryIntent::from_query("How does MemoryType parsing work in mem-search?");
        let terms = graph_like_terms(&intent);

        assert!(terms.contains(&"%memorytype%".to_string()));
        assert!(terms.contains(&"%parsing%".to_string()));
        assert!(terms.contains(&"%mem-search%".to_string()));
        assert!(!terms.contains(&"%memory%".to_string()));
    }

    #[test]
    fn global_query_execution_has_no_project_scope() {
        let request = GlobalQueryRequest {
            query: "find memory docs".to_string(),
            filters: QueryFilters::default(),
            top_k: 8,
            min_confidence: None,
            include_stale: false,
            history: false,
            retrieval_mode: None,
            answer_mode: None,
        };

        let execution = QueryExecution::from_global_request(&request);

        assert_eq!(execution.project, None);
        assert_eq!(execution.query, "find memory docs");
    }

    #[test]
    fn provenance_warnings_include_stale_source_statuses() {
        let memory_id = Uuid::new_v4();
        let results = vec![QueryResult {
            memory_id,
            project: None,
            project_name: None,
            repo_root: None,
            summary: "Moved helper".to_string(),
            memory_type: MemoryType::Refactor,
            score: 2.0,
            snippet: "helper moved".to_string(),
            match_kind: QueryMatchKind::Lexical,
            score_explanation: vec![],
            debug: QueryResultDebug::default(),
            tags: vec![],
            sources: vec![QuerySource {
                task_id: None,
                file_path: Some("src/old.rs".to_string()),
                symbol_name: None,
                symbol_kind: None,
                source_kind: SourceKind::File,
                excerpt: None,
                provenance: Some(SourceProvenanceRecord {
                    status: SourceProvenanceStatus::MissingFile,
                    checked_at: Utc::now(),
                    reason: Some("file source no longer exists".to_string()),
                    resolved_path: Some("/repo/src/old.rs".to_string()),
                }),
            }],
            graph_connections: vec![],
            needs_review: false,
        }];

        let warnings = provenance_warnings_for_results(&results);

        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code, "stale_memory_provenance");
        assert_eq!(warnings[0].severity, DiagnosticSeverity::Warning);
        assert!(warnings[0].message.contains(&memory_id.to_string()));
        assert!(warnings[0].message.contains("src/old.rs"));
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
    fn snippet_truncates_on_utf8_char_boundary() {
        let text = format!("{}│ detail {}", "─".repeat(80), "candidate ".repeat(40));
        let snippet = summarize_snippet(&text, &[], &[]);

        assert!(snippet.ends_with("..."));
        assert!(snippet.is_char_boundary(snippet.len()));
    }

    #[test]
    fn snippet_matching_window_uses_utf8_char_boundaries() {
        let text = format!("{} needle {}", "│".repeat(90), "candidate ".repeat(40));
        let snippet = summarize_snippet(&text, &["needle".to_string()], &[]);

        assert!(snippet.contains("needle"));
        assert!(snippet.is_char_boundary(snippet.len()));
    }

    #[test]
    fn vector_wrapper_preserves_embedding_length() {
        let vector = Vector::from(vec![1.0, 2.0, 3.0]);
        assert_eq!(vector, Vector::from(vec![1.0, 2.0, 3.0]));
    }

    fn synthesis_result(summary: &str, score: f64, memory_confidence: f32) -> QueryResult {
        QueryResult {
            memory_id: Uuid::new_v4(),
            project: None,
            project_name: None,
            repo_root: None,
            summary: summary.to_string(),
            memory_type: MemoryType::Architecture,
            score,
            snippet: summary.to_string(),
            match_kind: QueryMatchKind::Lexical,
            score_explanation: vec![
                "strong chunk match 1.20".to_string(),
                "term overlap 80%".to_string(),
            ],
            debug: QueryResultDebug {
                term_overlap: 0.8,
                semantic_similarity: 0.7,
                memory_confidence,
                ..QueryResultDebug::default()
            },
            tags: vec![],
            sources: vec![],
            graph_connections: vec![],
            needs_review: false,
        }
    }

    #[test]
    fn synthesize_answer_prefers_multiple_strong_results() {
        let results = vec![
            synthesis_result("The gateway terminates client sessions.", 7.0, 0.9),
            synthesis_result("Deploys run through the release pipeline.", 5.5, 0.9),
        ];

        let synthesis = synthesize_answer(&results);
        assert!(synthesis.answer.contains("gateway terminates"));
        assert!(synthesis.answer.contains("release pipeline"));
        assert!(synthesis.confidence > 0.45);
        assert!(!synthesis.insufficient_evidence);
        assert_eq!(
            synthesis.answer_generation.method,
            QueryAnswerMethod::Deterministic
        );
        assert_eq!(synthesis.answer_generation.cited_result_numbers, vec![1, 2]);
        assert_eq!(synthesis.answer_citations.len(), 2);
    }

    #[test]
    fn synthesize_answer_refuses_on_low_confidence_evidence() {
        // A vague, low-confidence memory can top the ranking on term overlap
        // alone; it must not be stated as a confident answer.
        let results = vec![synthesis_result(
            "Someone suggested moving the cache layer; nothing was decided.",
            7.0,
            0.3,
        )];

        let synthesis = synthesize_answer(&results);
        assert!(synthesis.insufficient_evidence);
        assert!(synthesis.answer_citations.is_empty());
    }

    #[test]
    fn synthesize_answer_refuses_on_weak_match_evidence() {
        // High aggregate score from confidence/importance boosts, but nothing
        // ties the memory to the question: low overlap, low similarity, no
        // phrase or tag anchor.
        let mut result = synthesis_result("The bridge gateway listens on port 7420.", 8.0, 0.95);
        result.debug.term_overlap = 0.28;
        result.debug.semantic_similarity = 0.4;
        result.score_explanation = vec!["term overlap 28%".to_string()];

        let synthesis = synthesize_answer(&[result]);
        assert!(synthesis.insufficient_evidence);
    }

    #[test]
    fn synthesize_answer_skips_same_topic_runner_up() {
        // The stale sibling states the same fact with a different value; it
        // must be neither echoed in the answer nor cited.
        let results = vec![
            synthesis_result(
                "The bridge gateway listens on port 7420 since the network rework.",
                8.0,
                0.95,
            ),
            synthesis_result("The bridge gateway listens on port 7100.", 6.5, 0.5),
        ];

        let synthesis = synthesize_answer(&results);
        assert!(!synthesis.insufficient_evidence);
        assert!(synthesis.answer.contains("7420"));
        assert!(!synthesis.answer.contains("7100"));
        assert_eq!(synthesis.answer_citations.len(), 1);
    }

    #[test]
    fn synthesize_answer_keeps_complementary_runner_up() {
        let results = vec![
            synthesis_result(
                "Ingest validates each record's schema before enqueue.",
                8.0,
                0.9,
            ),
            synthesis_result(
                "Failed syncs show a slate banner with a retry action.",
                6.5,
                0.9,
            ),
        ];

        let synthesis = synthesize_answer(&results);
        assert!(!synthesis.insufficient_evidence);
        assert_eq!(synthesis.answer_citations.len(), 2);
    }

    #[test]
    fn rank_candidate_includes_graph_boost_and_explanation() {
        let memory_id = Uuid::new_v4();
        let candidate = CandidateRecord {
            memory_id,
            project: None,
            project_name: None,
            repo_root: None,
            summary: "Unrelated summary".to_string(),
            memory_type: MemoryType::Implementation,
            canonical_text: "Durable implementation detail.".to_string(),
            importance: 2,
            confidence: 0.8,
            updated_at: Utc::now(),
            entry_fts: 0.0,
            chunk_fts: 0.0,
            semantic_similarity: 0.0,
            best_chunk_text: "Durable implementation detail.".to_string(),
            tags: Vec::new(),
            source_paths: vec!["src/lib.rs".to_string()],
            graph_boost: 9.0,
            graph_match_count: 3,
            graph_edge_count: 1,
            graph_connections: vec![QueryGraphConnection {
                file_path: "src/lib.rs".to_string(),
                symbol: Some("GraphTarget".to_string()),
                symbol_kind: Some("function".to_string()),
                edge_kind: Some("calls".to_string()),
                neighbor_symbol: Some("caller".to_string()),
                direction: Some("incoming".to_string()),
                score_boost: GRAPH_DIRECT_BOOST,
                reason: "code symbol match".to_string(),
            }],
        };

        let ranked = rank_candidate(
            candidate,
            &QueryIntent::from_query("GraphTarget"),
            &HashMap::new(),
            None,
            &ProvenanceConfig::default(),
            false,
            None,
            &ReinforcementRankParams::default(),
        );

        assert_eq!(ranked.memory_id, memory_id);
        assert_eq!(ranked.debug.graph_boost, GRAPH_BOOST_CAP);
        assert_eq!(ranked.debug.graph_match_count, 3);
        assert_eq!(ranked.debug.graph_edge_count, 1);
        assert!(
            ranked
                .score_explanation
                .iter()
                .any(|item| item == "graph match x3 boost 2.50")
        );
        assert_eq!(ranked.graph_connections.len(), 1);
    }

    #[test]
    fn rank_candidate_applies_activation_boost_and_needs_review_penalty() {
        let make_candidate = || CandidateRecord {
            memory_id: Uuid::new_v4(),
            project: None,
            project_name: None,
            repo_root: None,
            summary: "Activation ranking sample".to_string(),
            memory_type: MemoryType::Implementation,
            canonical_text: "Activation ranking sample detail.".to_string(),
            importance: 2,
            confidence: 0.8,
            updated_at: Utc::now(),
            entry_fts: 0.4,
            chunk_fts: 0.4,
            semantic_similarity: 0.0,
            best_chunk_text: "Activation ranking sample detail.".to_string(),
            tags: Vec::new(),
            source_paths: Vec::new(),
            graph_boost: 0.0,
            graph_match_count: 0,
            graph_edge_count: 0,
            graph_connections: Vec::new(),
        };
        let intent = QueryIntent::from_query("activation ranking sample");
        let params = ReinforcementRankParams::default();

        let baseline = rank_candidate(
            make_candidate(),
            &intent,
            &HashMap::new(),
            None,
            &ProvenanceConfig::default(),
            false,
            None,
            &params,
        );
        let boosted = rank_candidate(
            make_candidate(),
            &intent,
            &HashMap::new(),
            None,
            &ProvenanceConfig::default(),
            false,
            Some(&ReinforcementRankSignal {
                activation: 6.0,
                needs_review: false,
            }),
            &params,
        );
        assert!(boosted.final_score > baseline.final_score);
        assert!(!boosted.needs_review);
        assert!(
            boosted
                .score_explanation
                .iter()
                .any(|item| item.starts_with("activation 6.00"))
        );

        let flagged = rank_candidate(
            make_candidate(),
            &intent,
            &HashMap::new(),
            None,
            &ProvenanceConfig::default(),
            false,
            Some(&ReinforcementRankSignal {
                activation: 0.0,
                needs_review: true,
            }),
            &params,
        );
        assert!(flagged.needs_review);
        assert!(
            (flagged.final_score - baseline.final_score * params.needs_review_penalty).abs() < 1e-9,
            "penalty must scale the baseline score"
        );

        let disabled = ReinforcementRankParams {
            weight: 0.0,
            ..params.clone()
        };
        let unboosted = rank_candidate(
            make_candidate(),
            &intent,
            &HashMap::new(),
            None,
            &ProvenanceConfig::default(),
            false,
            Some(&ReinforcementRankSignal {
                activation: 6.0,
                needs_review: true,
            }),
            &disabled,
        );
        assert!(
            (unboosted.final_score - baseline.final_score).abs() < 1e-9,
            "weight 0 must leave ranking byte-identical"
        );
    }

    mod ranker_properties {
        use super::super::*;
        use chrono::Duration;
        use proptest::prelude::*;

        #[derive(Debug, Clone)]
        struct CandidateInputs {
            chunk_fts: f64,
            entry_fts: f64,
            semantic_similarity: f64,
            graph_boost: f64,
            importance: i32,
            confidence: f32,
            age_days: i64,
            activation: f64,
        }

        fn candidate_inputs() -> impl Strategy<Value = CandidateInputs> {
            (
                0.0..10.0f64,
                0.0..10.0f64,
                -1.0..1.0f64,
                0.0..20.0f64,
                0..6i32,
                0.0..1.0f32,
                0..2000i64,
                0.0..25.0f64,
            )
                .prop_map(
                    |(
                        chunk_fts,
                        entry_fts,
                        semantic_similarity,
                        graph_boost,
                        importance,
                        confidence,
                        age_days,
                        activation,
                    )| CandidateInputs {
                        chunk_fts,
                        entry_fts,
                        semantic_similarity,
                        graph_boost,
                        importance,
                        confidence,
                        age_days,
                        activation,
                    },
                )
        }

        fn build_candidate(inputs: &CandidateInputs, memory_id: Uuid) -> CandidateRecord {
            CandidateRecord {
                memory_id,
                project: None,
                project_name: None,
                repo_root: None,
                summary: "Ranker property sample".to_string(),
                memory_type: MemoryType::Implementation,
                canonical_text: "Ranker property sample detail.".to_string(),
                importance: inputs.importance,
                confidence: inputs.confidence,
                updated_at: Utc::now() - Duration::days(inputs.age_days),
                entry_fts: inputs.entry_fts,
                chunk_fts: inputs.chunk_fts,
                semantic_similarity: inputs.semantic_similarity,
                best_chunk_text: "Ranker property sample detail.".to_string(),
                tags: Vec::new(),
                source_paths: Vec::new(),
                graph_boost: inputs.graph_boost,
                graph_match_count: 0,
                graph_edge_count: 0,
                graph_connections: Vec::new(),
            }
        }

        fn rank(
            inputs: &CandidateInputs,
            memory_id: Uuid,
            needs_review: bool,
            decay_status: Option<SourceProvenanceStatus>,
        ) -> RankedCandidate {
            let signal = ReinforcementRankSignal {
                activation: inputs.activation,
                needs_review,
            };
            let provenance = decay_status.map(|status| ProvenanceRankSignal {
                decay_status: Some(status),
                unverified_count: 0,
            });
            rank_candidate(
                build_candidate(inputs, memory_id),
                &QueryIntent::from_query("ranker property sample"),
                &HashMap::new(),
                provenance.as_ref(),
                &ProvenanceConfig::default(),
                false,
                Some(&signal),
                &ReinforcementRankParams::default(),
            )
        }

        proptest! {
            #[test]
            fn final_score_is_finite(inputs in candidate_inputs(), needs_review: bool) {
                let ranked = rank(&inputs, Uuid::new_v4(), needs_review, None);
                prop_assert!(ranked.final_score.is_finite());
                prop_assert!(ranked.final_score >= 0.0);
            }

            #[test]
            fn needs_review_penalty_never_raises_score(inputs in candidate_inputs()) {
                let id = Uuid::new_v4();
                let clean = rank(&inputs, id, false, None);
                let flagged = rank(&inputs, id, true, None);
                prop_assert!(flagged.final_score <= clean.final_score + 1e-9);
            }

            #[test]
            fn provenance_decay_never_raises_score(inputs in candidate_inputs()) {
                let id = Uuid::new_v4();
                let verified = rank(&inputs, id, false, Some(SourceProvenanceStatus::Verified));
                for status in [
                    SourceProvenanceStatus::MissingFile,
                    SourceProvenanceStatus::MissingSymbol,
                    SourceProvenanceStatus::Stale,
                ] {
                    let decayed = rank(&inputs, id, false, Some(status));
                    prop_assert!(decayed.final_score <= verified.final_score + 1e-9);
                }
            }

            #[test]
            fn positive_signals_are_monotone(inputs in candidate_inputs(), bump in 0.01..5.0f64) {
                let id = Uuid::new_v4();
                let base = rank(&inputs, id, false, None);
                let raised = [
                    CandidateInputs { chunk_fts: inputs.chunk_fts + bump, ..inputs.clone() },
                    CandidateInputs { entry_fts: inputs.entry_fts + bump, ..inputs.clone() },
                    CandidateInputs {
                        semantic_similarity: (inputs.semantic_similarity + bump).min(1.0),
                        ..inputs.clone()
                    },
                    CandidateInputs { graph_boost: inputs.graph_boost + bump, ..inputs.clone() },
                    CandidateInputs { importance: inputs.importance + 1, ..inputs.clone() },
                    CandidateInputs {
                        confidence: (inputs.confidence + bump as f32).min(1.0),
                        ..inputs.clone()
                    },
                    CandidateInputs { activation: inputs.activation + bump, ..inputs.clone() },
                ];
                for stronger in raised {
                    let ranked = rank(&stronger, id, false, None);
                    prop_assert!(
                        ranked.final_score >= base.final_score - 1e-9,
                        "raising a positive signal lowered the score: {stronger:?}"
                    );
                }
            }

            #[test]
            fn ranked_ordering_is_total(all_inputs in proptest::collection::vec(candidate_inputs(), 2..12)) {
                let mut ranked = all_inputs
                    .iter()
                    .map(|inputs| rank(inputs, Uuid::new_v4(), false, None))
                    .collect::<Vec<_>>();
                for left in &ranked {
                    prop_assert_eq!(compare_ranked(left, left), std::cmp::Ordering::Equal);
                    for right in &ranked {
                        prop_assert_eq!(
                            compare_ranked(left, right),
                            compare_ranked(right, left).reverse()
                        );
                    }
                }
                ranked.sort_by(compare_ranked);
                for pair in ranked.windows(2) {
                    prop_assert_ne!(
                        compare_ranked(&pair[0], &pair[1]),
                        std::cmp::Ordering::Greater
                    );
                }
            }
        }
    }
}
