use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use mem_api::{QueryResponse, ResumeResponse, TokenUsage, UpToSpeedResponse};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvalItemMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_capability: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub difficulty: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim: Option<String>,
}

impl EvalItemMetadata {
    pub fn is_empty(&self) -> bool {
        self.reasoning_mode.is_none()
            && self.memory_capability.is_none()
            && self.difficulty.is_none()
            && self.claim.is_none()
    }

    fn group_value(&self, field: &str) -> Option<&str> {
        match field {
            "reasoning_mode" => self.reasoning_mode.as_deref(),
            "memory_capability" => self.memory_capability.as_deref(),
            "difficulty" => self.difficulty.as_deref(),
            "claim" => self.claim.as_deref(),
            _ => None,
        }
    }
}

fn skip_metadata(value: &EvalItemMetadata) -> bool {
    value.is_empty()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalSuiteManifest {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suite_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixture: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_profile: Option<EvalProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_items: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_repeats: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default = "default_items_path")]
    pub items: String,
}

fn default_items_path() -> String {
    "items.jsonl".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalSuite {
    pub manifest: EvalSuiteManifest,
    pub root: PathBuf,
    pub items: Vec<EvalItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "eval_type", rename_all = "snake_case")]
pub enum EvalItem {
    RetrievalQa(RetrievalQaItem),
    GroundedAnswer(GroundedAnswerItem),
    ResumeQuality(ResumeQualityItem),
    CommandTask(CommandTaskItem),
    AgentBuildTask(AgentBuildTaskItem),
    AgentBuildSequence(AgentBuildSequenceItem),
}

impl EvalItem {
    pub fn id(&self) -> &str {
        match self {
            Self::RetrievalQa(item) => &item.id,
            Self::GroundedAnswer(item) => &item.id,
            Self::ResumeQuality(item) => &item.id,
            Self::CommandTask(item) => &item.id,
            Self::AgentBuildTask(item) => &item.id,
            Self::AgentBuildSequence(item) => &item.id,
        }
    }

    pub fn project<'a>(&'a self, default_project: &'a str) -> &'a str {
        match self {
            Self::RetrievalQa(item) => item.project.as_deref().unwrap_or(default_project),
            Self::GroundedAnswer(item) => item.project.as_deref().unwrap_or(default_project),
            Self::ResumeQuality(item) => item.project.as_deref().unwrap_or(default_project),
            Self::CommandTask(item) => item.project.as_deref().unwrap_or(default_project),
            Self::AgentBuildTask(item) => item.project.as_deref().unwrap_or(default_project),
            Self::AgentBuildSequence(item) => item.project.as_deref().unwrap_or(default_project),
        }
    }

    pub fn metadata(&self) -> &EvalItemMetadata {
        match self {
            Self::RetrievalQa(item) => &item.metadata,
            Self::GroundedAnswer(item) => &item.metadata,
            Self::ResumeQuality(item) => &item.metadata,
            Self::CommandTask(item) => &item.metadata,
            Self::AgentBuildTask(item) => &item.metadata,
            Self::AgentBuildSequence(item) => &item.metadata,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalQaItem {
    pub id: String,
    #[serde(flatten, default, skip_serializing_if = "skip_metadata")]
    pub metadata: EvalItemMetadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    pub question: String,
    #[serde(default = "default_top_k")]
    pub top_k: i64,
    #[serde(default)]
    pub expected_memory_ids: Vec<Uuid>,
    #[serde(default)]
    pub expected_tags: Vec<String>,
    #[serde(default)]
    pub expected_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundedAnswerItem {
    pub id: String,
    #[serde(flatten, default, skip_serializing_if = "skip_metadata")]
    pub metadata: EvalItemMetadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    pub question: String,
    #[serde(default = "default_top_k")]
    pub top_k: i64,
    #[serde(default)]
    pub expected_memory_ids: Vec<Uuid>,
    #[serde(default)]
    pub required_assertions: Vec<String>,
    #[serde(default)]
    pub forbidden_assertions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeQualityItem {
    pub id: String,
    #[serde(flatten, default, skip_serializing_if = "skip_metadata")]
    pub metadata: EvalItemMetadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub required_topics: Vec<String>,
    #[serde(default)]
    pub forbidden_topics: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandTaskItem {
    pub id: String,
    #[serde(flatten, default, skip_serializing_if = "skip_metadata")]
    pub metadata: EvalItemMetadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    pub prompt: String,
    pub command: String,
    #[serde(default)]
    pub expected_exit_code: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentBuildTaskItem {
    pub id: String,
    #[serde(flatten, default, skip_serializing_if = "skip_metadata")]
    pub metadata: EvalItemMetadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    pub prompt: String,
    pub fixture: String,
    pub agent_command: String,
    #[serde(default)]
    pub memory_questions: Vec<String>,
    #[serde(default)]
    pub setup_commands: Vec<String>,
    #[serde(default)]
    pub score_commands: Vec<String>,
    #[serde(default = "default_agent_build_timeout_seconds")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub required_files: Vec<String>,
    #[serde(default)]
    pub forbidden_files: Vec<String>,
    #[serde(default)]
    pub required_content: Vec<AgentBuildContentAssertion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentBuildSequenceItem {
    pub id: String,
    #[serde(flatten, default, skip_serializing_if = "skip_metadata")]
    pub metadata: EvalItemMetadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    pub fixture: String,
    pub agent_command: String,
    #[serde(default)]
    pub setup_commands: Vec<String>,
    #[serde(default = "default_agent_build_timeout_seconds")]
    pub timeout_seconds: u64,
    pub steps: Vec<AgentBuildSequenceStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentBuildSequenceStep {
    pub id: String,
    #[serde(flatten, default, skip_serializing_if = "skip_metadata")]
    pub metadata: EvalItemMetadata,
    pub prompt: String,
    #[serde(default)]
    pub memory_questions: Vec<String>,
    #[serde(default)]
    pub score_commands: Vec<String>,
    #[serde(default)]
    pub required_files: Vec<String>,
    #[serde(default)]
    pub forbidden_files: Vec<String>,
    #[serde(default)]
    pub required_content: Vec<AgentBuildContentAssertion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentBuildContentAssertion {
    pub file: String,
    pub contains: String,
}

fn default_top_k() -> i64 {
    8
}

fn default_agent_build_timeout_seconds() -> u64 {
    900
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum EvalCondition {
    NoMemory,
    Lexical,
    Semantic,
    Graph,
    FullMemory,
}

impl std::fmt::Display for EvalCondition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::NoMemory => "no-memory",
            Self::Lexical => "lexical",
            Self::Semantic => "semantic",
            Self::Graph => "graph",
            Self::FullMemory => "full-memory",
        };
        f.write_str(value)
    }
}

impl std::str::FromStr for EvalCondition {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "no-memory" => Ok(Self::NoMemory),
            "lexical" => Ok(Self::Lexical),
            "semantic" => Ok(Self::Semantic),
            "graph" => Ok(Self::Graph),
            "full-memory" => Ok(Self::FullMemory),
            other => bail!(
                "unknown eval condition `{other}`; expected no-memory, lexical, semantic, graph, or full-memory"
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub enum EvalProfile {
    #[default]
    Llm,
    Offline,
}

impl std::fmt::Display for EvalProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Llm => "llm",
            Self::Offline => "offline",
        };
        f.write_str(value)
    }
}

impl std::str::FromStr for EvalProfile {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "llm" => Ok(Self::Llm),
            "offline" => Ok(Self::Offline),
            other => bail!("unknown eval profile `{other}`; expected llm or offline"),
        }
    }
}

fn default_run_group_id() -> Uuid {
    Uuid::nil()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalRun {
    pub suite: String,
    pub project: String,
    pub condition: EvalCondition,
    #[serde(default)]
    pub profile: EvalProfile,
    #[serde(default = "default_run_group_id")]
    pub run_group_id: Uuid,
    #[serde(default)]
    pub repeat_index: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suite_checksum: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixture_checksum: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_fingerprint: Option<String>,
    pub dry_run: bool,
    pub created_at: DateTime<Utc>,
    pub git_head: Option<String>,
    pub service_version: Option<String>,
    pub results: Vec<EvalItemResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalItemResult {
    pub item_id: String,
    pub eval_type: String,
    pub condition: EvalCondition,
    #[serde(default, skip_serializing_if = "skip_metadata")]
    pub metadata: EvalItemMetadata,
    pub success: bool,
    pub skipped: bool,
    pub scores: BTreeMap<String, f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<TokenUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sub_results: Vec<EvalSubResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalSubResult {
    pub id: String,
    pub eval_type: String,
    #[serde(default, skip_serializing_if = "skip_metadata")]
    pub metadata: EvalItemMetadata,
    pub success: bool,
    pub skipped: bool,
    pub scores: BTreeMap<String, f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<TokenUsage>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalComparison {
    pub baseline_condition: EvalCondition,
    pub candidate_condition: EvalCondition,
    pub paired_items: usize,
    pub baseline_profile: EvalProfile,
    pub candidate_profile: EvalProfile,
    pub baseline_success_rate: f64,
    pub candidate_success_rate: f64,
    pub success_rate_delta: f64,
    pub mcnemar_b: usize,
    pub mcnemar_c: usize,
    pub mcnemar_p_value: f64,
    pub baseline_total_tokens: u64,
    pub candidate_total_tokens: u64,
    pub token_delta: i64,
    pub baseline_mean_duration_ms: f64,
    pub candidate_mean_duration_ms: f64,
    pub duration_delta_ms: f64,
    pub cost_adjusted_success_delta_per_1k_tokens: f64,
    pub metric_deltas: BTreeMap<String, MetricDelta>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub groups: BTreeMap<String, EvalComparisonGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalComparisonGroup {
    pub paired_items: usize,
    pub baseline_success_rate: f64,
    pub candidate_success_rate: f64,
    pub success_rate_delta: f64,
    pub mcnemar_b: usize,
    pub mcnemar_c: usize,
    pub mcnemar_p_value: f64,
    pub baseline_total_tokens: u64,
    pub candidate_total_tokens: u64,
    pub token_delta: i64,
    pub baseline_mean_duration_ms: f64,
    pub candidate_mean_duration_ms: f64,
    pub duration_delta_ms: f64,
    pub cost_adjusted_success_delta_per_1k_tokens: f64,
    pub metric_deltas: BTreeMap<String, MetricDelta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalGatePolicy {
    #[serde(default)]
    pub min_paired_items: usize,
    #[serde(default)]
    pub min_success_rate_delta: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_mcnemar_p_value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_token_delta: Option<i64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub min_metric_delta: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalGateResult {
    pub passed: bool,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricDelta {
    pub baseline_mean: f64,
    pub candidate_mean: f64,
    pub mean_delta: f64,
    pub ci95_low: f64,
    pub ci95_high: f64,
}

pub fn load_suite(path: &Path) -> Result<EvalSuite> {
    let manifest_path = if path.is_dir() {
        path.join("suite.toml")
    } else {
        path.to_path_buf()
    };
    let root = manifest_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let manifest: EvalSuiteManifest = toml::from_str(
        &fs::read_to_string(&manifest_path)
            .with_context(|| format!("read {}", manifest_path.display()))?,
    )
    .with_context(|| format!("parse {}", manifest_path.display()))?;
    let items_path = root.join(&manifest.items);
    let items = load_items_jsonl(&items_path)?;
    Ok(EvalSuite {
        manifest,
        root,
        items,
    })
}

pub fn load_items_jsonl(path: &Path) -> Result<Vec<EvalItem>> {
    let body = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut items = Vec::new();
    for (index, line) in body.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let item: EvalItem = serde_json::from_str(line)
            .with_context(|| format!("parse {} line {}", path.display(), index + 1))?;
        items.push(item);
    }
    Ok(items)
}

pub fn suite_checksum(suite: &EvalSuite) -> Result<String> {
    let manifest_path = suite.root.join("suite.toml");
    let items_path = suite.root.join(&suite.manifest.items);
    let mut hasher = Sha256::new();
    hasher.update(
        fs::read(&manifest_path)
            .with_context(|| format!("read {} for suite checksum", manifest_path.display()))?,
    );
    hasher.update(b"\n--items--\n");
    hasher.update(
        fs::read(&items_path)
            .with_context(|| format!("read {} for suite checksum", items_path.display()))?,
    );
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(path, serde_json::to_string_pretty(value)?)
        .with_context(|| format!("write {}", path.display()))
}

pub fn load_run(path: &Path) -> Result<EvalRun> {
    serde_json::from_str(
        &fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?,
    )
    .with_context(|| format!("parse {}", path.display()))
}

pub fn score_retrieval_qa(
    item: &RetrievalQaItem,
    condition: EvalCondition,
    response: &QueryResponse,
) -> EvalItemResult {
    let mut scores = retrieval_scores(
        &item.expected_memory_ids,
        &item.expected_tags,
        &item.expected_files,
        response,
    );
    scores.insert(
        "semantic_candidates".to_string(),
        response.diagnostics.semantic_candidates as f64,
    );
    scores.insert(
        "graph_candidates".to_string(),
        response.diagnostics.graph_candidates as f64,
    );
    let success = retrieval_success(&scores);
    EvalItemResult {
        item_id: item.id.clone(),
        eval_type: "retrieval_qa".to_string(),
        condition,
        metadata: item.metadata.clone(),
        success,
        skipped: false,
        scores,
        duration_ms: Some(response.diagnostics.total_duration_ms),
        token_usage: response.answer_generation.token_usage.clone(),
        answer: Some(response.answer.clone()),
        notes: Vec::new(),
        sub_results: Vec::new(),
    }
}

pub fn score_grounded_answer(
    item: &GroundedAnswerItem,
    condition: EvalCondition,
    response: &QueryResponse,
) -> EvalItemResult {
    let mut scores = retrieval_scores(&item.expected_memory_ids, &[], &[], response);
    let assertion_scores = assertion_scores(
        &response.answer,
        &item.required_assertions,
        &item.forbidden_assertions,
    );
    let assertion_score = assertion_scores.assertion_recall;
    let forbidden_hits = assertion_scores.forbidden_hits;
    scores.insert("assertion_recall".to_string(), assertion_score);
    scores.insert("forbidden_hits".to_string(), forbidden_hits as f64);
    scores.insert("confidence".to_string(), response.confidence as f64);
    let success = assertion_score >= 1.0 && forbidden_hits == 0;
    EvalItemResult {
        item_id: item.id.clone(),
        eval_type: "grounded_answer".to_string(),
        condition,
        metadata: item.metadata.clone(),
        success,
        skipped: false,
        scores,
        duration_ms: Some(
            response.diagnostics.total_duration_ms + response.answer_generation.duration_ms,
        ),
        token_usage: response.answer_generation.token_usage.clone(),
        answer: Some(response.answer.clone()),
        notes: Vec::new(),
        sub_results: Vec::new(),
    }
}

pub fn score_plain_llm_grounded_answer(
    item: &GroundedAnswerItem,
    condition: EvalCondition,
    answer: String,
    confidence: Option<f32>,
    duration_ms: Option<u64>,
    token_usage: Option<TokenUsage>,
    mut notes: Vec<String>,
) -> EvalItemResult {
    let assertion_scores = assertion_scores(
        &answer,
        &item.required_assertions,
        &item.forbidden_assertions,
    );
    let mut scores = BTreeMap::new();
    scores.insert(
        "assertion_recall".to_string(),
        assertion_scores.assertion_recall,
    );
    scores.insert(
        "forbidden_hits".to_string(),
        assertion_scores.forbidden_hits as f64,
    );
    if let Some(confidence) = confidence {
        scores.insert("confidence".to_string(), confidence as f64);
    }
    notes.push("plain_llm: no Memory retrieval context supplied".to_string());
    EvalItemResult {
        item_id: item.id.clone(),
        eval_type: "grounded_answer".to_string(),
        condition,
        metadata: item.metadata.clone(),
        success: assertion_scores.assertion_recall >= 1.0 && assertion_scores.forbidden_hits == 0,
        skipped: false,
        scores,
        duration_ms,
        token_usage,
        answer: Some(answer),
        notes,
        sub_results: Vec::new(),
    }
}

pub fn score_resume_quality(
    item: &ResumeQualityItem,
    condition: EvalCondition,
    response: &ResumeResponse,
) -> EvalItemResult {
    score_briefing(BriefingScoreInput {
        item_id: &item.id,
        eval_type: "resume_quality",
        condition,
        briefing: &response.briefing,
        required_topics: &item.required_topics,
        forbidden_topics: &item.forbidden_topics,
        tokens: None,
        metadata: item.metadata.clone(),
    })
}

pub fn score_up_to_speed_quality(
    item: &ResumeQualityItem,
    condition: EvalCondition,
    response: &UpToSpeedResponse,
) -> EvalItemResult {
    score_briefing(BriefingScoreInput {
        item_id: &item.id,
        eval_type: "resume_quality",
        condition,
        briefing: &response.briefing,
        required_topics: &item.required_topics,
        forbidden_topics: &item.forbidden_topics,
        tokens: Some(response.token_usage.total_tokens),
        metadata: item.metadata.clone(),
    })
}

struct BriefingScoreInput<'a> {
    item_id: &'a str,
    eval_type: &'a str,
    condition: EvalCondition,
    briefing: &'a str,
    required_topics: &'a [String],
    forbidden_topics: &'a [String],
    tokens: Option<u64>,
    metadata: EvalItemMetadata,
}

fn score_briefing(input: BriefingScoreInput<'_>) -> EvalItemResult {
    let text = input.briefing.to_lowercase();
    let topic_hits = input
        .required_topics
        .iter()
        .filter(|value| text.contains(&value.to_lowercase()))
        .count();
    let forbidden_hits = input
        .forbidden_topics
        .iter()
        .filter(|value| text.contains(&value.to_lowercase()))
        .count();
    let topic_recall = if input.required_topics.is_empty() {
        1.0
    } else {
        topic_hits as f64 / input.required_topics.len() as f64
    };
    let mut scores = BTreeMap::new();
    scores.insert("topic_recall".to_string(), topic_recall);
    scores.insert("forbidden_hits".to_string(), forbidden_hits as f64);
    EvalItemResult {
        item_id: input.item_id.to_string(),
        eval_type: input.eval_type.to_string(),
        condition: input.condition,
        metadata: input.metadata,
        success: topic_recall >= 1.0 && forbidden_hits == 0,
        skipped: false,
        scores,
        duration_ms: None,
        token_usage: input.tokens.map(|total_tokens| TokenUsage {
            total_tokens,
            ..TokenUsage::default()
        }),
        answer: Some(input.briefing.to_string()),
        notes: Vec::new(),
        sub_results: Vec::new(),
    }
}

pub fn score_resume_text_quality(
    item: &ResumeQualityItem,
    condition: EvalCondition,
    briefing: String,
    duration_ms: Option<u64>,
    token_usage: Option<TokenUsage>,
    mut notes: Vec<String>,
) -> EvalItemResult {
    let mut result = score_briefing(BriefingScoreInput {
        item_id: &item.id,
        eval_type: "resume_quality",
        condition,
        briefing: &briefing,
        required_topics: &item.required_topics,
        forbidden_topics: &item.forbidden_topics,
        tokens: None,
        metadata: item.metadata.clone(),
    });
    result.duration_ms = duration_ms;
    result.token_usage = token_usage;
    result.answer = Some(briefing);
    notes.push("plain_llm: no Memory timeline or retrieval context supplied".to_string());
    result.notes = notes;
    result
}

struct AssertionScores {
    assertion_recall: f64,
    forbidden_hits: usize,
}

fn assertion_scores(
    answer: &str,
    required_assertions: &[String],
    forbidden_assertions: &[String],
) -> AssertionScores {
    let answer_lower = answer.to_lowercase();
    let required_hits = required_assertions
        .iter()
        .filter(|value| answer_lower.contains(&value.to_lowercase()))
        .count();
    let forbidden_hits = forbidden_assertions
        .iter()
        .filter(|value| answer_lower.contains(&value.to_lowercase()))
        .count();
    let assertion_recall = if required_assertions.is_empty() {
        1.0
    } else {
        required_hits as f64 / required_assertions.len() as f64
    };
    AssertionScores {
        assertion_recall,
        forbidden_hits,
    }
}

pub fn score_command_task(
    item: &CommandTaskItem,
    condition: EvalCondition,
    exit_code: Option<i32>,
    duration_ms: Option<u64>,
    notes: Vec<String>,
) -> EvalItemResult {
    let actual = exit_code.unwrap_or(-1);
    let mut scores = BTreeMap::new();
    scores.insert("exit_code".to_string(), actual as f64);
    EvalItemResult {
        item_id: item.id.clone(),
        eval_type: "command_task".to_string(),
        condition,
        metadata: item.metadata.clone(),
        success: exit_code == Some(item.expected_exit_code),
        skipped: exit_code.is_none(),
        scores,
        duration_ms,
        token_usage: None,
        answer: None,
        notes,
        sub_results: Vec::new(),
    }
}

#[derive(Debug, Clone)]
pub struct AgentBuildScoreInput {
    pub agent_exit_code: Option<i32>,
    pub setup_exit_codes: Vec<Option<i32>>,
    pub score_exit_codes: Vec<Option<i32>>,
    pub required_files_present: usize,
    pub required_files_total: usize,
    pub forbidden_files_absent: usize,
    pub forbidden_files_total: usize,
    pub content_assertions_passed: usize,
    pub content_assertions_total: usize,
    pub memory_queries_required: usize,
    pub memory_queries_verified: usize,
    pub memory_evidence_required: bool,
    pub memory_evidence_ok: bool,
    pub token_usage_required: bool,
    pub token_usage_ok: bool,
    pub token_usage: Option<TokenUsage>,
    pub duration_ms: Option<u64>,
    pub notes: Vec<String>,
    pub sub_results: Vec<EvalSubResult>,
    pub skipped: bool,
}

pub fn score_agent_build_task(
    item: &AgentBuildTaskItem,
    condition: EvalCondition,
    input: AgentBuildScoreInput,
) -> EvalItemResult {
    let setup_passed = input
        .setup_exit_codes
        .iter()
        .filter(|code| **code == Some(0))
        .count();
    let score_passed = input
        .score_exit_codes
        .iter()
        .filter(|code| **code == Some(0))
        .count();
    let mut scores = BTreeMap::new();
    scores.insert(
        "agent_exit_code".to_string(),
        input.agent_exit_code.unwrap_or(-1) as f64,
    );
    scores.insert("setup_commands_passed".to_string(), setup_passed as f64);
    scores.insert(
        "setup_commands_total".to_string(),
        input.setup_exit_codes.len() as f64,
    );
    scores.insert("score_commands_passed".to_string(), score_passed as f64);
    scores.insert(
        "score_commands_total".to_string(),
        input.score_exit_codes.len() as f64,
    );
    scores.insert(
        "required_files_present".to_string(),
        input.required_files_present as f64,
    );
    scores.insert(
        "required_files_total".to_string(),
        input.required_files_total as f64,
    );
    scores.insert(
        "forbidden_files_absent".to_string(),
        input.forbidden_files_absent as f64,
    );
    scores.insert(
        "forbidden_files_total".to_string(),
        input.forbidden_files_total as f64,
    );
    scores.insert(
        "content_assertions_passed".to_string(),
        input.content_assertions_passed as f64,
    );
    scores.insert(
        "content_assertions_total".to_string(),
        input.content_assertions_total as f64,
    );
    scores.insert(
        "memory_queries_required".to_string(),
        input.memory_queries_required as f64,
    );
    scores.insert(
        "memory_queries_verified".to_string(),
        input.memory_queries_verified as f64,
    );
    scores.insert(
        "memory_evidence_ok".to_string(),
        if input.memory_evidence_ok { 1.0 } else { 0.0 },
    );
    scores.insert(
        "token_usage_ok".to_string(),
        if input.token_usage_ok { 1.0 } else { 0.0 },
    );

    let setup_ok = setup_passed == input.setup_exit_codes.len();
    let score_ok = score_passed == input.score_exit_codes.len();
    let files_ok = input.required_files_present == input.required_files_total
        && input.forbidden_files_absent == input.forbidden_files_total;
    let content_ok = input.content_assertions_passed == input.content_assertions_total;
    let memory_ok = !input.memory_evidence_required || input.memory_evidence_ok;
    let token_ok = !input.token_usage_required || input.token_usage_ok;
    let agent_ok = input.agent_exit_code == Some(0);
    let success = !input.skipped
        && agent_ok
        && setup_ok
        && score_ok
        && files_ok
        && content_ok
        && memory_ok
        && token_ok;
    scores.insert("total_score".to_string(), if success { 1.0 } else { 0.0 });

    EvalItemResult {
        item_id: item.id.clone(),
        eval_type: "agent_build_task".to_string(),
        condition,
        metadata: item.metadata.clone(),
        success,
        skipped: input.skipped,
        scores,
        duration_ms: input.duration_ms,
        token_usage: input.token_usage,
        answer: None,
        notes: input.notes,
        sub_results: input.sub_results,
    }
}

pub fn score_agent_build_sequence(
    item: &AgentBuildSequenceItem,
    condition: EvalCondition,
    input: AgentBuildScoreInput,
) -> EvalItemResult {
    let mut result = score_agent_build_task(
        &AgentBuildTaskItem {
            id: item.id.clone(),
            metadata: item.metadata.clone(),
            project: item.project.clone(),
            prompt: String::new(),
            fixture: item.fixture.clone(),
            agent_command: item.agent_command.clone(),
            memory_questions: Vec::new(),
            setup_commands: item.setup_commands.clone(),
            score_commands: Vec::new(),
            timeout_seconds: item.timeout_seconds,
            required_files: Vec::new(),
            forbidden_files: Vec::new(),
            required_content: Vec::new(),
        },
        condition,
        input,
    );
    result.eval_type = "agent_build_sequence".to_string();
    result
}

pub fn skipped_result(
    item: &EvalItem,
    condition: EvalCondition,
    note: impl Into<String>,
) -> EvalItemResult {
    EvalItemResult {
        item_id: item.id().to_string(),
        eval_type: match item {
            EvalItem::RetrievalQa(_) => "retrieval_qa",
            EvalItem::GroundedAnswer(_) => "grounded_answer",
            EvalItem::ResumeQuality(_) => "resume_quality",
            EvalItem::CommandTask(_) => "command_task",
            EvalItem::AgentBuildTask(_) => "agent_build_task",
            EvalItem::AgentBuildSequence(_) => "agent_build_sequence",
        }
        .to_string(),
        condition,
        metadata: item.metadata().clone(),
        success: false,
        skipped: true,
        scores: BTreeMap::new(),
        duration_ms: None,
        token_usage: None,
        answer: None,
        notes: vec![note.into()],
        sub_results: Vec::new(),
    }
}

fn retrieval_scores(
    expected_memory_ids: &[Uuid],
    expected_tags: &[String],
    expected_files: &[String],
    response: &QueryResponse,
) -> BTreeMap<String, f64> {
    let expected: HashSet<Uuid> = expected_memory_ids.iter().copied().collect();
    let mut scores = BTreeMap::new();
    if expected.is_empty() {
        scores.insert("recall_at_k".to_string(), 1.0);
        scores.insert("mrr".to_string(), 1.0);
        scores.insert("ndcg".to_string(), 1.0);
    } else {
        let mut hits = 0usize;
        let mut first_hit = None;
        let mut dcg = 0.0;
        for (index, result) in response.results.iter().enumerate() {
            if expected.contains(&result.memory_id) {
                hits += 1;
                first_hit.get_or_insert(index + 1);
                dcg += 1.0 / ((index + 2) as f64).log2();
            }
        }
        let ideal_hits = expected.len().min(response.results.len());
        let idcg: f64 = (0..ideal_hits)
            .map(|index| 1.0 / ((index + 2) as f64).log2())
            .sum();
        scores.insert(
            "recall_at_k".to_string(),
            hits as f64 / expected.len() as f64,
        );
        scores.insert(
            "mrr".to_string(),
            first_hit.map(|rank| 1.0 / rank as f64).unwrap_or(0.0),
        );
        scores.insert(
            "ndcg".to_string(),
            if idcg > 0.0 { dcg / idcg } else { 0.0 },
        );
    }
    let cited_expected = response
        .answer_citations
        .iter()
        .filter(|citation| expected.contains(&citation.memory_id))
        .count();
    scores.insert(
        "citation_precision".to_string(),
        if response.answer_citations.is_empty() {
            1.0
        } else {
            cited_expected as f64 / response.answer_citations.len() as f64
        },
    );
    scores.insert(
        "tag_recall_at_k".to_string(),
        expected_value_recall(
            expected_tags,
            response
                .results
                .iter()
                .flat_map(|result| result.tags.iter().map(|tag| normalize_expected_value(tag))),
        ),
    );
    scores.insert(
        "file_recall_at_k".to_string(),
        expected_value_recall(
            expected_files,
            response.results.iter().flat_map(|result| {
                result
                    .sources
                    .iter()
                    .filter_map(|source| source.file_path.as_deref())
                    .chain(
                        result
                            .graph_connections
                            .iter()
                            .map(|connection| connection.file_path.as_str()),
                    )
                    .map(normalize_expected_value)
            }),
        ),
    );
    scores
}

fn expected_value_recall<I>(expected: &[String], observed: I) -> f64
where
    I: IntoIterator<Item = String>,
{
    if expected.is_empty() {
        return 1.0;
    }
    let observed = observed.into_iter().collect::<HashSet<_>>();
    let hits = expected
        .iter()
        .map(|value| normalize_expected_value(value))
        .filter(|value| observed.contains(value))
        .count();
    hits as f64 / expected.len() as f64
}

fn normalize_expected_value(value: &str) -> String {
    value.trim().to_lowercase()
}

fn retrieval_success(scores: &BTreeMap<String, f64>) -> bool {
    ["recall_at_k", "tag_recall_at_k", "file_recall_at_k"]
        .iter()
        .all(|name| scores.get(*name).copied().unwrap_or(1.0) >= 1.0)
}

pub fn compare_runs(baseline: &EvalRun, candidate: &EvalRun) -> EvalComparison {
    compare_run_sets(
        std::slice::from_ref(baseline),
        std::slice::from_ref(candidate),
    )
}

pub fn compare_run_sets(baselines: &[EvalRun], candidates: &[EvalRun]) -> EvalComparison {
    let baseline_condition = baselines
        .first()
        .map(|run| run.condition)
        .unwrap_or(EvalCondition::NoMemory);
    let candidate_condition = candidates
        .first()
        .map(|run| run.condition)
        .unwrap_or(EvalCondition::FullMemory);
    let baseline_profile = baselines
        .first()
        .map(|run| run.profile)
        .unwrap_or(EvalProfile::Llm);
    let candidate_profile = candidates
        .first()
        .map(|run| run.profile)
        .unwrap_or(EvalProfile::Llm);
    let pairs = paired_observations(baselines, candidates);
    let overall = summarize_pairs(&pairs);
    let groups = grouped_summaries(&pairs);
    EvalComparison {
        baseline_condition,
        candidate_condition,
        paired_items: overall.paired_items,
        baseline_profile,
        candidate_profile,
        baseline_success_rate: overall.baseline_success_rate,
        candidate_success_rate: overall.candidate_success_rate,
        success_rate_delta: overall.success_rate_delta,
        mcnemar_b: overall.mcnemar_b,
        mcnemar_c: overall.mcnemar_c,
        mcnemar_p_value: overall.mcnemar_p_value,
        baseline_total_tokens: overall.baseline_total_tokens,
        candidate_total_tokens: overall.candidate_total_tokens,
        token_delta: overall.token_delta,
        baseline_mean_duration_ms: overall.baseline_mean_duration_ms,
        candidate_mean_duration_ms: overall.candidate_mean_duration_ms,
        duration_delta_ms: overall.duration_delta_ms,
        cost_adjusted_success_delta_per_1k_tokens: overall
            .cost_adjusted_success_delta_per_1k_tokens,
        metric_deltas: overall.metric_deltas,
        groups,
    }
}

#[derive(Debug, Clone)]
struct EvalObservation {
    key: String,
    eval_type: String,
    metadata: EvalItemMetadata,
    success: bool,
    skipped: bool,
    scores: BTreeMap<String, f64>,
    duration_ms: Option<u64>,
    token_usage: Option<TokenUsage>,
}

fn paired_observations(
    baselines: &[EvalRun],
    candidates: &[EvalRun],
) -> Vec<(EvalObservation, EvalObservation)> {
    let baseline_by_key = baselines
        .iter()
        .flat_map(observations_for_run)
        .filter(|observation| !observation.skipped)
        .map(|observation| (observation.key.clone(), observation))
        .collect::<HashMap<_, _>>();
    candidates
        .iter()
        .flat_map(observations_for_run)
        .filter(|observation| !observation.skipped)
        .filter_map(|candidate| {
            baseline_by_key
                .get(&candidate.key)
                .cloned()
                .map(|baseline| (baseline, candidate))
        })
        .collect()
}

fn observations_for_run(run: &EvalRun) -> Vec<EvalObservation> {
    let mut observations = Vec::new();
    for result in &run.results {
        let base_key = format!("{}::r{}", result.item_id, run.repeat_index);
        observations.push(EvalObservation {
            key: base_key.clone(),
            eval_type: result.eval_type.clone(),
            metadata: result.metadata.clone(),
            success: result.success,
            skipped: result.skipped,
            scores: result.scores.clone(),
            duration_ms: result.duration_ms,
            token_usage: result.token_usage.clone(),
        });
        for sub_result in &result.sub_results {
            observations.push(EvalObservation {
                key: format!("{base_key}::{}", sub_result.id),
                eval_type: sub_result.eval_type.clone(),
                metadata: sub_result.metadata.clone(),
                success: sub_result.success,
                skipped: sub_result.skipped,
                scores: sub_result.scores.clone(),
                duration_ms: sub_result.duration_ms,
                token_usage: sub_result.token_usage.clone(),
            });
        }
    }
    observations
}

fn grouped_summaries(
    pairs: &[(EvalObservation, EvalObservation)],
) -> BTreeMap<String, EvalComparisonGroup> {
    let mut grouped = BTreeMap::new();
    for field in ["eval_type", "reasoning_mode", "memory_capability"] {
        let values = pairs
            .iter()
            .filter_map(|(base, cand)| group_value(field, base, cand).map(str::to_string))
            .collect::<HashSet<_>>();
        for value in values {
            let group_pairs = pairs
                .iter()
                .filter(|(base, cand)| group_value(field, base, cand) == Some(value.as_str()))
                .cloned()
                .collect::<Vec<_>>();
            grouped.insert(format!("{field}:{value}"), summarize_pairs(&group_pairs));
        }
    }
    grouped
}

fn group_value<'a>(
    field: &str,
    base: &'a EvalObservation,
    cand: &'a EvalObservation,
) -> Option<&'a str> {
    match field {
        "eval_type" => Some(cand.eval_type.as_str()),
        _ => cand
            .metadata
            .group_value(field)
            .or_else(|| base.metadata.group_value(field)),
    }
}

fn summarize_pairs(pairs: &[(EvalObservation, EvalObservation)]) -> EvalComparisonGroup {
    let paired_items = pairs.len();
    let baseline_successes = pairs.iter().filter(|(base, _)| base.success).count();
    let candidate_successes = pairs.iter().filter(|(_, cand)| cand.success).count();
    let baseline_total_tokens = pairs
        .iter()
        .filter_map(|(base, _)| base.token_usage.as_ref())
        .map(|usage| usage.total_tokens)
        .sum::<u64>();
    let candidate_total_tokens = pairs
        .iter()
        .filter_map(|(_, cand)| cand.token_usage.as_ref())
        .map(|usage| usage.total_tokens)
        .sum::<u64>();
    let baseline_durations = pairs
        .iter()
        .filter_map(|(base, _)| base.duration_ms.map(|value| value as f64))
        .collect::<Vec<_>>();
    let candidate_durations = pairs
        .iter()
        .filter_map(|(_, cand)| cand.duration_ms.map(|value| value as f64))
        .collect::<Vec<_>>();
    let baseline_mean_duration_ms = mean(&baseline_durations);
    let candidate_mean_duration_ms = mean(&candidate_durations);
    let b = pairs
        .iter()
        .filter(|(base, cand)| !base.success && cand.success)
        .count();
    let c = pairs
        .iter()
        .filter(|(base, cand)| base.success && !cand.success)
        .count();
    let baseline_success_rate = rate(baseline_successes, paired_items);
    let candidate_success_rate = rate(candidate_successes, paired_items);
    let success_rate_delta = candidate_success_rate - baseline_success_rate;

    let mut metric_names = HashSet::new();
    for (base, cand) in pairs {
        metric_names.extend(base.scores.keys().cloned());
        metric_names.extend(cand.scores.keys().cloned());
    }
    let mut metric_deltas = BTreeMap::new();
    for name in metric_names {
        let deltas = pairs
            .iter()
            .filter_map(|(base, cand)| Some(cand.scores.get(&name)? - base.scores.get(&name)?))
            .collect::<Vec<_>>();
        if deltas.is_empty() {
            continue;
        }
        let baseline_values = pairs
            .iter()
            .filter_map(|(base, _)| base.scores.get(&name).copied())
            .collect::<Vec<_>>();
        let candidate_values = pairs
            .iter()
            .filter_map(|(_, cand)| cand.scores.get(&name).copied())
            .collect::<Vec<_>>();
        let (low, high) = bootstrap_ci95(&deltas);
        metric_deltas.insert(
            name,
            MetricDelta {
                baseline_mean: mean(&baseline_values),
                candidate_mean: mean(&candidate_values),
                mean_delta: mean(&deltas),
                ci95_low: low,
                ci95_high: high,
            },
        );
    }

    let token_delta = candidate_total_tokens as i64 - baseline_total_tokens as i64;
    EvalComparisonGroup {
        paired_items,
        baseline_success_rate,
        candidate_success_rate,
        success_rate_delta,
        mcnemar_b: b,
        mcnemar_c: c,
        mcnemar_p_value: mcnemar_exact_p_value(b, c),
        baseline_total_tokens,
        candidate_total_tokens,
        token_delta,
        baseline_mean_duration_ms,
        candidate_mean_duration_ms,
        duration_delta_ms: candidate_mean_duration_ms - baseline_mean_duration_ms,
        cost_adjusted_success_delta_per_1k_tokens: cost_adjusted_success_delta(
            success_rate_delta,
            baseline_total_tokens,
            candidate_total_tokens,
        ),
        metric_deltas,
    }
}

fn cost_adjusted_success_delta(
    success_rate_delta: f64,
    baseline_total_tokens: u64,
    candidate_total_tokens: u64,
) -> f64 {
    let added_tokens = candidate_total_tokens.saturating_sub(baseline_total_tokens);
    if added_tokens == 0 {
        return success_rate_delta;
    }
    success_rate_delta / (added_tokens as f64 / 1_000.0)
}

pub fn evaluate_gate(comparison: &EvalComparison, policy: &EvalGatePolicy) -> EvalGateResult {
    let mut reasons = Vec::new();
    if comparison.paired_items < policy.min_paired_items {
        reasons.push(format!(
            "paired_items {} is below required {}",
            comparison.paired_items, policy.min_paired_items
        ));
    }
    if comparison.success_rate_delta < policy.min_success_rate_delta {
        reasons.push(format!(
            "success_rate_delta {:.4} is below required {:.4}",
            comparison.success_rate_delta, policy.min_success_rate_delta
        ));
    }
    if let Some(max_p) = policy.max_mcnemar_p_value
        && comparison.mcnemar_p_value > max_p
    {
        reasons.push(format!(
            "mcnemar_p_value {:.4} is above allowed {:.4}",
            comparison.mcnemar_p_value, max_p
        ));
    }
    if let Some(max_delta) = policy.max_token_delta
        && comparison.token_delta > max_delta
    {
        reasons.push(format!(
            "token_delta {} is above allowed {}",
            comparison.token_delta, max_delta
        ));
    }
    for (metric, required) in &policy.min_metric_delta {
        let actual = comparison
            .metric_deltas
            .get(metric)
            .map(|delta| delta.mean_delta)
            .unwrap_or(0.0);
        if actual < *required {
            reasons.push(format!(
                "{metric} delta {:.4} is below required {:.4}",
                actual, required
            ));
        }
    }
    EvalGateResult {
        passed: reasons.is_empty(),
        reasons,
    }
}

fn rate(count: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        count as f64 / total as f64
    }
}

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

pub fn mcnemar_exact_p_value(b: usize, c: usize) -> f64 {
    let n = b + c;
    if n == 0 {
        return 1.0;
    }
    let k = b.min(c);
    let tail = (0..=k).map(|i| binomial_probability(n, i)).sum::<f64>();
    (2.0 * tail).min(1.0)
}

fn binomial_probability(n: usize, k: usize) -> f64 {
    let combination = (0..k).fold(1.0, |acc, i| acc * (n - i) as f64 / (i + 1) as f64);
    combination * 0.5_f64.powi(n as i32)
}

fn bootstrap_ci95(values: &[f64]) -> (f64, f64) {
    if values.is_empty() {
        return (0.0, 0.0);
    }
    if values.len() == 1 {
        return (values[0], values[0]);
    }
    let mut state = 0x5eed_u64;
    let mut samples = Vec::with_capacity(1_000);
    for _ in 0..1_000 {
        let mut total = 0.0;
        for _ in 0..values.len() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let index = (state as usize) % values.len();
            total += values[index];
        }
        samples.push(total / values.len() as f64);
    }
    samples.sort_by(f64::total_cmp);
    let low = samples[(samples.len() as f64 * 0.025).floor() as usize];
    let high = samples[(samples.len() as f64 * 0.975).floor() as usize];
    (low, high)
}

pub fn comparison_text(comparison: &EvalComparison) -> String {
    let mut lines = vec![
        format!(
            "{} [{}] vs {} [{}] ({} paired item(s))",
            comparison.candidate_condition,
            comparison.candidate_profile,
            comparison.baseline_condition,
            comparison.baseline_profile,
            comparison.paired_items
        ),
        format!(
            "success: {:.1}% -> {:.1}% ({:+.1} pp), McNemar p={:.4}",
            comparison.baseline_success_rate * 100.0,
            comparison.candidate_success_rate * 100.0,
            comparison.success_rate_delta * 100.0,
            comparison.mcnemar_p_value
        ),
        format!(
            "tokens: {} -> {} ({:+}), mean duration: {:.1}ms -> {:.1}ms ({:+.1}ms), cost-adjusted delta/1k tokens: {:+.4}",
            comparison.baseline_total_tokens,
            comparison.candidate_total_tokens,
            comparison.token_delta,
            comparison.baseline_mean_duration_ms,
            comparison.candidate_mean_duration_ms,
            comparison.duration_delta_ms,
            comparison.cost_adjusted_success_delta_per_1k_tokens
        ),
    ];
    for (name, delta) in &comparison.metric_deltas {
        lines.push(format!(
            "{name}: {:.3} -> {:.3} ({:+.3}, 95% CI {:+.3}..{:+.3})",
            delta.baseline_mean,
            delta.candidate_mean,
            delta.mean_delta,
            delta.ci95_low,
            delta.ci95_high
        ));
    }
    if !comparison.groups.is_empty() {
        lines.push("groups:".to_string());
        for (name, group) in &comparison.groups {
            lines.push(format!(
                "- {name}: {} pair(s), success {:.1}% -> {:.1}% ({:+.1} pp), tokens {} -> {} ({:+})",
                group.paired_items,
                group.baseline_success_rate * 100.0,
                group.candidate_success_rate * 100.0,
                group.success_rate_delta * 100.0,
                group.baseline_total_tokens,
                group.candidate_total_tokens,
                group.token_delta
            ));
        }
    }
    lines.join("\n")
}

pub fn comparison_markdown(comparison: &EvalComparison) -> String {
    let mut lines = vec![
        "# Memory Eval Comparison".to_string(),
        String::new(),
        format!(
            "**Candidate:** `{}` / `{}`",
            comparison.candidate_condition, comparison.candidate_profile
        ),
        format!(
            "**Baseline:** `{}` / `{}`",
            comparison.baseline_condition, comparison.baseline_profile
        ),
        String::new(),
        "## Summary".to_string(),
        String::new(),
        "| Metric | Baseline | Candidate | Delta |".to_string(),
        "| --- | ---: | ---: | ---: |".to_string(),
        format!(
            "| Success rate | {:.1}% | {:.1}% | {:+.1} pp |",
            comparison.baseline_success_rate * 100.0,
            comparison.candidate_success_rate * 100.0,
            comparison.success_rate_delta * 100.0
        ),
        format!(
            "| Total tokens | {} | {} | {:+} |",
            comparison.baseline_total_tokens,
            comparison.candidate_total_tokens,
            comparison.token_delta
        ),
        format!(
            "| Mean duration | {:.1} ms | {:.1} ms | {:+.1} ms |",
            comparison.baseline_mean_duration_ms,
            comparison.candidate_mean_duration_ms,
            comparison.duration_delta_ms
        ),
        format!(
            "| Cost-adjusted success delta / 1k added tokens |  |  | {:+.4} |",
            comparison.cost_adjusted_success_delta_per_1k_tokens
        ),
        format!(
            "| McNemar p-value |  |  | {:.4} |",
            comparison.mcnemar_p_value
        ),
        String::new(),
        "## Metric Deltas".to_string(),
        String::new(),
    ];
    push_metric_table(&mut lines, &comparison.metric_deltas);
    if !comparison.groups.is_empty() {
        lines.extend([
            String::new(),
            "## Groups".to_string(),
            String::new(),
            "| Group | Pairs | Baseline success | Candidate success | Delta | Token delta | Cost-adjusted delta / 1k |".to_string(),
            "| --- | ---: | ---: | ---: | ---: | ---: | ---: |".to_string(),
        ]);
        for (name, group) in &comparison.groups {
            lines.push(format!(
                "| `{}` | {} | {:.1}% | {:.1}% | {:+.1} pp | {:+} | {:+.4} |",
                name,
                group.paired_items,
                group.baseline_success_rate * 100.0,
                group.candidate_success_rate * 100.0,
                group.success_rate_delta * 100.0,
                group.token_delta,
                group.cost_adjusted_success_delta_per_1k_tokens
            ));
        }
    }
    lines.push(String::new());
    lines.push("## Interpretation".to_string());
    lines.push(String::new());
    lines.push(
        "Treat positive full-memory deltas as evidence only when paired items, grouped reasoning modes, and deterministic checks all point in the same direction. Token deltas describe the price paid for that improvement."
            .to_string(),
    );
    lines.join("\n")
}

fn push_metric_table(lines: &mut Vec<String>, metric_deltas: &BTreeMap<String, MetricDelta>) {
    if metric_deltas.is_empty() {
        lines.push("No paired metric deltas were available.".to_string());
        return;
    }
    lines.push("| Metric | Baseline | Candidate | Delta | 95% CI |".to_string());
    lines.push("| --- | ---: | ---: | ---: | ---: |".to_string());
    for (name, delta) in metric_deltas {
        lines.push(format!(
            "| `{}` | {:.3} | {:.3} | {:+.3} | {:+.3}..{:+.3} |",
            name,
            delta.baseline_mean,
            delta.candidate_mean,
            delta.mean_delta,
            delta.ci95_low,
            delta.ci95_high
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcnemar_handles_no_discordant_pairs() {
        assert_eq!(mcnemar_exact_p_value(0, 0), 1.0);
    }

    #[test]
    fn mcnemar_detects_asymmetric_discordant_pairs() {
        assert!(mcnemar_exact_p_value(8, 1) < 0.05);
    }

    #[test]
    fn comparison_pairs_items_and_reports_success_delta() {
        let baseline = EvalRun {
            suite: "s".to_string(),
            project: "p".to_string(),
            condition: EvalCondition::NoMemory,
            profile: EvalProfile::Offline,
            run_group_id: Uuid::new_v4(),
            repeat_index: 0,
            suite_checksum: None,
            fixture_checksum: None,
            config_fingerprint: None,
            dry_run: false,
            created_at: Utc::now(),
            git_head: None,
            service_version: None,
            results: vec![result("a", false, 0.0), result("b", true, 0.5)],
        };
        let candidate = EvalRun {
            condition: EvalCondition::FullMemory,
            results: vec![result("a", true, 1.0), result("b", true, 1.0)],
            ..baseline.clone()
        };
        let comparison = compare_runs(&baseline, &candidate);
        assert_eq!(comparison.paired_items, 2);
        assert_eq!(comparison.mcnemar_b, 1);
        assert_eq!(comparison.mcnemar_c, 0);
        assert_eq!(comparison.success_rate_delta, 0.5);
        assert!(comparison.metric_deltas.contains_key("recall_at_k"));
    }

    #[test]
    fn retrieval_scoring_requires_expected_tags_and_files() {
        let item = RetrievalQaItem {
            id: "tag-file".to_string(),
            metadata: EvalItemMetadata::default(),
            project: None,
            question: "What changed?".to_string(),
            top_k: 8,
            expected_memory_ids: Vec::new(),
            expected_tags: vec!["graph".to_string()],
            expected_files: vec!["crates/mem-search/src/lib.rs".to_string()],
        };
        let response = QueryResponse {
            answer: String::new(),
            confidence: 1.0,
            results: vec![mem_api::QueryResult {
                memory_id: Uuid::new_v4(),
                summary: "Graph retrieval".to_string(),
                memory_type: mem_api::MemoryType::Reference,
                score: 1.0,
                snippet: "graph".to_string(),
                match_kind: mem_api::QueryMatchKind::Lexical,
                score_explanation: Vec::new(),
                debug: mem_api::QueryResultDebug::default(),
                tags: vec!["graph".to_string()],
                sources: vec![mem_api::QuerySource {
                    task_id: None,
                    file_path: Some("crates/mem-search/src/lib.rs".to_string()),
                    source_kind: mem_api::SourceKind::File,
                    excerpt: None,
                }],
                graph_connections: Vec::new(),
            }],
            insufficient_evidence: false,
            answer_generation: mem_api::QueryAnswerGeneration::default(),
            answer_citations: Vec::new(),
            diagnostics: mem_api::QueryDiagnostics::default(),
        };

        let result = score_retrieval_qa(&item, EvalCondition::FullMemory, &response);

        assert!(result.success);
        assert_eq!(result.scores["tag_recall_at_k"], 1.0);
        assert_eq!(result.scores["file_recall_at_k"], 1.0);
    }

    #[test]
    fn comparison_groups_sequence_steps_by_reasoning_mode() {
        let baseline = EvalRun {
            suite: "s".to_string(),
            project: "p".to_string(),
            condition: EvalCondition::NoMemory,
            profile: EvalProfile::Offline,
            run_group_id: Uuid::new_v4(),
            repeat_index: 0,
            suite_checksum: None,
            fixture_checksum: None,
            config_fingerprint: None,
            dry_run: false,
            created_at: Utc::now(),
            git_head: None,
            service_version: None,
            results: vec![sequence_result(false, "deductive")],
        };
        let candidate = EvalRun {
            condition: EvalCondition::FullMemory,
            results: vec![sequence_result(true, "deductive")],
            ..baseline.clone()
        };

        let comparison = compare_runs(&baseline, &candidate);

        assert_eq!(comparison.paired_items, 2);
        let group = comparison
            .groups
            .get("reasoning_mode:deductive")
            .expect("reasoning group");
        assert_eq!(group.paired_items, 1);
        assert_eq!(group.success_rate_delta, 1.0);
        assert!(comparison_markdown(&comparison).contains("Cost-adjusted"));
    }

    #[test]
    fn gate_reports_failed_policy_reasons() {
        let comparison = EvalComparison {
            baseline_condition: EvalCondition::NoMemory,
            candidate_condition: EvalCondition::FullMemory,
            paired_items: 2,
            baseline_profile: EvalProfile::Llm,
            candidate_profile: EvalProfile::Llm,
            baseline_success_rate: 0.5,
            candidate_success_rate: 0.5,
            success_rate_delta: 0.0,
            mcnemar_b: 0,
            mcnemar_c: 0,
            mcnemar_p_value: 1.0,
            baseline_total_tokens: 10,
            candidate_total_tokens: 30,
            token_delta: 20,
            baseline_mean_duration_ms: 10.0,
            candidate_mean_duration_ms: 12.0,
            duration_delta_ms: 2.0,
            cost_adjusted_success_delta_per_1k_tokens: 0.0,
            metric_deltas: BTreeMap::from([(
                "recall_at_k".to_string(),
                MetricDelta {
                    baseline_mean: 0.2,
                    candidate_mean: 0.25,
                    mean_delta: 0.05,
                    ci95_low: 0.0,
                    ci95_high: 0.1,
                },
            )]),
            groups: BTreeMap::new(),
        };

        let gate = evaluate_gate(
            &comparison,
            &EvalGatePolicy {
                min_paired_items: 10,
                min_success_rate_delta: 0.1,
                max_mcnemar_p_value: Some(0.05),
                max_token_delta: Some(5),
                min_metric_delta: BTreeMap::from([("recall_at_k".to_string(), 0.1)]),
            },
        );

        assert!(!gate.passed);
        assert_eq!(gate.reasons.len(), 5);
    }

    #[test]
    fn plain_llm_grounded_answer_scores_assertions_and_tokens() {
        let item = GroundedAnswerItem {
            id: "plain-answer".to_string(),
            metadata: EvalItemMetadata::default(),
            project: None,
            question: "How are tokens reported?".to_string(),
            top_k: 8,
            expected_memory_ids: Vec::new(),
            required_assertions: vec!["token".to_string()],
            forbidden_assertions: vec!["database password".to_string()],
        };

        let result = score_plain_llm_grounded_answer(
            &item,
            EvalCondition::NoMemory,
            "The report includes token usage.".to_string(),
            Some(0.8),
            Some(42),
            Some(TokenUsage {
                input_tokens: 10,
                output_tokens: 5,
                total_tokens: 15,
                ..TokenUsage::default()
            }),
            Vec::new(),
        );

        assert!(result.success);
        assert_eq!(result.duration_ms, Some(42));
        assert_eq!(result.token_usage.unwrap().total_tokens, 15);
        assert!(
            result
                .notes
                .iter()
                .any(|note| note.contains("no Memory retrieval"))
        );
    }

    #[test]
    fn resume_text_quality_preserves_full_token_usage() {
        let item = ResumeQualityItem {
            id: "resume".to_string(),
            metadata: EvalItemMetadata::default(),
            project: None,
            prompt: String::new(),
            required_topics: vec!["memory".to_string()],
            forbidden_topics: Vec::new(),
        };

        let result = score_resume_text_quality(
            &item,
            EvalCondition::NoMemory,
            "Memory context is unavailable.".to_string(),
            Some(12),
            Some(TokenUsage {
                input_tokens: 7,
                output_tokens: 8,
                cache_read_tokens: 1,
                total_tokens: 16,
                ..TokenUsage::default()
            }),
            Vec::new(),
        );

        assert!(result.success);
        assert_eq!(result.duration_ms, Some(12));
        let usage = result.token_usage.unwrap();
        assert_eq!(usage.input_tokens, 7);
        assert_eq!(usage.cache_read_tokens, 1);
        assert_eq!(usage.total_tokens, 16);
    }

    #[test]
    fn parses_agent_build_task_items() {
        let line = r#"{"eval_type":"agent_build_task","id":"app","project":"memory","prompt":"Build it","fixture":"fixtures/app","agent_command":"sh agent.sh","memory_questions":["What changed recently?"],"score_commands":["sh scripts/check.sh"],"required_files":["index.html"],"forbidden_files":["debug.log"],"required_content":[{"file":"index.html","contains":"Launch"}]}"#;

        let item: EvalItem = serde_json::from_str(line).expect("parse agent build task");

        let EvalItem::AgentBuildTask(item) = item else {
            panic!("expected agent build task");
        };
        assert_eq!(item.id, "app");
        assert_eq!(item.timeout_seconds, 900);
        assert_eq!(item.memory_questions, vec!["What changed recently?"]);
        assert_eq!(item.score_commands, vec!["sh scripts/check.sh"]);
        assert_eq!(item.required_content[0].contains, "Launch");
    }

    #[test]
    fn parses_agent_build_sequence_items() {
        let line = r#"{"eval_type":"agent_build_sequence","id":"app-sequence","project":"memory","fixture":"fixtures/app","agent_command":"sh agent.sh","steps":[{"id":"hero","prompt":"Build hero","memory_questions":["What matters?"],"score_commands":["sh scripts/check.sh"],"required_files":["index.html"],"required_content":[{"file":"index.html","contains":"Hero"}]}]}"#;

        let item: EvalItem = serde_json::from_str(line).expect("parse sequence");

        let EvalItem::AgentBuildSequence(item) = item else {
            panic!("expected agent build sequence");
        };
        assert_eq!(item.id, "app-sequence");
        assert_eq!(item.steps.len(), 1);
        assert_eq!(item.steps[0].id, "hero");
        assert_eq!(item.steps[0].memory_questions, vec!["What matters?"]);
        assert_eq!(item.steps[0].timeout_seconds, None);
    }

    #[test]
    fn agent_build_task_scores_deterministic_checks() {
        let item = AgentBuildTaskItem {
            id: "app".to_string(),
            metadata: EvalItemMetadata::default(),
            project: Some("memory".to_string()),
            prompt: "Build it".to_string(),
            fixture: "fixtures/app".to_string(),
            agent_command: "sh agent.sh".to_string(),
            memory_questions: vec!["What should I know first?".to_string()],
            setup_commands: vec!["true".to_string()],
            score_commands: vec!["sh scripts/check.sh".to_string()],
            timeout_seconds: 60,
            required_files: vec!["index.html".to_string()],
            forbidden_files: vec!["debug.log".to_string()],
            required_content: vec![AgentBuildContentAssertion {
                file: "index.html".to_string(),
                contains: "Launch".to_string(),
            }],
        };

        let result = score_agent_build_task(
            &item,
            EvalCondition::FullMemory,
            AgentBuildScoreInput {
                agent_exit_code: Some(0),
                setup_exit_codes: vec![Some(0)],
                score_exit_codes: vec![Some(0)],
                required_files_present: 1,
                required_files_total: 1,
                forbidden_files_absent: 1,
                forbidden_files_total: 1,
                content_assertions_passed: 1,
                content_assertions_total: 1,
                memory_queries_required: 1,
                memory_queries_verified: 1,
                memory_evidence_required: true,
                memory_evidence_ok: true,
                token_usage_required: false,
                token_usage_ok: true,
                token_usage: None,
                duration_ms: Some(10),
                notes: Vec::new(),
                sub_results: Vec::new(),
                skipped: false,
            },
        );

        assert!(result.success);
        assert_eq!(result.eval_type, "agent_build_task");
        assert_eq!(result.scores["total_score"], 1.0);
        assert_eq!(result.scores["memory_evidence_ok"], 1.0);
    }

    fn result(id: &str, success: bool, recall: f64) -> EvalItemResult {
        EvalItemResult {
            item_id: id.to_string(),
            eval_type: "retrieval_qa".to_string(),
            condition: EvalCondition::FullMemory,
            metadata: EvalItemMetadata::default(),
            success,
            skipped: false,
            scores: BTreeMap::from([("recall_at_k".to_string(), recall)]),
            duration_ms: None,
            token_usage: None,
            answer: None,
            notes: Vec::new(),
            sub_results: Vec::new(),
        }
    }

    fn sequence_result(success: bool, reasoning_mode: &str) -> EvalItemResult {
        EvalItemResult {
            item_id: "sequence".to_string(),
            eval_type: "agent_build_sequence".to_string(),
            condition: EvalCondition::FullMemory,
            metadata: EvalItemMetadata::default(),
            success,
            skipped: false,
            scores: BTreeMap::from([("total_score".to_string(), if success { 1.0 } else { 0.0 })]),
            duration_ms: None,
            token_usage: None,
            answer: None,
            notes: Vec::new(),
            sub_results: vec![EvalSubResult {
                id: "step".to_string(),
                eval_type: "agent_build_sequence_step".to_string(),
                metadata: EvalItemMetadata {
                    reasoning_mode: Some(reasoning_mode.to_string()),
                    ..EvalItemMetadata::default()
                },
                success,
                skipped: false,
                scores: BTreeMap::from([(
                    "total_score".to_string(),
                    if success { 1.0 } else { 0.0 },
                )]),
                duration_ms: None,
                token_usage: None,
                notes: Vec::new(),
            }],
        }
    }
}
