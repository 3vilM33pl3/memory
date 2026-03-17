use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use mem_api::{
    AppConfig, AutomationConfig, AutomationMode, AutomationStatus, CaptureTaskRequest,
    CurateRequest, CurateResponse, TestResult,
};
use reqwest::{Client, header::HeaderMap};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionWindow {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_activity_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub changed_files: Vec<String>,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub passed_tests: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DecisionRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationState {
    pub project: String,
    pub repo_root: String,
    pub mode: AutomationMode,
    pub enabled: bool,
    #[serde(default)]
    pub current_session: SessionWindow,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_persisted_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_decision: Option<DecisionRecord>,
}

impl AutomationState {
    pub fn new(project: &str, repo_root: &Path, config: &AutomationConfig) -> Self {
        Self {
            project: project.to_string(),
            repo_root: repo_root.display().to_string(),
            mode: config.mode.clone(),
            enabled: config.enabled,
            current_session: SessionWindow::default(),
            last_persisted_at: None,
            last_decision: None,
        }
    }
}

pub fn default_runtime_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".memory-layer")
}

pub fn state_path(config: &AutomationConfig, repo_root: &Path) -> PathBuf {
    config
        .state_file_path
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| default_runtime_dir(repo_root).join("automation-state.json"))
}

pub fn flush_path(repo_root: &Path) -> PathBuf {
    default_runtime_dir(repo_root).join("automation-flush")
}

pub fn audit_log_path(config: &AutomationConfig, repo_root: &Path) -> PathBuf {
    config
        .audit_log_path
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| default_runtime_dir(repo_root).join("automation.log"))
}

pub async fn load_state(
    project: &str,
    repo_root: &Path,
    config: &AutomationConfig,
) -> Result<AutomationState> {
    let path = state_path(config, repo_root);
    if !path.exists() {
        return Ok(AutomationState::new(project, repo_root, config));
    }
    let content = tokio::fs::read_to_string(&path)
        .await
        .with_context(|| format!("read automation state {}", path.display()))?;
    Ok(serde_json::from_str(&content).context("parse automation state")?)
}

pub async fn save_state(state: &AutomationState, config: &AutomationConfig) -> Result<()> {
    let repo_root = PathBuf::from(&state.repo_root);
    let path = state_path(config, &repo_root);
    ensure_runtime_dir(&repo_root).await?;
    tokio::fs::write(path, serde_json::to_vec_pretty(state)?)
        .await
        .context("write automation state")?;
    Ok(())
}

pub fn to_status(state: &AutomationState) -> AutomationStatus {
    AutomationStatus {
        enabled: state.enabled,
        mode: state.mode.clone(),
        repo_root: state.repo_root.clone(),
        last_activity_at: state.current_session.last_activity_at,
        last_persisted_at: state.last_persisted_at,
        dirty_file_count: Some(state.current_session.changed_files.len()),
        pending_note_count: Some(state.current_session.notes.len()),
        last_decision: state
            .last_decision
            .as_ref()
            .map(|record| format!("{}: {}", record.action, record.reason)),
    }
}

pub async fn ensure_runtime_dir(repo_root: &Path) -> Result<()> {
    tokio::fs::create_dir_all(default_runtime_dir(repo_root))
        .await
        .context("create automation runtime directory")?;
    Ok(())
}

pub fn detect_changed_files(repo_root: &Path, ignored_paths: &[String]) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(repo_root)
        .output()
        .context("run git status --porcelain")?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let stdout = String::from_utf8(output.stdout).context("decode git status output")?;
    let mut files = BTreeSet::new();
    for line in stdout.lines() {
        if line.len() < 4 {
            continue;
        }
        let path = line[3..].trim();
        if path.is_empty() {
            continue;
        }
        let normalized = if let Some((_, new_path)) = path.split_once(" -> ") {
            new_path.to_string()
        } else {
            path.to_string()
        };
        if ignored_paths
            .iter()
            .any(|ignored| normalized.starts_with(ignored))
        {
            continue;
        }
        files.insert(normalized);
    }
    Ok(files.into_iter().collect())
}

pub fn update_session_from_repo(
    state: &mut AutomationState,
    changed_files: Vec<String>,
    automation: &AutomationConfig,
) {
    if changed_files.is_empty() {
        return;
    }
    let now = Utc::now();
    if state.current_session.started_at.is_none() {
        state.current_session.started_at = Some(now);
    }
    state.current_session.last_activity_at = Some(now);
    let mut merged = BTreeSet::new();
    for file in state.current_session.changed_files.drain(..) {
        merged.insert(file);
    }
    for file in changed_files {
        if !automation
            .ignored_paths
            .iter()
            .any(|ignored| file.starts_with(ignored))
        {
            merged.insert(file);
        }
    }
    state.current_session.changed_files = merged.into_iter().collect();
    state.current_session.notes = derive_notes(&state.current_session.changed_files);
    state.current_session.fingerprint = Some(fingerprint(&state.current_session.changed_files));
}

pub fn should_flush(
    state: &AutomationState,
    automation: &AutomationConfig,
    explicit_flush: bool,
) -> bool {
    if explicit_flush {
        return true;
    }
    let Some(last_activity) = state.current_session.last_activity_at else {
        return false;
    };
    let Ok(idle) = chrono::Duration::from_std(automation.idle_threshold) else {
        return false;
    };
    Utc::now() - last_activity >= idle
}

pub fn should_persist(state: &AutomationState, automation: &AutomationConfig) -> (bool, String) {
    if !state.enabled {
        return (false, "automation disabled".to_string());
    }
    if state.current_session.changed_files.len() < automation.min_changed_files
        && state.current_session.notes.is_empty()
    {
        return (false, "insufficient signal".to_string());
    }
    if automation.require_passing_test && state.current_session.passed_tests.is_empty() {
        return (false, "passing test required".to_string());
    }
    (true, "high confidence".to_string())
}

pub async fn append_audit_log(
    config: &AutomationConfig,
    repo_root: &Path,
    line: &str,
) -> Result<()> {
    ensure_runtime_dir(repo_root).await?;
    let path = audit_log_path(config, repo_root);
    let mut existing = if path.exists() {
        tokio::fs::read_to_string(&path).await.unwrap_or_default()
    } else {
        String::new()
    };
    existing.push_str(line);
    existing.push('\n');
    tokio::fs::write(&path, existing)
        .await
        .with_context(|| format!("write audit log {}", path.display()))?;
    Ok(())
}

pub async fn run_remember_flow(
    client: &Client,
    config: &AppConfig,
    state: &AutomationState,
) -> Result<(serde_json::Value, CurateResponse)> {
    let request = build_capture_request(state);
    let capture: serde_json::Value = send_json(
        client
            .post(service_url(config, "/v1/capture/task"))
            .headers(write_headers(&config.service.api_token)?)
            .json(&request)
            .send()
            .await?,
    )
    .await?;
    let curate: CurateResponse = send_json(
        client
            .post(service_url(config, "/v1/curate"))
            .headers(write_headers(&config.service.api_token)?)
            .json(&CurateRequest {
                project: state.project.clone(),
                batch_size: None,
            })
            .send()
            .await?,
    )
    .await?;
    Ok((capture, curate))
}

pub fn build_capture_request(state: &AutomationState) -> CaptureTaskRequest {
    let files = state.current_session.changed_files.clone();
    let summary = if files.is_empty() {
        format!(
            "Automatically captured meaningful work in project {}.",
            state.project
        )
    } else {
        format!(
            "Automatically captured meaningful work in project {} touching: {}.",
            state.project,
            files.iter().take(5).cloned().collect::<Vec<_>>().join(", ")
        )
    };
    CaptureTaskRequest {
        project: state.project.clone(),
        task_title: format!("Automatic memory update for {}", state.project),
        user_prompt: format!(
            "Automatically persisted meaningful repository work in project {}.",
            state.project
        ),
        agent_summary: summary,
        files_changed: files,
        git_diff_summary: None,
        tests: state
            .current_session
            .passed_tests
            .iter()
            .map(|command| TestResult {
                command: command.clone(),
                status: "passed".to_string(),
                output: None,
            })
            .collect(),
        notes: state.current_session.notes.clone(),
        command_output: None,
        idempotency_key: None,
    }
}

pub async fn run_once(
    config: &AppConfig,
    client: &Client,
    project: &str,
    repo_root: &Path,
    explicit_flush: bool,
) -> Result<()> {
    let mut state = load_state(project, repo_root, &config.automation).await?;
    let changed = detect_changed_files(repo_root, &config.automation.ignored_paths)?;
    update_session_from_repo(&mut state, changed, &config.automation);

    let flush_requested = explicit_flush || flush_path(repo_root).exists();
    if flush_requested {
        let _ = tokio::fs::remove_file(flush_path(repo_root)).await;
    }

    if should_flush(&state, &config.automation, flush_requested) {
        let (persist, reason) = should_persist(&state, &config.automation);
        if persist {
            match config.automation.mode {
                AutomationMode::Suggest => {
                    let decision = DecisionRecord {
                        at: Some(Utc::now()),
                        action: "suggested".to_string(),
                        reason: reason.clone(),
                    };
                    append_audit_log(
                        &config.automation,
                        repo_root,
                        &format!(
                            "{} suggested {} files",
                            Utc::now().to_rfc3339(),
                            state.current_session.changed_files.len()
                        ),
                    )
                    .await?;
                    clear_session(&mut state, decision, false);
                }
                AutomationMode::Auto => {
                    let (_capture, curate) = run_remember_flow(client, config, &state).await?;
                    let decision = DecisionRecord {
                        at: Some(Utc::now()),
                        action: "persisted".to_string(),
                        reason: format!(
                            "{} ({} captures, {} memories)",
                            reason, curate.input_count, curate.output_count
                        ),
                    };
                    append_audit_log(
                        &config.automation,
                        repo_root,
                        &format!(
                            "{} persisted automation write for project {}",
                            Utc::now().to_rfc3339(),
                            project
                        ),
                    )
                    .await?;
                    clear_session(&mut state, decision, true);
                }
            }
        } else {
            let decision = DecisionRecord {
                at: Some(Utc::now()),
                action: "skipped".to_string(),
                reason: reason.clone(),
            };
            append_audit_log(
                &config.automation,
                repo_root,
                &format!(
                    "{} skipped automation write: {}",
                    Utc::now().to_rfc3339(),
                    reason
                ),
            )
            .await?;
            clear_session(&mut state, decision, false);
        }
    }

    save_state(&state, &config.automation).await?;
    Ok(())
}

pub fn clear_session(state: &mut AutomationState, decision: DecisionRecord, persisted: bool) {
    if persisted {
        state.last_persisted_at = decision.at;
    }
    state.last_decision = Some(decision);
    state.current_session = SessionWindow::default();
}

fn derive_notes(files: &[String]) -> Vec<String> {
    if files.is_empty() {
        return Vec::new();
    }
    let subsystems = files
        .iter()
        .filter_map(|file| file.split('/').next())
        .filter(|part| !part.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .take(3)
        .collect::<Vec<_>>();

    vec![format!(
        "Updated repository work in subsystems: {}.",
        subsystems.join(", ")
    )]
}

fn fingerprint(files: &[String]) -> String {
    let mut hasher = Sha256::new();
    for file in files {
        hasher.update(file.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn service_url(config: &AppConfig, path: &str) -> String {
    format!("http://{}{}", config.service.bind_addr, path)
}

fn write_headers(token: &str) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert("x-api-token", token.parse()?);
    Ok(headers)
}

async fn send_json<T: serde::de::DeserializeOwned>(response: reqwest::Response) -> Result<T> {
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        anyhow::bail!("{status} {body}");
    }
    Ok(serde_json::from_str(&body)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_notes_from_subsystems() {
        let notes = derive_notes(&["src/main.rs".to_string(), "docs/notes.md".to_string()]);
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("src"));
    }

    #[test]
    fn status_reflects_state() {
        let mut state = AutomationState::new(
            "memory",
            Path::new("/tmp/memory"),
            &AutomationConfig::default(),
        );
        state.current_session.changed_files = vec!["src/main.rs".to_string()];
        let status = to_status(&state);
        assert_eq!(status.repo_root, "/tmp/memory");
        assert_eq!(status.dirty_file_count, Some(1));
    }
}
