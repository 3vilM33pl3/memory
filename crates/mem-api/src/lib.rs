use std::{
    collections::HashMap,
    env, fmt,
    io::Cursor,
    path::{Path, PathBuf},
    time::Duration,
};

use capnp::{message::ReaderOptions, serialize};
use chrono::{DateTime, Utc};
use config::{Config, ConfigError, Environment, File, FileFormat};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemoryType {
    Architecture,
    Convention,
    Decision,
    Incident,
    Debugging,
    Environment,
    DomainFact,
}

impl fmt::Display for MemoryType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Architecture => "architecture",
            Self::Convention => "convention",
            Self::Decision => "decision",
            Self::Incident => "incident",
            Self::Debugging => "debugging",
            Self::Environment => "environment",
            Self::DomainFact => "domain_fact",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemoryRelationType {
    Duplicates,
    Supersedes,
    Supports,
    RelatedTo,
    DependsOn,
}

impl fmt::Display for MemoryRelationType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Duplicates => "duplicates",
            Self::Supersedes => "supersedes",
            Self::Supports => "supports",
            Self::RelatedTo => "related_to",
            Self::DependsOn => "depends_on",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryStatus {
    Active,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    TaskPrompt,
    File,
    GitCommit,
    CommandOutput,
    Test,
    Note,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    pub command: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureCandidateSourceInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    pub source_kind: SourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureCandidateInput {
    pub canonical_text: String,
    pub summary: String,
    pub memory_type: MemoryType,
    #[serde(default = "default_candidate_confidence")]
    pub confidence: f32,
    #[serde(default = "default_candidate_importance")]
    pub importance: i32,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub sources: Vec<CaptureCandidateSourceInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureTaskRequest {
    pub project: String,
    pub task_title: String,
    pub user_prompt: String,
    pub agent_summary: String,
    #[serde(default)]
    pub files_changed: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_diff_summary: Option<String>,
    #[serde(default)]
    pub tests: Vec<TestResult>,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub structured_candidates: Vec<CaptureCandidateInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

impl CaptureTaskRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.project.trim().is_empty() {
            return Err(ValidationError::new("project must be non-empty"));
        }
        if self.task_title.trim().is_empty() {
            return Err(ValidationError::new("task_title must be non-empty"));
        }
        if self.user_prompt.trim().is_empty() {
            return Err(ValidationError::new("user_prompt must be non-empty"));
        }
        if self.agent_summary.trim().is_empty() {
            return Err(ValidationError::new("agent_summary must be non-empty"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurateRequest {
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_size: Option<i64>,
}

impl CurateRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.project.trim().is_empty() {
            return Err(ValidationError::new("project must be non-empty"));
        }
        if let Some(batch_size) = self.batch_size {
            if batch_size <= 0 {
                return Err(ValidationError::new("batch_size must be positive"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QueryFilters {
    #[serde(default)]
    pub types: Vec<MemoryType>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryRequest {
    pub project: String,
    pub query: String,
    #[serde(default)]
    pub filters: QueryFilters,
    #[serde(default = "default_top_k")]
    pub top_k: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_confidence: Option<f32>,
}

impl QueryRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.project.trim().is_empty() {
            return Err(ValidationError::new("project must be non-empty"));
        }
        if self.query.trim().is_empty() {
            return Err(ValidationError::new("query must be non-empty"));
        }
        if !(1..=50).contains(&self.top_k) {
            return Err(ValidationError::new("top_k must be in 1..=50"));
        }
        if let Some(value) = self.min_confidence {
            if !(0.0..=1.0).contains(&value) {
                return Err(ValidationError::new("min_confidence must be in 0.0..=1.0"));
            }
        }
        Ok(())
    }
}

fn default_top_k() -> i64 {
    8
}

fn default_candidate_confidence() -> f32 {
    0.75
}

fn default_candidate_importance() -> i32 {
    2
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuerySource {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    pub source_kind: SourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub memory_id: Uuid,
    pub summary: String,
    pub memory_type: MemoryType,
    pub score: f64,
    pub snippet: String,
    #[serde(default)]
    pub match_kind: QueryMatchKind,
    #[serde(default)]
    pub score_explanation: Vec<String>,
    #[serde(default)]
    pub debug: QueryResultDebug,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub sources: Vec<QuerySource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueryMatchKind {
    Lexical,
    Semantic,
    Hybrid,
}

impl Default for QueryMatchKind {
    fn default() -> Self {
        Self::Lexical
    }
}

impl fmt::Display for QueryMatchKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Lexical => "lexical",
            Self::Semantic => "semantic",
            Self::Hybrid => "hybrid",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QueryResultDebug {
    #[serde(default)]
    pub chunk_fts: f64,
    #[serde(default)]
    pub entry_fts: f64,
    #[serde(default)]
    pub semantic_similarity: f64,
    #[serde(default)]
    pub exact_phrase_matches: usize,
    #[serde(default)]
    pub term_overlap: f64,
    #[serde(default)]
    pub tag_match_count: usize,
    #[serde(default)]
    pub path_match_count: usize,
    #[serde(default)]
    pub relation_boost: f64,
    #[serde(default)]
    pub importance: i32,
    #[serde(default)]
    pub memory_confidence: f32,
    #[serde(default)]
    pub recency_boost: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QueryDiagnostics {
    #[serde(default)]
    pub lexical_candidates: usize,
    #[serde(default)]
    pub semantic_candidates: usize,
    #[serde(default)]
    pub merged_candidates: usize,
    #[serde(default)]
    pub returned_results: usize,
    #[serde(default)]
    pub relation_augmented_candidates: usize,
    #[serde(default)]
    pub lexical_duration_ms: u64,
    #[serde(default)]
    pub semantic_duration_ms: u64,
    #[serde(default)]
    pub rerank_duration_ms: u64,
    #[serde(default)]
    pub total_duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedMemorySummary {
    pub memory_id: Uuid,
    pub relation_type: MemoryRelationType,
    pub summary: String,
    pub memory_type: MemoryType,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResponse {
    pub answer: String,
    pub confidence: f32,
    pub results: Vec<QueryResult>,
    pub insufficient_evidence: bool,
    #[serde(default)]
    pub diagnostics: QueryDiagnostics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureTaskResponse {
    pub project_id: Uuid,
    pub session_id: Uuid,
    pub task_id: Uuid,
    pub raw_capture_id: Uuid,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurateResponse {
    pub project_id: Uuid,
    pub run_id: Uuid,
    pub input_count: i64,
    pub output_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySourceRecord {
    pub id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_commit: Option<String>,
    pub source_kind: SourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntryResponse {
    pub id: Uuid,
    pub project: String,
    pub canonical_text: String,
    pub summary: String,
    pub memory_type: MemoryType,
    pub importance: i32,
    pub confidence: f32,
    pub status: MemoryStatus,
    pub tags: Vec<String>,
    pub sources: Vec<MemorySourceRecord>,
    #[serde(default)]
    pub related_memories: Vec<RelatedMemorySummary>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsResponse {
    pub projects: i64,
    pub sessions: i64,
    pub tasks: i64,
    pub raw_captures: i64,
    pub memory_entries: i64,
    pub curation_runs: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamRequest {
    Health,
    ProjectOverview { project: String },
    ProjectMemories { project: String },
    MemoryDetail { memory_id: Uuid },
    SubscribeProject { project: String },
    SubscribeMemory { memory_id: Uuid },
    UnsubscribeMemory,
    Ping,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamResponse {
    Health {
        value: serde_json::Value,
    },
    ProjectOverview {
        value: ProjectOverviewResponse,
    },
    ProjectMemories {
        value: ProjectMemoriesResponse,
    },
    MemoryDetail {
        value: Option<MemoryEntryResponse>,
    },
    ProjectSnapshot {
        overview: ProjectOverviewResponse,
        memories: ProjectMemoriesResponse,
    },
    ProjectChanged {
        overview: ProjectOverviewResponse,
        memories: ProjectMemoriesResponse,
    },
    MemorySnapshot {
        detail: Option<MemoryEntryResponse>,
    },
    MemoryChanged {
        detail: Option<MemoryEntryResponse>,
    },
    Activity {
        event: ActivityEvent,
    },
    Ack {
        message: String,
    },
    Pong,
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    CaptureTask,
    Curate,
    Reindex,
    Archive,
    DeleteMemory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActivityDetails {
    CaptureTask {
        session_id: Uuid,
        task_id: Uuid,
        raw_capture_id: Uuid,
        idempotency_key: String,
    },
    Curate {
        run_id: Uuid,
        input_count: i64,
        output_count: i64,
    },
    Reindex {
        reindexed_entries: u64,
    },
    Archive {
        archived_count: u64,
        max_confidence: f32,
        max_importance: i32,
    },
    DeleteMemory {
        deleted: bool,
        summary: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEvent {
    pub recorded_at: DateTime<Utc>,
    pub project: String,
    pub kind: ActivityKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_id: Option<Uuid>,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<ActivityDetails>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveRequest {
    pub project: String,
    #[serde(default = "default_archive_threshold")]
    pub max_confidence: f32,
    #[serde(default = "default_archive_importance")]
    pub max_importance: i32,
}

impl ArchiveRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.project.trim().is_empty() {
            return Err(ValidationError::new("project must be non-empty"));
        }
        Ok(())
    }
}

fn default_archive_threshold() -> f32 {
    0.3
}

fn default_archive_importance() -> i32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveResponse {
    pub archived_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteMemoryRequest {
    pub memory_id: Uuid,
}

impl DeleteMemoryRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.memory_id.is_nil() {
            return Err(ValidationError::new("memory_id must be non-nil"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteMemoryResponse {
    pub memory_id: Uuid,
    pub project: String,
    pub summary: String,
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReindexRequest {
    pub project: String,
}

impl ReindexRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.project.trim().is_empty() {
            return Err(ValidationError::new("project must be non-empty"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReindexResponse {
    pub reindexed_entries: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemoryListItem {
    pub id: Uuid,
    pub summary: String,
    pub preview: String,
    pub memory_type: MemoryType,
    pub status: MemoryStatus,
    pub confidence: f32,
    pub importance: i32,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub tag_count: i64,
    pub source_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemoriesResponse {
    pub project: String,
    pub total: i64,
    pub items: Vec<ProjectMemoryListItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryTypeCount {
    pub memory_type: MemoryType,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceKindCount {
    pub source_kind: SourceKind,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedCount {
    pub name: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectOverviewResponse {
    pub project: String,
    pub service_status: String,
    pub database_status: String,
    pub memory_entries_total: i64,
    pub active_memories: i64,
    pub archived_memories: i64,
    pub raw_captures_total: i64,
    pub uncurated_raw_captures: i64,
    pub tasks_total: i64,
    pub sessions_total: i64,
    pub curation_runs_total: i64,
    pub recent_memories_7d: i64,
    pub recent_captures_7d: i64,
    pub high_confidence_memories: i64,
    pub medium_confidence_memories: i64,
    pub low_confidence_memories: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_memory_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_capture_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_curation_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oldest_uncurated_capture_age_hours: Option<i64>,
    #[serde(default)]
    pub memory_type_breakdown: Vec<MemoryTypeCount>,
    #[serde(default)]
    pub source_kind_breakdown: Vec<SourceKindCount>,
    #[serde(default)]
    pub top_tags: Vec<NamedCount>,
    #[serde(default)]
    pub top_files: Vec<NamedCount>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automation: Option<AutomationStatus>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutomationMode {
    #[default]
    Suggest,
    Auto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationStatus {
    pub enabled: bool,
    pub mode: AutomationMode,
    pub repo_root: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_activity_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_persisted_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dirty_file_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_note_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_decision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub service: ServiceConfig,
    pub database: DatabaseConfig,
    #[serde(default)]
    pub features: FeatureFlags,
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub embeddings: EmbeddingConfig,
    #[serde(default)]
    pub automation: AutomationConfig,
}

impl AppConfig {
    pub fn load_from_path(path: Option<PathBuf>) -> Result<Self, ConfigError> {
        let mut builder = Config::builder();
        let mut env_files = Vec::new();
        if let Some(path) = path {
            env_files.push(env_path_for_config(&path));
            builder = builder.add_source(File::from(path).required(false));
        } else {
            if let Some(path) = discover_global_config_path() {
                env_files.push(env_path_for_config(&path));
                builder = builder.add_source(File::from(path).required(false));
            } else {
                builder = builder.add_source(File::with_name("memory-layer").required(false));
            }
            if let Some(path) = discover_repo_config_path() {
                env_files.push(
                    path.parent()
                        .unwrap_or_else(|| Path::new("."))
                        .join("memory-layer.env"),
                );
                builder = builder.add_source(File::from(path).required(false));
            }
        }

        for env_file in env_files {
            if let Some(source) = env_file_source(&env_file)? {
                builder = builder.add_source(source);
            }
        }

        let config = builder
            .add_source(Environment::with_prefix("MEMORY_LAYER").separator("__"))
            .build()?;
        let mut value: serde_json::Value = config.try_deserialize()?;
        normalize_legacy_config_keys(&mut value);
        serde_json::from_value(value).map_err(|error| ConfigError::Foreign(Box::new(error)))
    }
}

fn normalize_legacy_config_keys(value: &mut serde_json::Value) {
    let Some(root) = value.as_object_mut() else {
        return;
    };
    let Some(automation) = root
        .get_mut("automation")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return;
    };

    if automation.contains_key("capture_idle_threshold") {
        automation.remove("idle_threshold");
    } else if let Some(legacy) = automation.remove("idle_threshold") {
        automation.insert("capture_idle_threshold".to_string(), legacy);
    }
}

fn env_file_source(
    path: &Path,
) -> Result<Option<config::File<config::FileSourceString, FileFormat>>, ConfigError> {
    let values = memory_layer_env_file_values(path)?;
    if values.is_empty() {
        return Ok(None);
    }

    let mut lines = values
        .into_iter()
        .map(|(key, value)| format!("{key} = {}", serde_json::to_string(&value).unwrap()))
        .collect::<Vec<_>>();
    lines.sort();
    Ok(Some(File::from_str(&lines.join("\n"), FileFormat::Toml)))
}

fn memory_layer_env_file_values(path: &Path) -> Result<HashMap<String, String>, ConfigError> {
    let mut values = HashMap::new();
    let Ok(content) = std::fs::read_to_string(path) else {
        return Ok(values);
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((name, value)) = trimmed.split_once('=') else {
            continue;
        };
        let key = name.trim();
        if !key.starts_with("MEMORY_LAYER__") {
            continue;
        }
        let config_key = key["MEMORY_LAYER__".len()..]
            .split("__")
            .map(|segment| segment.trim().to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join(".");
        values.insert(config_key, value.trim().to_string());
    }

    Ok(values)
}

fn env_path_for_config(path: &Path) -> PathBuf {
    path.parent()
        .unwrap_or_else(|| Path::new("."))
        .join("memory-layer.env")
}

pub fn discover_repo_config_path() -> Option<PathBuf> {
    let cwd = env::current_dir().ok()?;
    find_repo_config_path(&cwd)
}

pub fn discover_repo_env_path() -> Option<PathBuf> {
    let config_path = discover_repo_config_path()?;
    Some(env_path_for_config(&config_path))
}

pub fn discover_global_env_path() -> Option<PathBuf> {
    let config_path = discover_global_config_path()?;
    Some(env_path_for_config(&config_path))
}

pub fn discover_global_config_path() -> Option<PathBuf> {
    if let Ok(config_home) = env::var("XDG_CONFIG_HOME") {
        let candidate = PathBuf::from(config_home)
            .join("memory-layer")
            .join("memory-layer.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    if let Ok(home) = env::var("HOME") {
        let candidate = PathBuf::from(home)
            .join(".config")
            .join("memory-layer")
            .join("memory-layer.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    let system_candidate = PathBuf::from("/etc/memory-layer/memory-layer.toml");
    if system_candidate.is_file() {
        return Some(system_candidate);
    }

    None
}

pub fn find_repo_config_path(start: &Path) -> Option<PathBuf> {
    for directory in start.ancestors() {
        let candidate = directory.join(".mem").join("config.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub fn resolve_secret_value(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .or_else(|| discover_repo_env_path().and_then(|path| env_lookup(&path, key)))
        .or_else(|| discover_global_env_path().and_then(|path| env_lookup(&path, key)))
}

fn env_lookup(path: &Path, key: &str) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((name, value)) = trimmed.split_once('=') {
            if name.trim() == key {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceConfig {
    #[serde(default = "default_bind_addr")]
    pub bind_addr: String,
    #[serde(default = "default_capnp_unix_socket")]
    pub capnp_unix_socket: String,
    #[serde(default = "default_capnp_tcp_addr")]
    pub capnp_tcp_addr: String,
    #[serde(default = "default_api_token")]
    pub api_token: String,
    #[serde(default = "default_request_timeout")]
    #[serde(with = "humantime_serde")]
    pub request_timeout: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FeatureFlags {
    #[serde(default)]
    pub llm_curation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    #[serde(default = "default_llm_provider")]
    pub provider: String,
    #[serde(default = "default_llm_base_url")]
    pub base_url: String,
    #[serde(default = "default_llm_api_key_env")]
    pub api_key_env: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub temperature: f32,
    #[serde(default = "default_llm_max_input_bytes")]
    pub max_input_bytes: usize,
    #[serde(default = "default_llm_max_output_tokens")]
    pub max_output_tokens: u32,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: default_llm_provider(),
            base_url: default_llm_base_url(),
            api_key_env: default_llm_api_key_env(),
            model: String::new(),
            temperature: 0.0,
            max_input_bytes: default_llm_max_input_bytes(),
            max_output_tokens: default_llm_max_output_tokens(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    #[serde(default = "default_embeddings_provider")]
    pub provider: String,
    #[serde(default = "default_embeddings_base_url")]
    pub base_url: String,
    #[serde(default = "default_embeddings_api_key_env")]
    pub api_key_env: String,
    #[serde(default)]
    pub model: String,
    #[serde(default = "default_embeddings_batch_size")]
    pub batch_size: usize,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            provider: default_embeddings_provider(),
            base_url: default_embeddings_base_url(),
            api_key_env: default_embeddings_api_key_env(),
            model: String::new(),
            batch_size: default_embeddings_batch_size(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub mode: AutomationMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<String>,
    #[serde(default = "default_poll_interval")]
    #[serde(with = "humantime_serde")]
    pub poll_interval: Duration,
    #[serde(default = "default_capture_idle_threshold", alias = "idle_threshold")]
    #[serde(with = "humantime_serde")]
    pub capture_idle_threshold: Duration,
    #[serde(default = "default_min_changed_files")]
    pub min_changed_files: usize,
    #[serde(default)]
    pub require_passing_test: bool,
    #[serde(default = "default_curate_after_captures")]
    pub curate_after_captures: usize,
    #[serde(default = "default_curate_on_explicit_flush")]
    pub curate_on_explicit_flush: bool,
    #[serde(default)]
    pub ignored_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audit_log_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_file_path: Option<String>,
}

impl Default for AutomationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: AutomationMode::Suggest,
            repo_root: None,
            poll_interval: default_poll_interval(),
            capture_idle_threshold: default_capture_idle_threshold(),
            min_changed_files: default_min_changed_files(),
            require_passing_test: false,
            curate_after_captures: default_curate_after_captures(),
            curate_on_explicit_flush: default_curate_on_explicit_flush(),
            ignored_paths: Vec::new(),
            audit_log_path: None,
            state_file_path: None,
        }
    }
}

fn default_bind_addr() -> String {
    "127.0.0.1:4040".to_string()
}

fn default_api_token() -> String {
    "dev-memory-token".to_string()
}

fn default_capnp_unix_socket() -> String {
    "/tmp/memory-layer.capnp.sock".to_string()
}

fn default_capnp_tcp_addr() -> String {
    "127.0.0.1:4041".to_string()
}

fn default_request_timeout() -> Duration {
    Duration::from_secs(30)
}

fn default_llm_provider() -> String {
    "openai_compatible".to_string()
}

fn default_llm_base_url() -> String {
    "https://api.openai.com/v1".to_string()
}

fn default_llm_api_key_env() -> String {
    "OPENAI_API_KEY".to_string()
}

fn default_embeddings_provider() -> String {
    "openai_compatible".to_string()
}

fn default_embeddings_base_url() -> String {
    "https://api.openai.com/v1".to_string()
}

fn default_embeddings_api_key_env() -> String {
    "OPENAI_API_KEY".to_string()
}

fn default_embeddings_batch_size() -> usize {
    16
}

fn default_llm_max_input_bytes() -> usize {
    120_000
}

fn default_llm_max_output_tokens() -> u32 {
    3_000
}

fn default_poll_interval() -> Duration {
    Duration::from_secs(10)
}

fn default_capture_idle_threshold() -> Duration {
    Duration::from_secs(600)
}

fn default_min_changed_files() -> usize {
    2
}

fn default_curate_after_captures() -> usize {
    3
}

fn default_curate_on_explicit_flush() -> bool {
    true
}

#[derive(Debug, Error)]
#[error("{message}")]
pub struct ValidationError {
    message: String,
}

pub async fn write_capnp_text_frame<W>(writer: &mut W, text: &str) -> Result<(), std::io::Error>
where
    W: AsyncWrite + Unpin,
{
    let payload = encode_capnp_text(text)?;
    writer.write_u32_le(payload.len() as u32).await?;
    writer.write_all(&payload).await?;
    writer.flush().await
}

pub async fn read_capnp_text_frame<R>(reader: &mut R) -> Result<Option<String>, std::io::Error>
where
    R: AsyncRead + Unpin,
{
    let len = match reader.read_u32_le().await {
        Ok(len) => len,
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut buf = vec![0_u8; len as usize];
    reader.read_exact(&mut buf).await?;
    decode_capnp_text(&buf).map(Some)
}

pub fn encode_capnp_text(text: &str) -> Result<Vec<u8>, std::io::Error> {
    let mut message = capnp::message::Builder::new_default();
    message
        .set_root::<capnp::text::Owned>(text)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))?;
    let mut bytes = Vec::new();
    serialize::write_message(&mut bytes, &message)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))?;
    Ok(bytes)
}

pub fn decode_capnp_text(bytes: &[u8]) -> Result<String, std::io::Error> {
    let mut cursor = Cursor::new(bytes);
    let reader = serialize::read_message(&mut cursor, ReaderOptions::new())
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))?;
    let text = reader
        .get_root::<capnp::text::Reader<'_>>()
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))?;
    text.to_string()
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))
}

impl ValidationError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        sync::{Mutex, OnceLock},
    };

    use super::*;

    #[test]
    fn query_request_rejects_empty_query() {
        let request = QueryRequest {
            project: "memory".to_string(),
            query: String::new(),
            filters: QueryFilters::default(),
            top_k: 8,
            min_confidence: None,
        };

        assert!(request.validate().is_err());
    }

    #[test]
    fn capture_task_requires_project() {
        let request = CaptureTaskRequest {
            project: String::new(),
            task_title: "task".to_string(),
            user_prompt: "prompt".to_string(),
            agent_summary: "summary".to_string(),
            files_changed: Vec::new(),
            git_diff_summary: None,
            tests: Vec::new(),
            notes: Vec::new(),
            structured_candidates: Vec::new(),
            command_output: None,
            idempotency_key: None,
        };

        assert!(request.validate().is_err());
    }

    #[test]
    fn finds_repo_local_mem_config() {
        let temp_dir = unique_temp_dir("mem-api-config");
        let mem_dir = temp_dir.join(".mem");
        fs::create_dir_all(&mem_dir).unwrap();
        let config_path = mem_dir.join("config.toml");
        fs::write(&config_path, "test = true\n").unwrap();

        let nested = temp_dir.join("nested").join("deeper");
        fs::create_dir_all(&nested).unwrap();

        let discovered = find_repo_config_path(&nested).unwrap();
        assert_eq!(discovered, config_path);

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn prefers_xdg_global_config_path_when_present() {
        let _guard = env_lock().lock().unwrap();
        let temp_dir = unique_temp_dir("mem-api-global");
        let config_home = temp_dir.join("config-home");
        fs::create_dir_all(config_home.join("memory-layer")).unwrap();
        let global_path = config_home.join("memory-layer").join("memory-layer.toml");
        fs::write(&global_path, "test = true\n").unwrap();

        unsafe {
            env::set_var("XDG_CONFIG_HOME", &config_home);
        }
        let discovered = discover_global_config_path().unwrap();
        unsafe {
            env::remove_var("XDG_CONFIG_HOME");
        }

        assert_eq!(discovered, global_path);
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn repo_config_is_found_from_nested_directory() {
        let temp_dir = unique_temp_dir("mem-api-repo");
        let mem_dir = temp_dir.join(".mem");
        fs::create_dir_all(&mem_dir).unwrap();
        let config_path = mem_dir.join("config.toml");
        fs::write(&config_path, "[automation]\nenabled = false\n").unwrap();

        let nested = temp_dir.join("nested").join("src");
        fs::create_dir_all(&nested).unwrap();

        assert_eq!(find_repo_config_path(&nested).unwrap(), config_path);
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn shared_env_file_overrides_config_for_explicit_path() {
        let _guard = env_lock().lock().unwrap();
        let temp_dir = unique_temp_dir("mem-api-shared-env");
        let config_dir = temp_dir.join("config");
        fs::create_dir_all(&config_dir).unwrap();
        let config_path = config_dir.join("memory-layer.toml");
        fs::write(
            &config_path,
            "[service]\nbind_addr = \"127.0.0.1:4040\"\ncapnp_unix_socket = \"/tmp/a.sock\"\ncapnp_tcp_addr = \"127.0.0.1:4041\"\napi_token = \"from-config\"\nrequest_timeout = \"30s\"\n\n[database]\nurl = \"postgresql://config\"\n",
        )
        .unwrap();
        fs::write(
            config_dir.join("memory-layer.env"),
            "MEMORY_LAYER__DATABASE__URL=postgresql://from-env\nMEMORY_LAYER__SERVICE__API_TOKEN=from-env\nOPENAI_API_KEY=test\n",
        )
        .unwrap();

        unsafe {
            env::remove_var("MEMORY_LAYER__DATABASE__URL");
            env::remove_var("MEMORY_LAYER__SERVICE__API_TOKEN");
        }
        let config = AppConfig::load_from_path(Some(config_path)).unwrap();
        unsafe {
            env::remove_var("MEMORY_LAYER__DATABASE__URL");
            env::remove_var("MEMORY_LAYER__SERVICE__API_TOKEN");
        }

        assert_eq!(config.database.url, "postgresql://from-env");
        assert_eq!(config.service.api_token, "from-env");
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn process_env_still_wins_over_env_file_for_explicit_path() {
        let _guard = env_lock().lock().unwrap();
        let temp_dir = unique_temp_dir("mem-api-env-precedence");
        let config_dir = temp_dir.join("config");
        fs::create_dir_all(&config_dir).unwrap();
        let config_path = config_dir.join("memory-layer.toml");
        fs::write(
            &config_path,
            "[service]\nbind_addr = \"127.0.0.1:4040\"\ncapnp_unix_socket = \"/tmp/a.sock\"\ncapnp_tcp_addr = \"127.0.0.1:4041\"\napi_token = \"from-config\"\nrequest_timeout = \"30s\"\n\n[database]\nurl = \"postgresql://config\"\n",
        )
        .unwrap();
        fs::write(
            config_dir.join("memory-layer.env"),
            "MEMORY_LAYER__DATABASE__URL=postgresql://from-env-file\n",
        )
        .unwrap();

        unsafe {
            env::remove_var("MEMORY_LAYER__DATABASE__URL");
            env::set_var(
                "MEMORY_LAYER__DATABASE__URL",
                "postgresql://from-process-env",
            );
        }
        let config = AppConfig::load_from_path(Some(config_path)).unwrap();
        unsafe {
            env::remove_var("MEMORY_LAYER__DATABASE__URL");
        }

        assert_eq!(config.database.url, "postgresql://from-process-env");
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn legacy_and_new_capture_threshold_keys_can_be_merged() {
        let _guard = env_lock().lock().unwrap();
        let temp_dir = unique_temp_dir("mem-api-threshold-merge");
        let config_home = temp_dir.join("config-home");
        let repo_dir = temp_dir.join("repo");
        let global_dir = config_home.join("memory-layer");
        let mem_dir = repo_dir.join(".mem");
        fs::create_dir_all(&global_dir).unwrap();
        fs::create_dir_all(&mem_dir).unwrap();

        fs::write(
            global_dir.join("memory-layer.toml"),
            "[service]\nbind_addr = \"127.0.0.1:4040\"\ncapnp_unix_socket = \"/tmp/a.sock\"\ncapnp_tcp_addr = \"127.0.0.1:4041\"\napi_token = \"from-config\"\nrequest_timeout = \"30s\"\n\n[database]\nurl = \"postgresql://config\"\n\n[automation]\nidle_threshold = \"5m\"\n",
        )
        .unwrap();
        fs::write(
            mem_dir.join("config.toml"),
            "[automation]\ncapture_idle_threshold = \"10m\"\n",
        )
        .unwrap();

        let original_dir = env::current_dir().unwrap();
        unsafe {
            env::set_var("XDG_CONFIG_HOME", &config_home);
        }
        env::set_current_dir(&repo_dir).unwrap();
        let config = AppConfig::load_from_path(None).unwrap();
        env::set_current_dir(original_dir).unwrap();
        unsafe {
            env::remove_var("XDG_CONFIG_HOME");
        }

        assert_eq!(
            config.automation.capture_idle_threshold,
            Duration::from_secs(600)
        );
        let _ = fs::remove_dir_all(temp_dir);
    }

    fn unique_temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        if path.exists() {
            let _ = fs::remove_dir_all(&path);
        }
        path
    }

    fn env_lock() -> &'static Mutex<()> {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }
}
