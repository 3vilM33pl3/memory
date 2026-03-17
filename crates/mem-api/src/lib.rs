use std::{
    env, fmt,
    io::Cursor,
    path::{Path, PathBuf},
    time::Duration,
};

use capnp::{message::ReaderOptions, serialize};
use chrono::{DateTime, Utc};
use config::{Config, ConfigError, Environment, File};
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
    pub tags: Vec<String>,
    #[serde(default)]
    pub sources: Vec<QuerySource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResponse {
    pub answer: String,
    pub confidence: f32,
    pub results: Vec<QueryResult>,
    pub insufficient_evidence: bool,
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
    Ack {
        message: String,
    },
    Pong,
    Error {
        message: String,
    },
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
    pub automation: AutomationConfig,
}

impl AppConfig {
    pub fn load_from_path(path: Option<PathBuf>) -> Result<Self, ConfigError> {
        let mut builder = Config::builder();
        if let Some(path) = path {
            builder = builder.add_source(File::from(path).required(false));
        } else {
            if let Some(path) = discover_global_config_path() {
                builder = builder.add_source(File::from(path).required(false));
            } else {
                builder = builder.add_source(File::with_name("memory-layer").required(false));
            }
            if let Some(path) = discover_repo_config_path() {
                builder = builder.add_source(File::from(path).required(false));
            }
        }

        builder
            .add_source(Environment::with_prefix("MEMORY_LAYER").separator("__"))
            .build()?
            .try_deserialize()
    }
}

pub fn discover_repo_config_path() -> Option<PathBuf> {
    let cwd = env::current_dir().ok()?;
    find_repo_config_path(&cwd)
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
    #[serde(default = "default_idle_threshold")]
    #[serde(with = "humantime_serde")]
    pub idle_threshold: Duration,
    #[serde(default = "default_min_changed_files")]
    pub min_changed_files: usize,
    #[serde(default)]
    pub require_passing_test: bool,
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
            idle_threshold: default_idle_threshold(),
            min_changed_files: default_min_changed_files(),
            require_passing_test: false,
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

fn default_poll_interval() -> Duration {
    Duration::from_secs(10)
}

fn default_idle_threshold() -> Duration {
    Duration::from_secs(300)
}

fn default_min_changed_files() -> usize {
    2
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
    use std::{env, fs};

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
}
