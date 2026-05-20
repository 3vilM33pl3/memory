use crate::resume;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
};

use anyhow::{Context, Result};
use chrono::Utc;
use mem_api::{
    AppConfig, CaptureTaskRequest, CheckpointActivityRequest, CurateResponse, MemoryType,
    PlanActivityAction, PlanActivityRequest, TestResult, read_repo_project_slug,
};
use mem_watch::{
    build_capture_request as build_automation_capture_request,
    detect_changed_files as watch_detect_changed_files,
    fetch_project_overview as fetch_automation_overview, load_state, should_capture, should_curate,
    update_session_from_repo,
};
use reqwest::Client;
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::commands::{api::ApiClient, runtime::RememberArgs, skill_support::resolve_repo_root};
use crate::plan_execution::{
    durable_plan_source_path, ensure_checkbox_plan, normalize_plan_markdown_for_hash,
    parse_plan_checkboxes,
};
use crate::writer_identity::WriterIdentity;

pub(crate) fn resolve_project_slug(project: Option<String>, cwd: &Path) -> Result<String> {
    if let Some(project) = project {
        return Ok(project);
    }
    let repo_root = resolve_repo_root(cwd)?;
    if let Some(project) = read_repo_project_slug(&repo_root) {
        return Ok(project);
    }
    let Some(name) = repo_root.file_name().and_then(|value| value.to_str()) else {
        anyhow::bail!("could not determine project slug from current directory");
    };
    Ok(name.to_string())
}

pub(in crate::commands) fn build_remember_request(
    args: RememberArgs,
    project: &str,
    writer_id: &str,
    writer_name: Option<&str>,
) -> Result<CaptureTaskRequest> {
    let mut files_changed = args.files_changed;
    if args.auto_files {
        for file in detect_changed_files()? {
            if !files_changed.contains(&file) {
                files_changed.push(file);
            }
        }
    }

    let command_output = match args.command_output_file {
        Some(path) => Some(fs::read_to_string(path).context("read command output file")?),
        None => None,
    };

    let tests = args
        .tests_passed
        .into_iter()
        .map(|command| TestResult {
            command,
            status: "passed".to_string(),
            output: None,
        })
        .chain(args.tests_failed.into_iter().map(|command| TestResult {
            command,
            status: "failed".to_string(),
            output: None,
        }))
        .collect::<Vec<_>>();

    let title = args
        .title
        .unwrap_or_else(|| format!("Memory update for {project}"));
    let prompt = args
        .prompt
        .unwrap_or_else(|| format!("Auto-captured repository work in project {project}."));
    let summary = args
        .summary
        .unwrap_or_else(|| derive_summary(project, &files_changed));
    let mut candidate = build_remember_implementation_candidate(
        &title,
        &summary,
        &prompt,
        &args.notes,
        &files_changed,
        &tests,
        command_output.as_deref(),
    );
    if let Some(type_str) = &args.memory_type {
        candidate.memory_type = parse_memory_type_arg(type_str)?;
    }

    Ok(CaptureTaskRequest {
        project: project.to_string(),
        task_title: title,
        user_prompt: prompt,
        writer_id: writer_id.to_string(),
        writer_name: writer_name.map(|value| value.to_string()),
        agent_summary: summary,
        files_changed,
        git_diff_summary: None,
        tests,
        notes: args.notes,
        structured_candidates: vec![candidate],
        command_output,
        idempotency_key: None,
        dry_run: false,
    })
}

pub(crate) fn parse_memory_type_arg(value: &str) -> Result<MemoryType> {
    match value {
        "architecture" => Ok(MemoryType::Architecture),
        "convention" => Ok(MemoryType::Convention),
        "decision" => Ok(MemoryType::Decision),
        "incident" => Ok(MemoryType::Incident),
        "debugging" => Ok(MemoryType::Debugging),
        "environment" => Ok(MemoryType::Environment),
        "domain_fact" => Ok(MemoryType::DomainFact),
        "documentation" => Ok(MemoryType::Documentation),
        "task" => Ok(MemoryType::Task),
        "plan" => Ok(MemoryType::Plan),
        "implementation" => Ok(MemoryType::Implementation),
        "refactor" => Ok(MemoryType::Refactor),
        "user" => Ok(MemoryType::User),
        "feedback" => Ok(MemoryType::Feedback),
        "project" => Ok(MemoryType::Project),
        "reference" => Ok(MemoryType::Reference),
        _ => anyhow::bail!(
            "unknown memory type '{value}'; expected one of: architecture, convention, \
             decision, incident, debugging, environment, domain_fact, documentation, task, plan, \
             implementation, refactor, user, feedback, project, reference"
        ),
    }
}

pub(crate) async fn save_checkpoint_with_activity(
    api: &ApiClient,
    project: &str,
    repo_root: &Path,
    note: Option<String>,
) -> Result<(mem_api::ResumeCheckpoint, PathBuf)> {
    let (checkpoint, path) = resume::save_checkpoint(project, repo_root, note)?;
    let request = CheckpointActivityRequest {
        project: project.to_string(),
        checkpoint: checkpoint.clone(),
    };
    if let Err(error) = api.log_checkpoint_activity(&request).await {
        eprintln!("warning: failed to log checkpoint activity for `{project}`: {error}");
    }
    Ok((checkpoint, path))
}

pub(crate) fn preview_checkpoint(
    project: &str,
    repo_root: &Path,
    note: Option<String>,
) -> Result<(mem_api::ResumeCheckpoint, PathBuf)> {
    Ok((
        resume::build_checkpoint(project, repo_root, note),
        resume::checkpoint_store_location()?,
    ))
}

pub(crate) async fn preview_automation_flush(
    config: &AppConfig,
    client: &Client,
    project: &str,
    repo_root: &Path,
    force_curate: bool,
    writer_id: &str,
    writer_name: Option<&str>,
) -> Result<serde_json::Value> {
    let mut state = load_state(project, repo_root, &config.automation).await?;
    let changed = watch_detect_changed_files(repo_root, &config.automation.ignored_paths)?;
    update_session_from_repo(&mut state, changed, &config.automation);
    let (capture, capture_reason) = should_capture(&state, &config.automation, true);
    let overview = fetch_automation_overview(client, config, project).await?;
    let (curate, curate_reason) = should_curate(
        &config.automation,
        overview.uncurated_raw_captures,
        true,
        force_curate,
    );
    let capture_request =
        capture.then(|| build_automation_capture_request(&state, writer_id, writer_name));
    Ok(serde_json::json!({
        "project": project,
        "dry_run": true,
        "capture": {
            "would_run": capture,
            "reason": capture_reason,
            "request": capture_request,
        },
        "curate": {
            "would_run": curate,
            "reason": curate_reason,
            "force": force_curate,
            "uncurated_raw_captures": overview.uncurated_raw_captures,
        }
    }))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_plan_activity_request(
    project: &str,
    action: PlanActivityAction,
    title: &str,
    thread_key: &str,
    total_items: usize,
    completed_items: usize,
    remaining_items: Vec<String>,
    source_path: Option<String>,
) -> PlanActivityRequest {
    PlanActivityRequest {
        project: project.to_string(),
        action,
        title: title.to_string(),
        thread_key: thread_key.to_string(),
        total_items,
        completed_items,
        remaining_items,
        source_path,
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ActivePlanSelection {
    pub(crate) memory_id: Uuid,
    pub(crate) title: String,
    pub(crate) thread_key: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PlanExecutionFinishReport {
    pub(crate) project: String,
    pub(crate) thread_key: String,
    pub(crate) plan_title: String,
    pub(crate) total_items: usize,
    pub(crate) completed_items: usize,
    pub(crate) completed_item_texts: Vec<String>,
    pub(crate) remaining_items: Vec<String>,
    pub(crate) verified_complete: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ImplementationMemoryPreview {
    pub(crate) summary: String,
    pub(crate) memory_type: mem_api::MemoryType,
    pub(crate) tags: Vec<String>,
    pub(crate) canonical_text: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ImplementationMemoryResult {
    pub(crate) recorded: bool,
    pub(crate) summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) preview: Option<ImplementationMemoryPreview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) capture: Option<mem_api::CaptureTaskResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) curate: Option<CurateResponse>,
}

pub(crate) async fn resolve_active_plan_selection(
    api: &ApiClient,
    project: &str,
    thread_key: Option<&str>,
) -> Result<ActivePlanSelection> {
    let memories = api.project_memories(project).await?;
    let mut plans = memories
        .items
        .into_iter()
        .filter(|item| item.status == mem_api::MemoryStatus::Active)
        .filter(|item| item.memory_type == mem_api::MemoryType::Plan)
        .filter_map(|item| {
            extract_plan_thread_key(&item.tags).map(|key| ActivePlanSelection {
                memory_id: item.id,
                title: item.summary,
                thread_key: key.to_string(),
            })
        })
        .collect::<Vec<_>>();

    if let Some(thread_key) = thread_key {
        plans.retain(|plan| plan.thread_key == thread_key);
        return match plans.as_slice() {
            [] => anyhow::bail!("no active plan found for thread `{thread_key}`"),
            [plan] => Ok(plan.clone()),
            _ => anyhow::bail!(
                "multiple active plans found for thread `{thread_key}`; review plan memories first"
            ),
        };
    }

    match plans.as_slice() {
        [] => anyhow::bail!("no active plan memory found for `{project}`"),
        [plan] => Ok(plan.clone()),
        _ => {
            let available = plans
                .iter()
                .map(|plan| format!("{} ({})", plan.title, plan.thread_key))
                .collect::<Vec<_>>()
                .join(", ");
            anyhow::bail!(
                "multiple active plan memories found; rerun with --thread-key. Available threads: {available}"
            );
        }
    }
}

pub(crate) fn extract_plan_thread_key(tags: &[String]) -> Option<&str> {
    tags.iter()
        .find_map(|tag| tag.strip_prefix("plan-thread:"))
        .filter(|value| !value.trim().is_empty())
}

pub(crate) fn load_plan_content(
    plan_file: Option<&Path>,
    plan_stdin: bool,
) -> Result<(String, Option<PathBuf>)> {
    match (plan_file, plan_stdin) {
        (Some(_), true) => anyhow::bail!("use either --plan-file or --plan-stdin, not both"),
        (None, false) => anyhow::bail!("provide --plan-file <path> or --plan-stdin"),
        (Some(path), false) => Ok((
            fs::read_to_string(path)
                .with_context(|| format!("read plan file {}", path.display()))?,
            Some(path.to_path_buf()),
        )),
        (None, true) => {
            let mut buffer = String::new();
            std::io::stdin()
                .read_to_string(&mut buffer)
                .context("read plan content from stdin")?;
            Ok((buffer, None))
        }
    }
}

pub(crate) fn build_plan_execution_idempotency_key(
    project: &str,
    thread_key: &str,
    plan_markdown: &str,
    git_head: Option<&str>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"plan-execution");
    hasher.update(project.as_bytes());
    hasher.update(thread_key.as_bytes());
    hasher.update(normalize_plan_markdown_for_hash(plan_markdown).as_bytes());
    if let Some(git_head) = git_head.map(str::trim).filter(|value| !value.is_empty()) {
        hasher.update(git_head.as_bytes());
    }
    format!("plan-execution:{:x}", hasher.finalize())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_plan_execution_request(
    project: &str,
    writer: &WriterIdentity,
    title: &str,
    thread_key: &str,
    plan_markdown: &str,
    source_path: Option<&Path>,
    repo_root: &Path,
    git_head: Option<&str>,
) -> CaptureTaskRequest {
    let normalized_plan = normalize_plan_markdown_for_hash(plan_markdown);
    let mut sources = vec![
        mem_api::CaptureCandidateSourceInput {
            file_path: None,
            symbol_name: None,
            symbol_kind: None,
            source_kind: mem_api::SourceKind::TaskPrompt,
            excerpt: Some("Approved execution plan entered implementation.".to_string()),
        },
        mem_api::CaptureCandidateSourceInput {
            file_path: None,
            symbol_name: None,
            symbol_kind: None,
            source_kind: mem_api::SourceKind::Note,
            excerpt: Some(normalized_plan.clone()),
        },
    ];
    if let Some(source_path) = source_path
        && let Some(source_path) = durable_plan_source_path(source_path, repo_root)
    {
        sources.insert(
            0,
            mem_api::CaptureCandidateSourceInput {
                file_path: Some(source_path.display().to_string()),
                symbol_name: None,
                symbol_kind: None,
                source_kind: mem_api::SourceKind::File,
                excerpt: Some(format!(
                    "Approved plan source file: {}",
                    source_path.display()
                )),
            },
        );
    }

    CaptureTaskRequest {
        project: project.to_string(),
        task_title: format!("Approved plan: {title}"),
        user_prompt: format!("Approved execution plan for project {project}."),
        writer_id: writer.id.clone(),
        writer_name: writer.name.clone(),
        agent_summary: title.to_string(),
        files_changed: Vec::new(),
        git_diff_summary: git_head.map(|head| format!("Execution started from git HEAD {head}")),
        tests: Vec::new(),
        notes: Vec::new(),
        structured_candidates: vec![mem_api::CaptureCandidateInput {
            canonical_text: normalized_plan.clone(),
            summary: title.to_string(),
            memory_type: mem_api::MemoryType::Plan,
            confidence: 0.95,
            importance: 4,
            tags: vec![
                "plan".to_string(),
                format!("plan-thread:{thread_key}"),
                "execution-started".to_string(),
            ],
            sources,
        }],
        command_output: None,
        idempotency_key: Some(build_plan_execution_idempotency_key(
            project,
            thread_key,
            &normalized_plan,
            git_head,
        )),
        dry_run: false,
    }
}

pub(crate) fn build_task_start_idempotency_key(
    project: &str,
    thread_key: &str,
    title: &str,
    prompt: &str,
    git_head: Option<&str>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"task-start");
    hasher.update(project.as_bytes());
    hasher.update(thread_key.as_bytes());
    hasher.update(title.trim().as_bytes());
    hasher.update(prompt.trim().as_bytes());
    if let Some(git_head) = git_head.map(str::trim).filter(|value| !value.is_empty()) {
        hasher.update(git_head.as_bytes());
    }
    format!("task-start:{:x}", hasher.finalize())
}

pub(crate) fn build_task_start_canonical_text(
    project: &str,
    title: &str,
    prompt: &str,
    thread_key: &str,
    git_head: Option<&str>,
) -> String {
    let mut lines = vec![
        format!("# Task: {}", title.trim()),
        String::new(),
        "Status: started".to_string(),
        format!("Project: {project}"),
        format!("Thread: {thread_key}"),
    ];
    if let Some(git_head) = git_head.map(str::trim).filter(|value| !value.is_empty()) {
        lines.push(format!("Git head: {git_head}"));
    }
    lines.extend([
        String::new(),
        "Original user request:".to_string(),
        prompt.trim().to_string(),
    ]);
    lines.join("\n")
}

pub(crate) fn build_task_start_request(
    project: &str,
    writer: &WriterIdentity,
    title: &str,
    prompt: &str,
    thread_key: &str,
    git_head: Option<&str>,
) -> CaptureTaskRequest {
    let canonical_text =
        build_task_start_canonical_text(project, title, prompt, thread_key, git_head);
    CaptureTaskRequest {
        project: project.to_string(),
        task_title: format!("Task started: {}", title.trim()),
        user_prompt: prompt.trim().to_string(),
        writer_id: writer.id.clone(),
        writer_name: writer.name.clone(),
        agent_summary: format!("Started direct no-plan task: {}", title.trim()),
        files_changed: Vec::new(),
        git_diff_summary: git_head.map(|head| format!("Task started from git HEAD {head}")),
        tests: Vec::new(),
        notes: Vec::new(),
        structured_candidates: vec![mem_api::CaptureCandidateInput {
            canonical_text,
            summary: title.trim().to_string(),
            memory_type: mem_api::MemoryType::Task,
            confidence: 0.95,
            importance: 3,
            tags: vec![
                "task".to_string(),
                format!("task-thread:{thread_key}"),
                "direct-execution".to_string(),
                "no-approved-plan".to_string(),
            ],
            sources: vec![
                mem_api::CaptureCandidateSourceInput {
                    file_path: None,
                    symbol_name: None,
                    symbol_kind: None,
                    source_kind: mem_api::SourceKind::TaskPrompt,
                    excerpt: Some(prompt.trim().to_string()),
                },
                mem_api::CaptureCandidateSourceInput {
                    file_path: None,
                    symbol_name: None,
                    symbol_kind: None,
                    source_kind: mem_api::SourceKind::Note,
                    excerpt: Some("Direct no-plan task entered execution.".to_string()),
                },
            ],
        }],
        command_output: None,
        idempotency_key: Some(build_task_start_idempotency_key(
            project, thread_key, title, prompt, git_head,
        )),
        dry_run: false,
    }
}

pub(crate) async fn verify_task_start_memory(
    api: &ApiClient,
    project: &str,
    thread_key: &str,
) -> Result<mem_api::ProjectMemoryListItem> {
    let thread_tag = format!("task-thread:{thread_key}");
    let memories = api.project_memories(project).await?;
    memories
        .items
        .into_iter()
        .find(|item| {
            item.status == mem_api::MemoryStatus::Active
                && item.memory_type == mem_api::MemoryType::Task
                && item.tags.iter().any(|tag| tag == &thread_tag)
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "task start capture was written, but no active `task` memory with tag `{thread_tag}` exists. Run `memory curate --project {project}` or review queued replacement proposals, then retry."
            )
        })
}

pub(crate) fn implementation_sources(
    prompt: &str,
    notes: &[String],
    files_changed: &[String],
    tests: &[TestResult],
    command_output: Option<&str>,
) -> Vec<mem_api::CaptureCandidateSourceInput> {
    let mut sources = Vec::new();
    if !prompt.trim().is_empty() {
        sources.push(mem_api::CaptureCandidateSourceInput {
            file_path: None,
            symbol_name: None,
            symbol_kind: None,
            source_kind: mem_api::SourceKind::TaskPrompt,
            excerpt: Some(prompt.trim().to_string()),
        });
    }
    for note in notes {
        let trimmed = note.trim();
        if trimmed.is_empty() {
            continue;
        }
        sources.push(mem_api::CaptureCandidateSourceInput {
            file_path: None,
            symbol_name: None,
            symbol_kind: None,
            source_kind: mem_api::SourceKind::Note,
            excerpt: Some(trimmed.to_string()),
        });
    }
    for file in files_changed {
        sources.push(mem_api::CaptureCandidateSourceInput {
            file_path: Some(file.clone()),
            symbol_name: None,
            symbol_kind: None,
            source_kind: mem_api::SourceKind::File,
            excerpt: Some(format!("Changed file during task: {file}")),
        });
    }
    for test in tests {
        sources.push(mem_api::CaptureCandidateSourceInput {
            file_path: None,
            symbol_name: None,
            symbol_kind: None,
            source_kind: mem_api::SourceKind::Test,
            excerpt: Some(format!("{}: {}", test.command, test.status)),
        });
    }
    if let Some(output) = command_output
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        sources.push(mem_api::CaptureCandidateSourceInput {
            file_path: None,
            symbol_name: None,
            symbol_kind: None,
            source_kind: mem_api::SourceKind::CommandOutput,
            excerpt: Some(output.to_string()),
        });
    }
    sources
}

pub(crate) fn normalize_sentence_fragment(input: &str) -> String {
    let mut value = input.trim().replace('\n', " ");
    while value.contains("  ") {
        value = value.replace("  ", " ");
    }
    if value.is_empty() {
        return value;
    }
    if !value.ends_with('.') {
        value.push('.');
    }
    value
}

pub(crate) fn is_refactor_completion(
    title: &str,
    summary: &str,
    prompt: &str,
    notes: &[String],
    completed_items: &[String],
) -> bool {
    let mut haystack = format!(
        "{} {} {}",
        title.to_ascii_lowercase(),
        summary.to_ascii_lowercase(),
        prompt.to_ascii_lowercase()
    );
    for note in notes {
        haystack.push(' ');
        haystack.push_str(&note.to_ascii_lowercase());
    }
    for item in completed_items {
        haystack.push(' ');
        haystack.push_str(&item.to_ascii_lowercase());
    }

    let has_refactor_cue = [
        "refactor",
        "refactored",
        "refactoring",
        "restructure",
        "restructured",
        "reorganize",
        "reorganized",
        "rename",
        "renamed",
        "move",
        "moved",
        "extract helper",
        "extracted helper",
        "cleanup",
        "clean up",
        "mechanical change",
    ]
    .iter()
    .any(|cue| haystack.contains(cue));
    if !has_refactor_cue {
        return false;
    }

    let behavior_preserving = [
        "no functional change",
        "no behavior change",
        "without functional change",
        "without behavior change",
        "behavior preserving",
        "behaviour preserving",
        "behavior-preserving",
        "behaviour-preserving",
        "pure refactor",
    ]
    .iter()
    .any(|cue| haystack.contains(cue));

    let functional_change = [
        "fix",
        "fixed",
        "bug",
        "feature",
        "implemented",
        "add support",
        "added support",
        "new behavior",
        "new behaviour",
    ]
    .iter()
    .any(|cue| haystack.contains(cue));

    behavior_preserving || !functional_change
}

pub(crate) fn build_implementation_canonical_text(
    title: &str,
    summary: &str,
    implemented_items: &[String],
    notes: &[String],
) -> String {
    let mut sections = vec![normalize_sentence_fragment(summary)];
    if !title.trim().is_empty() {
        sections.push(format!("Plan: {}.", title.trim()));
    }
    if !implemented_items.is_empty() {
        sections.push(format!(
            "Implemented items:\n{}",
            implemented_items
                .iter()
                .map(|item| format!("- {}", item.trim()))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    let cleaned_notes = notes
        .iter()
        .map(|note| note.trim())
        .filter(|note| !note.is_empty())
        .collect::<Vec<_>>();
    if !cleaned_notes.is_empty() {
        sections.push(format!(
            "Implementation notes:\n{}",
            cleaned_notes
                .iter()
                .map(|note| format!("- {note}"))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    sections.join("\n\n")
}

pub(crate) fn build_remember_implementation_candidate(
    title: &str,
    summary: &str,
    prompt: &str,
    notes: &[String],
    files_changed: &[String],
    tests: &[TestResult],
    command_output: Option<&str>,
) -> mem_api::CaptureCandidateInput {
    let canonical_text = build_implementation_canonical_text("", summary, &[], notes);
    let memory_type = if is_refactor_completion(title, summary, prompt, notes, &[]) {
        mem_api::MemoryType::Refactor
    } else {
        mem_api::MemoryType::Implementation
    };
    let mut tags = match memory_type {
        mem_api::MemoryType::Refactor => vec!["refactor".to_string(), "refactored".to_string()],
        _ => vec!["implementation".to_string(), "implemented".to_string()],
    };
    for file in files_changed {
        if let Some(prefix) = file.split('/').next().filter(|prefix| !prefix.is_empty()) {
            tags.push(prefix.to_string());
        }
    }
    tags.sort();
    tags.dedup();

    mem_api::CaptureCandidateInput {
        canonical_text,
        summary: summary.trim().to_string(),
        memory_type,
        confidence: if tests.iter().any(|test| test.status == "passed") {
            0.9
        } else {
            0.8
        },
        importance: if !tests.is_empty() || !files_changed.is_empty() {
            3
        } else {
            2
        },
        tags,
        sources: implementation_sources(prompt, notes, files_changed, tests, command_output),
    }
}

pub(crate) fn derive_finish_execution_implementation_summary(
    explicit_summary: Option<&str>,
    report: &PlanExecutionFinishReport,
) -> String {
    if let Some(summary) = explicit_summary
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return summary.to_string();
    }
    match report.completed_items {
        0 => format!("Completed {}", report.plan_title),
        1 => report
            .completed_item_texts
            .first()
            .cloned()
            .unwrap_or_else(|| format!("Completed {}", report.plan_title)),
        _ => format!(
            "Implemented {} items for {}",
            report.completed_items, report.plan_title
        ),
    }
}

pub(crate) fn build_finish_execution_implementation_idempotency_key(
    project: &str,
    report: &PlanExecutionFinishReport,
    summary: &str,
    notes: &[String],
    git_head: Option<&str>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"implementation-finish");
    hasher.update(project.as_bytes());
    hasher.update(report.thread_key.as_bytes());
    hasher.update(report.plan_title.as_bytes());
    hasher.update(summary.as_bytes());
    for item in &report.completed_item_texts {
        hasher.update(item.as_bytes());
    }
    for note in notes {
        hasher.update(note.trim().as_bytes());
    }
    if let Some(git_head) = git_head.map(str::trim).filter(|value| !value.is_empty()) {
        hasher.update(git_head.as_bytes());
    }
    format!("implementation-finish:{:x}", hasher.finalize())
}

pub(crate) fn build_finish_execution_implementation_request(
    project: &str,
    writer: &WriterIdentity,
    report: &PlanExecutionFinishReport,
    summary: &str,
    notes: &[String],
    git_head: Option<&str>,
) -> CaptureTaskRequest {
    let canonical_text = build_implementation_canonical_text(
        &report.plan_title,
        summary,
        &report.completed_item_texts,
        notes,
    );
    let memory_type = if is_refactor_completion(
        &report.plan_title,
        summary,
        &report.plan_title,
        notes,
        &report.completed_item_texts,
    ) {
        mem_api::MemoryType::Refactor
    } else {
        mem_api::MemoryType::Implementation
    };
    let mut tags = match memory_type {
        mem_api::MemoryType::Refactor => vec!["refactor".to_string(), "refactored".to_string()],
        _ => vec!["implementation".to_string(), "implemented".to_string()],
    };
    tags.push(format!("plan-thread:{}", report.thread_key));
    tags.sort();
    tags.dedup();

    CaptureTaskRequest {
        project: project.to_string(),
        task_title: format!("Implemented: {}", report.plan_title),
        user_prompt: format!(
            "Verified completed implementation for plan {} in project {}.",
            report.plan_title, project
        ),
        writer_id: writer.id.clone(),
        writer_name: writer.name.clone(),
        agent_summary: summary.to_string(),
        files_changed: Vec::new(),
        git_diff_summary: git_head
            .map(|head| format!("Implementation verified from git HEAD {head}")),
        tests: Vec::new(),
        notes: Vec::new(),
        structured_candidates: vec![mem_api::CaptureCandidateInput {
            canonical_text: canonical_text.clone(),
            summary: summary.to_string(),
            memory_type,
            confidence: 0.95,
            importance: 4,
            tags,
            sources: implementation_sources(
                &format!(
                    "Verified completed implementation for plan {} in project {}.",
                    report.plan_title, project
                ),
                notes,
                &[],
                &[],
                None,
            ),
        }],
        command_output: None,
        idempotency_key: Some(build_finish_execution_implementation_idempotency_key(
            project, report, summary, notes, git_head,
        )),
        dry_run: false,
    }
}

pub(crate) fn build_plan_execution_finish_report(
    project: &str,
    detail: &mem_api::MemoryEntryResponse,
) -> Result<PlanExecutionFinishReport> {
    let items = parse_plan_checkboxes(&detail.canonical_text);
    ensure_checkbox_plan(&items)?;
    let completed_items = items.iter().filter(|item| item.checked).count();
    let completed_item_texts = items
        .iter()
        .filter(|item| item.checked)
        .map(|item| item.text.clone())
        .collect::<Vec<_>>();
    let remaining_items = items
        .iter()
        .filter(|item| !item.checked)
        .map(|item| item.text.clone())
        .collect::<Vec<_>>();
    let thread_key = extract_plan_thread_key(&detail.tags)
        .ok_or_else(|| anyhow::anyhow!("active plan is missing a `plan-thread:` tag"))?
        .to_string();

    Ok(PlanExecutionFinishReport {
        project: project.to_string(),
        thread_key,
        plan_title: detail.summary.clone(),
        total_items: items.len(),
        completed_items,
        completed_item_texts,
        verified_complete: remaining_items.is_empty(),
        remaining_items,
    })
}

pub(crate) fn plan_detail_from_markdown(
    selection: &ActivePlanSelection,
    markdown: &str,
    memory_id: Uuid,
) -> Result<mem_api::MemoryEntryResponse> {
    let items = parse_plan_checkboxes(markdown);
    ensure_checkbox_plan(&items)?;
    Ok(mem_api::MemoryEntryResponse {
        id: memory_id,
        canonical_text: normalize_plan_markdown_for_hash(markdown),
        summary: selection.title.clone(),
        memory_type: mem_api::MemoryType::Plan,
        importance: 4,
        confidence: 0.95,
        status: mem_api::MemoryStatus::Active,
        tags: vec![
            "plan".to_string(),
            format!("plan-thread:{}", selection.thread_key),
        ],
        sources: Vec::new(),
        related_memories: Vec::new(),
        embedding_spaces: Vec::new(),
        project: String::new(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        canonical_id: memory_id,
        version_no: 1,
        is_tombstone: false,
    })
}

pub(crate) fn derive_summary(project: &str, files_changed: &[String]) -> String {
    if files_changed.is_empty() {
        format!("Captured meaningful work for project {project}.")
    } else {
        let preview = files_changed
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        format!("Updated files in project {project}: {preview}.")
    }
}

pub(crate) fn detect_changed_files() -> Result<Vec<String>> {
    let inside_repo = ProcessCommand::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .output();

    let Ok(output) = inside_repo else {
        return Ok(Vec::new());
    };
    if !output.status.success() {
        return Ok(Vec::new());
    }

    let output = ProcessCommand::new("git")
        .args(["status", "--porcelain"])
        .output()
        .context("run git status --porcelain")?;
    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8(output.stdout).context("decode git status output")?;
    let mut files = Vec::new();
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
        if !files.contains(&normalized) {
            files.push(normalized);
        }
    }
    Ok(files)
}

pub(crate) fn repo_git_head(repo_root: &Path) -> Option<String> {
    let output = ProcessCommand::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;
    let head = stdout.trim();
    if head.is_empty() {
        None
    } else {
        Some(head.to_string())
    }
}

pub(crate) trait SourceKindString {
    fn source_kind_string(&self) -> &'static str;
}

impl SourceKindString for mem_api::SourceKind {
    fn source_kind_string(&self) -> &'static str {
        match self {
            mem_api::SourceKind::TaskPrompt => "task_prompt",
            mem_api::SourceKind::File => "file",
            mem_api::SourceKind::GitCommit => "git_commit",
            mem_api::SourceKind::CommandOutput => "command_output",
            mem_api::SourceKind::Test => "test",
            mem_api::SourceKind::Note => "note",
        }
    }
}
