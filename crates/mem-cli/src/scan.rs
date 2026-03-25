use std::{
    collections::{BTreeSet, HashSet},
    env, fs,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
};

use anyhow::{Context, Result};
use chrono::Utc;
use mem_api::{
    AgentProjectConfig, AppConfig, CaptureCandidateInput, CaptureCandidateSourceInput,
    CaptureTaskRequest, MemoryType, SourceKind, discover_global_config_path,
    discover_repo_env_path, load_repo_agent_settings,
};
use reqwest::{Client, header};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::ApiClient;

const PROMPT_VERSION: &str = "scan-v1";
const MAX_CANDIDATES: usize = 12;
const MAX_FILE_BYTES: usize = 8_000;
const MAX_FILES: usize = 18;
const MAX_COMMITS: usize = 20;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ScanReport {
    pub project: String,
    pub repo_root: String,
    pub files_considered: usize,
    pub commits_considered: usize,
    pub candidate_count: usize,
    pub written: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub curate_run_id: Option<String>,
    pub report_path: String,
    pub summary: String,
}

pub(crate) async fn run_scan(
    api: &ApiClient,
    repo_root: &Path,
    project: &str,
    since: Option<&str>,
    dry_run: bool,
    writer_id: &str,
    writer_name: Option<&str>,
) -> Result<ScanReport> {
    ensure_llm_config(&api.config)?;
    let dossier = build_dossier(repo_root, project, since, api.config.llm.max_input_bytes)?;
    let response = analyze_dossier(&api.client, &api.config, &dossier).await?;
    let candidates = validate_candidates(response.candidates)?;
    let summary = normalize_summary(&response.summary, project, &candidates);
    let request = build_capture_request(
        project,
        &dossier,
        &summary,
        &candidates,
        writer_id,
        writer_name,
    )?;
    let report_path =
        write_scan_report(repo_root, project, &dossier, &summary, &candidates, dry_run)?;

    if dry_run {
        return Ok(ScanReport {
            project: project.to_string(),
            repo_root: repo_root.display().to_string(),
            files_considered: dossier.files.len(),
            commits_considered: dossier.commits.len(),
            candidate_count: candidates.len(),
            written: false,
            capture_id: None,
            curate_run_id: None,
            report_path: report_path.display().to_string(),
            summary,
        });
    }

    let capture = api.capture_task(&request).await?;
    let curate = api.curate(project).await?;
    Ok(ScanReport {
        project: project.to_string(),
        repo_root: repo_root.display().to_string(),
        files_considered: dossier.files.len(),
        commits_considered: dossier.commits.len(),
        candidate_count: candidates.len(),
        written: true,
        capture_id: Some(capture.raw_capture_id.to_string()),
        curate_run_id: Some(curate.run_id.to_string()),
        report_path: report_path.display().to_string(),
        summary,
    })
}

fn ensure_llm_config(config: &AppConfig) -> Result<()> {
    if config.llm.provider.trim() != "openai_compatible" {
        anyhow::bail!("unsupported llm.provider: {}", config.llm.provider);
    }
    if config.llm.model.trim().is_empty() {
        anyhow::bail!("missing [llm].model in config");
    }
    let api_key = llm_api_key(config).unwrap_or_default();
    if api_key.trim().is_empty() {
        let repo_env = discover_repo_env_path()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<repo-local env file not found>".to_string());
        let shared_env = discover_global_config_path()
            .map(|path| {
                path.parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join("memory-layer.env")
                    .display()
                    .to_string()
            })
            .unwrap_or_else(|| "<shared env file not found>".to_string());
        anyhow::bail!(
            "missing LLM API key {}. Checked process env, {}, and {}",
            config.llm.api_key_env,
            repo_env,
            shared_env
        );
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
struct ScanDossier {
    project: String,
    repo_root: String,
    head: Option<String>,
    files: Vec<RepoFileContext>,
    commits: Vec<GitCommitContext>,
}

#[derive(Debug, Clone, Serialize)]
struct RepoFileContext {
    path: String,
    score: i32,
    content: String,
}

#[derive(Debug, Clone, Serialize)]
struct GitCommitContext {
    hash: String,
    committed_at: String,
    subject: String,
    body: String,
    files: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessageResponse,
}

#[derive(Debug, Deserialize)]
struct ChatMessageResponse {
    content: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequest<'a> {
    model: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    max_completion_tokens: u32,
    response_format: serde_json::Value,
    messages: Vec<ChatMessage<'a>>,
}

#[derive(Debug, Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ScanLlmResponse {
    summary: String,
    candidates: Vec<ScanLlmCandidate>,
}

#[derive(Debug, Deserialize)]
struct ScanLlmCandidate {
    canonical_text: String,
    summary: String,
    memory_type: MemoryType,
    #[serde(default)]
    confidence: Option<f32>,
    #[serde(default)]
    importance: Option<i32>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    provenance_files: Vec<String>,
    #[serde(default)]
    provenance_commits: Vec<String>,
    #[serde(default)]
    rationale: String,
}

fn build_dossier(
    repo_root: &Path,
    project: &str,
    since: Option<&str>,
    max_input_bytes: usize,
) -> Result<ScanDossier> {
    let repo_root = repo_root
        .canonicalize()
        .with_context(|| format!("canonicalize {}", repo_root.display()))?;
    let head = git_output(repo_root.as_path(), ["rev-parse", "HEAD"])
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty());
    let file_budget = max_input_bytes.saturating_mul(7) / 10;
    let commit_budget = max_input_bytes.saturating_sub(file_budget);

    Ok(ScanDossier {
        project: project.to_string(),
        repo_root: repo_root.display().to_string(),
        head,
        files: collect_repo_files(repo_root.as_path(), file_budget)?,
        commits: collect_git_history(repo_root.as_path(), since, commit_budget)?,
    })
}

fn collect_repo_files(repo_root: &Path, budget: usize) -> Result<Vec<RepoFileContext>> {
    let settings = load_repo_agent_settings(repo_root).unwrap_or_default();
    let tracked = git_output(repo_root, ["ls-files"])
        .map(|output| output.lines().map(ToOwned::to_owned).collect::<Vec<_>>())
        .unwrap_or_default();

    let mut scored = tracked
        .into_iter()
        .filter(|path| !is_ignored_path(path, &settings))
        .filter_map(|path| {
            let score = file_score(&path, &settings);
            (score > 0).then_some((path, score))
        })
        .collect::<Vec<_>>();

    scored.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));

    let mut files = Vec::new();
    let mut used = 0usize;
    for (path, score) in scored.into_iter().take(MAX_FILES * 3) {
        if files.len() >= MAX_FILES || used >= budget {
            break;
        }
        let full_path = repo_root.join(&path);
        if !full_path.is_file() {
            continue;
        }
        let content = match fs::read_to_string(&full_path) {
            Ok(content) => content,
            Err(_) => continue,
        };
        let trimmed = trim_text(&content, MAX_FILE_BYTES);
        if trimmed.trim().is_empty() {
            continue;
        }
        let bytes = trimmed.len();
        if used + bytes > budget && !files.is_empty() {
            break;
        }
        used += bytes;
        files.push(RepoFileContext {
            path,
            score,
            content: trimmed,
        });
    }

    Ok(files)
}

fn collect_git_history(
    repo_root: &Path,
    since: Option<&str>,
    budget: usize,
) -> Result<Vec<GitCommitContext>> {
    let mut args = vec![
        "log",
        "--date=iso-strict",
        "--format=%x1e%H%x1f%cI%x1f%s%x1f%b",
        "--name-only",
        "--no-merges",
        "-n",
        "20",
    ];
    if let Some(since) = since {
        args.push("--since");
        args.push(since);
    }

    let output = git_output(repo_root, args).unwrap_or_default();
    let mut commits = Vec::new();
    let mut used = 0usize;
    for record in output
        .split('\u{1e}')
        .filter(|record| !record.trim().is_empty())
    {
        if commits.len() >= MAX_COMMITS || used >= budget {
            break;
        }
        let mut lines = record.lines();
        let Some(header) = lines.next() else {
            continue;
        };
        let fields = header.split('\u{1f}').collect::<Vec<_>>();
        if fields.len() < 4 {
            continue;
        }
        let files = lines
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .take(12)
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        let body = trim_text(fields[3], 800);
        let subject = fields[2].trim().to_string();
        let payload_size =
            subject.len() + body.len() + files.iter().map(String::len).sum::<usize>();
        if used + payload_size > budget && !commits.is_empty() {
            break;
        }
        used += payload_size;
        commits.push(GitCommitContext {
            hash: fields[0].trim().to_string(),
            committed_at: fields[1].trim().to_string(),
            subject,
            body,
            files,
        });
    }
    Ok(commits)
}

async fn analyze_dossier(
    client: &Client,
    config: &AppConfig,
    dossier: &ScanDossier,
) -> Result<ScanLlmResponse> {
    let api_key = llm_api_key(config)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("read {}", config.llm.api_key_env))?;
    let request = ChatCompletionRequest {
        model: &config.llm.model,
        temperature: Some(config.llm.temperature),
        max_completion_tokens: config.llm.max_output_tokens,
        response_format: serde_json::json!({ "type": "json_object" }),
        messages: vec![
            ChatMessage {
                role: "system",
                content: [
                    "You extract durable repository memory.",
                    "Return strict JSON with keys summary and candidates.",
                    "Each candidate must be repo-specific, durable, and grounded in provenance_files and/or provenance_commits.",
                    "Do not include speculative claims, transient task notes, or generic software advice.",
                    "memory_type must be one of architecture, convention, decision, incident, debugging, environment, domain_fact.",
                    "Keep candidates concise and high-signal.",
                ]
                .join(" "),
            },
            ChatMessage {
                role: "user",
                content: format!(
                    "Analyze this repository dossier and extract up to {MAX_CANDIDATES} durable memories.\n\
                     Return JSON in this shape:\n\
                     {{\"summary\":\"...\",\"candidates\":[{{\"canonical_text\":\"...\",\"summary\":\"...\",\"memory_type\":\"architecture\",\"confidence\":0.82,\"importance\":3,\"tags\":[\"...\"],\"provenance_files\":[\"path\"],\"provenance_commits\":[\"hash\"],\"rationale\":\"...\"}}]}}\n\
                     Dossier:\n{}",
                    serde_json::to_string_pretty(dossier)?
                ),
            },
        ],
    };

    let url = format!(
        "{}/chat/completions",
        config.llm.base_url.trim_end_matches('/')
    );
    let (status, body) = send_scan_request(client, &url, &api_key, &request).await?;
    if !status.is_success() {
        if request_rejects_temperature(&body) {
            let retry_request = ChatCompletionRequest {
                temperature: None,
                ..request
            };
            let (retry_status, retry_body) =
                send_scan_request(client, &url, &api_key, &retry_request).await?;
            if !retry_status.is_success() {
                anyhow::bail!("llm scan request failed: {retry_status} {retry_body}");
            }
            return parse_scan_response(&retry_body);
        }
        anyhow::bail!("llm scan request failed: {status} {body}");
    }
    parse_scan_response(&body)
}

async fn send_scan_request(
    client: &Client,
    url: &str,
    api_key: &str,
    request: &ChatCompletionRequest<'_>,
) -> Result<(reqwest::StatusCode, String)> {
    let response = client
        .post(url)
        .header(header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(header::CONTENT_TYPE, "application/json")
        .json(request)
        .send()
        .await
        .context("send llm scan request")?;
    let status = response.status();
    let body = response.text().await.context("read llm scan response")?;
    Ok((status, body))
}

fn request_rejects_temperature(body: &str) -> bool {
    body.contains("\"param\": \"temperature\"")
        || body.contains("Unsupported value: 'temperature'")
        || body.contains("Unsupported parameter: 'temperature'")
}

fn parse_scan_response(body: &str) -> Result<ScanLlmResponse> {
    let parsed: ChatCompletionResponse =
        serde_json::from_str(body).context("parse llm chat completion response")?;
    let content = parsed
        .choices
        .first()
        .map(|choice| extract_content_text(&choice.message.content))
        .transpose()?
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("llm scan response did not include message content"))?;

    serde_json::from_str(&content).context("parse llm scan JSON")
}

fn llm_api_key(config: &AppConfig) -> Option<String> {
    env::var(&config.llm.api_key_env)
        .ok()
        .or_else(|| {
            discover_repo_env_path()
                .and_then(|path| shared_env_lookup(&path, &config.llm.api_key_env))
        })
        .or_else(|| {
            discover_global_config_path()
                .map(|path| {
                    path.parent()
                        .unwrap_or_else(|| Path::new("."))
                        .join("memory-layer.env")
                })
                .and_then(|path| shared_env_lookup(&path, &config.llm.api_key_env))
        })
}

fn shared_env_lookup(path: &Path, key: &str) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
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

fn extract_content_text(value: &serde_json::Value) -> Result<String> {
    if let Some(text) = value.as_str() {
        return Ok(text.to_string());
    }
    if let Some(items) = value.as_array() {
        let mut parts = Vec::new();
        for item in items {
            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                parts.push(text.to_string());
            }
        }
        return Ok(parts.join("\n"));
    }
    anyhow::bail!("unsupported llm message content shape")
}

fn validate_candidates(raw: Vec<ScanLlmCandidate>) -> Result<Vec<CaptureCandidateInput>> {
    let mut seen = HashSet::new();
    let mut candidates = Vec::new();

    for candidate in raw {
        let canonical_text = normalize_sentence(&candidate.canonical_text);
        let summary = candidate.summary.trim().to_string();
        if canonical_text.is_empty() || summary.is_empty() {
            continue;
        }
        if candidate.provenance_files.is_empty() && candidate.provenance_commits.is_empty() {
            continue;
        }
        let dedupe_key = canonical_text.to_lowercase();
        if !seen.insert(dedupe_key) {
            continue;
        }

        let mut tags = candidate
            .tags
            .into_iter()
            .map(|tag| tag.trim().to_lowercase())
            .filter(|tag| !tag.is_empty())
            .collect::<Vec<_>>();
        tags.sort();
        tags.dedup();

        let mut sources = Vec::new();
        let mut file_paths = BTreeSet::new();
        for file in candidate.provenance_files {
            let file = file.trim().to_string();
            if file.is_empty() || !file_paths.insert(file.clone()) {
                continue;
            }
            sources.push(CaptureCandidateSourceInput {
                file_path: Some(file.clone()),
                source_kind: SourceKind::File,
                excerpt: Some(format!("Scanned file: {file}")),
            });
        }

        let mut commits = BTreeSet::new();
        for commit in candidate.provenance_commits {
            let commit = commit.trim().to_string();
            if commit.is_empty() || !commits.insert(commit.clone()) {
                continue;
            }
            sources.push(CaptureCandidateSourceInput {
                file_path: None,
                source_kind: SourceKind::GitCommit,
                excerpt: Some(format!("Scanned commit: {commit}")),
            });
        }

        if !candidate.rationale.trim().is_empty() {
            sources.push(CaptureCandidateSourceInput {
                file_path: None,
                source_kind: SourceKind::Note,
                excerpt: Some(trim_text(&candidate.rationale, 300)),
            });
        }

        candidates.push(CaptureCandidateInput {
            canonical_text,
            summary,
            memory_type: candidate.memory_type,
            confidence: candidate.confidence.unwrap_or(0.78).clamp(0.0, 1.0),
            importance: candidate.importance.unwrap_or(3).clamp(1, 5),
            tags,
            sources,
        });

        if candidates.len() >= MAX_CANDIDATES {
            break;
        }
    }

    if candidates.is_empty() {
        anyhow::bail!("scan did not produce any valid durable candidates");
    }

    Ok(candidates)
}

fn normalize_summary(summary: &str, project: &str, candidates: &[CaptureCandidateInput]) -> String {
    let trimmed = summary.trim();
    if !trimmed.is_empty() {
        return trim_text(trimmed, 240);
    }
    let preview = candidates
        .iter()
        .take(3)
        .map(|candidate| candidate.summary.clone())
        .collect::<Vec<_>>()
        .join("; ");
    trim_text(&format!("Scanned repository {project}: {preview}"), 240)
}

fn build_capture_request(
    project: &str,
    dossier: &ScanDossier,
    summary: &str,
    candidates: &[CaptureCandidateInput],
    writer_id: &str,
    writer_name: Option<&str>,
) -> Result<CaptureTaskRequest> {
    let file_paths = dossier
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    let git_diff_summary = if dossier.commits.is_empty() {
        None
    } else {
        Some(
            dossier
                .commits
                .iter()
                .map(|commit| format!("{} {}", short_hash(&commit.hash), commit.subject))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    };

    let mut hasher = Sha256::new();
    hasher.update(PROMPT_VERSION.as_bytes());
    hasher.update(project.as_bytes());
    if let Some(head) = &dossier.head {
        hasher.update(head.as_bytes());
    }
    for file in &dossier.files {
        hasher.update(file.path.as_bytes());
        hasher.update(file.content.as_bytes());
    }
    for commit in &dossier.commits {
        hasher.update(commit.hash.as_bytes());
    }
    let idempotency_key = format!("{:x}", hasher.finalize());

    Ok(CaptureTaskRequest {
        project: project.to_string(),
        task_title: format!("Repository scan for {project}"),
        user_prompt: format!(
            "Scan the repository and extract durable architecture, functionality, workflow, and setup memory for project {project}."
        ),
        writer_id: writer_id.to_string(),
        writer_name: writer_name.map(|value| value.to_string()),
        agent_summary: summary.to_string(),
        files_changed: file_paths,
        git_diff_summary,
        tests: Vec::new(),
        notes: Vec::new(),
        structured_candidates: candidates.to_vec(),
        command_output: None,
        idempotency_key: Some(idempotency_key),
    })
}

fn write_scan_report(
    repo_root: &Path,
    project: &str,
    dossier: &ScanDossier,
    summary: &str,
    candidates: &[CaptureCandidateInput],
    dry_run: bool,
) -> Result<PathBuf> {
    let scan_dir = repo_root.join(".mem").join("runtime").join("scan");
    fs::create_dir_all(&scan_dir).with_context(|| format!("create {}", scan_dir.display()))?;
    let stamp = Utc::now().format("%Y%m%dT%H%M%SZ");
    let file_name = if dry_run {
        format!("{project}-scan-{stamp}-dry-run.json")
    } else {
        format!("{project}-scan-{stamp}.json")
    };
    let path = scan_dir.join(file_name);
    let payload = serde_json::json!({
        "prompt_version": PROMPT_VERSION,
        "project": project,
        "dry_run": dry_run,
        "summary": summary,
        "files_considered": dossier.files.len(),
        "commits_considered": dossier.commits.len(),
        "dossier": dossier,
        "candidates": candidates,
    });
    fs::write(&path, serde_json::to_string_pretty(&payload)?)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

fn git_output<I, S>(repo_root: &Path, args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args = args
        .into_iter()
        .map(|value| value.as_ref().to_string())
        .collect::<Vec<_>>();
    let output = ProcessCommand::new("git")
        .args(&args)
        .current_dir(repo_root)
        .output()
        .with_context(|| format!("run git {}", args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git {} failed: {}", args.join(" "), stderr.trim());
    }
    String::from_utf8(output.stdout).context("decode git output")
}

fn is_ignored_path(path: &str, settings: &AgentProjectConfig) -> bool {
    path.starts_with(".git/")
        || path.starts_with("target/")
        || path.starts_with(".mem/")
        || path.starts_with("node_modules/")
        || path.contains("/node_modules/")
        || matches_path_prefix(path, &settings.capture.ignore_paths)
}

fn file_score(path: &str, settings: &AgentProjectConfig) -> i32 {
    let file_name = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path);
    if matches!(
        file_name,
        "Cargo.lock" | "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock"
    ) {
        return 0;
    }

    let mut score = 0;
    if matches_path_prefix(path, &settings.capture.include_paths) {
        score += 140;
    }
    if file_name.starts_with("README") {
        score += 120;
    }
    if path.starts_with("docs/") {
        score += 110;
    }
    if path == "Cargo.toml"
        || matches!(
            file_name,
            "package.json"
                | "pyproject.toml"
                | "go.mod"
                | "docker-compose.yml"
                | "docker-compose.yaml"
        )
    {
        score += 100;
    }
    if path.starts_with("crates/")
        && (path.ends_with("src/main.rs") || path.ends_with("src/lib.rs"))
    {
        score += 95;
    }
    if path.starts_with("src/") {
        score += 85;
    }
    if path.starts_with("scripts/")
        || path.starts_with("packaging/")
        || path.starts_with(".agents/skills/")
        || path.starts_with(".github/")
    {
        score += 75;
    }
    if matches!(
        Path::new(path).extension().and_then(|ext| ext.to_str()),
        Some("rs" | "toml" | "md" | "yaml" | "yml" | "json" | "sh" | "service")
    ) {
        score += 35;
    }
    score
}

fn matches_path_prefix(path: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|pattern| {
        let trimmed = pattern.trim().trim_start_matches("./");
        !trimmed.is_empty()
            && (path == trimmed
                || path.starts_with(trimmed.trim_end_matches('/'))
                || path.starts_with(&format!("{}/", trimmed.trim_end_matches('/'))))
    })
}

fn trim_text(text: &str, max_bytes: usize) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.len() <= max_bytes {
        normalized
    } else {
        let mut end = max_bytes.min(normalized.len());
        while end > 0 && !normalized.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &normalized[..end])
    }
}

fn normalize_sentence(text: &str) -> String {
    let mut value = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if value.is_empty() {
        return value;
    }
    if !value.ends_with('.') {
        value.push('.');
    }
    value
}

fn short_hash(value: &str) -> String {
    value.chars().take(8).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_candidates_dedupes_and_requires_provenance() {
        let candidates = validate_candidates(vec![
            ScanLlmCandidate {
                canonical_text: "Memory Layer uses PostgreSQL".to_string(),
                summary: "Storage backend".to_string(),
                memory_type: MemoryType::Architecture,
                confidence: Some(0.9),
                importance: Some(4),
                tags: vec!["db".to_string()],
                provenance_files: vec!["README.md".to_string()],
                provenance_commits: vec![],
                rationale: "Mentioned in the overview.".to_string(),
            },
            ScanLlmCandidate {
                canonical_text: "Memory Layer uses PostgreSQL".to_string(),
                summary: "Duplicate".to_string(),
                memory_type: MemoryType::Architecture,
                confidence: None,
                importance: None,
                tags: vec![],
                provenance_files: vec!["README.md".to_string()],
                provenance_commits: vec![],
                rationale: String::new(),
            },
        ])
        .unwrap();

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].memory_type, MemoryType::Architecture);
    }

    #[test]
    fn file_score_prioritizes_readme_and_docs() {
        let settings = AgentProjectConfig::default();
        assert!(file_score("README.md", &settings) > file_score("src/main.rs", &settings));
        assert!(
            file_score("docs/architecture.md", &settings)
                > file_score("scripts/build.sh", &settings)
        );
    }

    #[test]
    fn file_score_respects_agent_include_and_ignore_paths() {
        let settings = AgentProjectConfig {
            capture: mem_api::AgentCaptureConfig {
                include_paths: vec!["ops/".to_string()],
                ignore_paths: vec!["docs/private/".to_string()],
            },
            ..AgentProjectConfig::default()
        };

        assert!(file_score("ops/runbook.md", &settings) > file_score("misc.txt", &settings));
        assert!(is_ignored_path("docs/private/secrets.md", &settings));
    }
}
