use std::{fmt, path::PathBuf, time::Duration};

use chrono::{DateTime, Utc};
use config::{Config, ConfigError, Environment, File};
use serde::{Deserialize, Serialize};
use thiserror::Error;
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
pub struct AppConfig {
    pub service: ServiceConfig,
    pub database: DatabaseConfig,
    #[serde(default)]
    pub features: FeatureFlags,
}

impl AppConfig {
    pub fn load_from_path(path: Option<PathBuf>) -> Result<Self, ConfigError> {
        let mut builder = Config::builder();
        if let Some(path) = path {
            builder = builder.add_source(File::from(path).required(false));
        } else {
            builder = builder.add_source(File::with_name("memory-layer").required(false));
        }

        builder
            .add_source(Environment::with_prefix("MEMORY_LAYER").separator("__"))
            .build()?
            .try_deserialize()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceConfig {
    #[serde(default = "default_bind_addr")]
    pub bind_addr: String,
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

fn default_bind_addr() -> String {
    "127.0.0.1:4040".to_string()
}

fn default_api_token() -> String {
    "dev-memory-token".to_string()
}

fn default_request_timeout() -> Duration {
    Duration::from_secs(30)
}

#[derive(Debug, Error)]
#[error("{message}")]
pub struct ValidationError {
    message: String,
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
}
