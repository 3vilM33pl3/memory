use std::collections::BTreeMap;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, Stdio},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use chrono::Utc;
use mem_api::{
    AppConfig, MemoryType, QueryAnswerCitation, QueryAnswerGeneration, QueryAnswerMethod,
    QueryDiagnostics, QueryFilters, QueryMatchKind, QueryRequest, QueryResponse, QueryResult,
    QueryResultDebug, QuerySource, SourceKind, TokenUsage, UpToSpeedRequest,
    effective_llm_base_url, is_supported_llm_provider, llm_max_output_tokens_field,
    llm_requires_api_key, resolve_llm_api_key,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::commands::{
    api::ApiClient,
    memory_ops::resolve_project_slug,
    output::service_url,
    runtime::{EvalArgs, EvalCommand},
};

pub(super) async fn handle_eval_command(args: EvalArgs, cwd: &Path, api: &ApiClient) -> Result<()> {
    match args.command {
        EvalCommand::Doctor(args) => {
            let suite = mem_eval::load_suite(&args.suite)?;
            let mut checks = Vec::new();
            checks.push(serde_json::json!({
                "name": "suite.load",
                "status": "ok",
                "message": format!("Loaded {} item(s).", suite.items.len()),
            }));
            let checksum = mem_eval::suite_checksum(&suite)?;
            checks.push(serde_json::json!({
                "name": "suite.checksum",
                "status": "ok",
                "message": checksum,
            }));
            let shell_required = mem_eval::suite_requires_shell(&suite);
            checks.push(serde_json::json!({
                "name": "suite.shell",
                "status": if shell_required { "warn" } else { "ok" },
                "message": if shell_required {
                    "suite contains shell-executing items; eval run requires --allow-shell unless --dry-run is used"
                } else {
                    "suite has no shell-executing items"
                },
            }));
            let reviewed = suite.manifest.label_status.as_deref() == Some("reviewed");
            checks.push(serde_json::json!({
                "name": "suite.labels",
                "status": if reviewed { "ok" } else { "warn" },
                "message": suite.manifest.label_status.as_deref().unwrap_or("unreviewed"),
            }));
            if let Some(min_items) = suite.manifest.min_items {
                checks.push(serde_json::json!({
                    "name": "suite.min_items",
                    "status": if suite.items.len() >= min_items { "ok" } else { "fail" },
                    "message": format!("{} item(s), required {}", suite.items.len(), min_items),
                }));
            }
            for item in &suite.items {
                match item {
                    mem_eval::EvalItem::AgentBuildTask(item) => {
                        let result = validate_agent_build_suite_item(&suite, item);
                        checks.push(serde_json::json!({
                            "name": format!("agent_build_task.{}", item.id),
                            "status": if result.is_ok() { "ok" } else { "fail" },
                            "message": result.err().map(|error| error.to_string()).unwrap_or_else(|| "fixture and paths are valid".to_string()),
                        }));
                    }
                    mem_eval::EvalItem::AgentBuildSequence(item) => {
                        let result = validate_agent_build_sequence_suite_item(&suite, item);
                        checks.push(serde_json::json!({
                            "name": format!("agent_build_sequence.{}", item.id),
                            "status": if result.is_ok() { "ok" } else { "fail" },
                            "message": result.err().map(|error| error.to_string()).unwrap_or_else(|| format!("fixture, paths, and {} steps are valid", item.steps.len())),
                        }));
                    }
                    _ => {}
                }
            }
            match api.health().await {
                Ok(value) => checks.push(serde_json::json!({
                    "name": "backend.health",
                    "status": "ok",
                    "message": value,
                })),
                Err(error) => checks.push(serde_json::json!({
                    "name": "backend.health",
                    "status": "fail",
                    "message": error.to_string(),
                })),
            }
            let failed = checks
                .iter()
                .any(|check| check.get("status").and_then(|value| value.as_str()) == Some("fail"));
            let payload = serde_json::json!({
                "ok": !failed,
                "suite": suite.manifest.name,
                "checks": checks,
            });
            if args.text {
                println!(
                    "{}: {}",
                    payload["suite"].as_str().unwrap_or("suite"),
                    if failed { "fail" } else { "ok" }
                );
                for check in payload["checks"].as_array().into_iter().flatten() {
                    println!(
                        "{} [{}] {}",
                        check["name"].as_str().unwrap_or("?"),
                        check["status"].as_str().unwrap_or("?"),
                        check["message"]
                    );
                }
            } else {
                println!("{}", serde_json::to_string_pretty(&payload)?);
            }
            if failed {
                anyhow::bail!("eval doctor failed");
            }
        }
        EvalCommand::Scaffold(args) => {
            let project = resolve_project_slug(args.project, cwd)?;
            let response = api.project_memories(&project).await?;
            let selected = response
                .items
                .into_iter()
                .take(args.limit.clamp(1, 100))
                .collect::<Vec<_>>();
            let manifest = format!(
                "name = \"{} starter eval\"\nproject = \"{}\"\nitems = \"items.jsonl\"\n",
                project, project
            );
            let mut lines = Vec::new();
            for item in selected {
                lines.push(serde_json::json!({
                    "eval_type": "retrieval_qa",
                    "id": format!("memory-{}", item.id),
                    "project": project,
                    "question": format!("What should an agent know about {}?", item.summary),
                    "top_k": 8,
                    "expected_memory_ids": [item.id],
                    "expected_tags": item.tags,
                }));
            }
            if args.dry_run {
                let payload = serde_json::json!({
                    "dry_run": true,
                    "out": args.out,
                    "suite_toml": manifest,
                    "items": lines,
                });
                if args.text {
                    println!(
                        "Would write starter eval suite with {} item(s) to {}",
                        lines.len(),
                        payload["out"].as_str().unwrap_or("<path>")
                    );
                } else {
                    println!("{}", serde_json::to_string_pretty(&payload)?);
                }
                return Ok(());
            }
            fs::create_dir_all(&args.out)
                .with_context(|| format!("create {}", args.out.display()))?;
            fs::write(args.out.join("suite.toml"), manifest)
                .with_context(|| format!("write {}", args.out.join("suite.toml").display()))?;
            let jsonl = lines
                .iter()
                .map(serde_json::to_string)
                .collect::<Result<Vec<_>, _>>()?
                .join("\n");
            fs::write(args.out.join("items.jsonl"), format!("{jsonl}\n"))
                .with_context(|| format!("write {}", args.out.join("items.jsonl").display()))?;
            let payload = serde_json::json!({
                "dry_run": false,
                "out": args.out,
                "items": lines.len(),
            });
            if args.text {
                println!(
                    "Wrote starter eval suite with {} item(s) to {}",
                    payload["items"].as_u64().unwrap_or_default(),
                    payload["out"].as_str().unwrap_or("<path>")
                );
            } else {
                println!("{}", serde_json::to_string_pretty(&payload)?);
            }
        }
        EvalCommand::Run(args) => {
            let suite = mem_eval::load_suite(&args.suite)?;
            ensure_eval_shell_allowed(&suite, args.allow_shell, args.dry_run)?;
            ensure_eval_retriever_allowed(args.retriever_cmd.as_deref(), args.allow_shell)?;
            if args.fail_on_unreviewed_labels
                && suite.manifest.label_status.as_deref() != Some("reviewed")
            {
                anyhow::bail!(
                    "suite labels are not reviewed; set label_status = \"reviewed\" or omit --fail-on-unreviewed-labels"
                );
            }
            let project = match suite.manifest.project.clone() {
                Some(project) => project,
                None => resolve_project_slug(None, cwd)?,
            };
            let profile = args.profile.parse::<mem_eval::EvalProfile>()?;
            let conditions = args
                .conditions
                .iter()
                .map(|value| value.parse::<mem_eval::EvalCondition>())
                .collect::<Result<Vec<_>>>()?;
            let mut runs = Vec::new();
            let repeat = args
                .repeat
                .max(1)
                .max(suite.manifest.default_repeats.unwrap_or(1));
            let run_group_id = uuid::Uuid::new_v4();
            let suite_checksum = mem_eval::suite_checksum(&suite).ok();
            let mut total_tokens = 0u64;
            for repeat_index in 0..repeat {
                for condition in &conditions {
                    let context = EvalRunContext {
                        profile,
                        repeat_index,
                        run_group_id,
                        suite_checksum: suite_checksum.clone(),
                        dry_run: args.dry_run,
                        artifacts_root: args.out.clone(),
                        memory_command: eval_memory_command(),
                        memory_base_url: service_url(&api.config, ""),
                        memory_config_path: eval_memory_config_path(cwd),
                        llm_judge: args.llm_judge,
                        retriever_cmd: args.retriever_cmd.clone(),
                        command_cwd: cwd.to_path_buf(),
                    };
                    let run = run_eval_suite(&suite, &project, *condition, context, api).await?;
                    total_tokens += run
                        .results
                        .iter()
                        .filter_map(|result| result.token_usage.as_ref())
                        .map(|usage| usage.total_tokens)
                        .sum::<u64>();
                    if let Some(max_cost) = args.max_cost
                        && total_tokens > max_cost
                    {
                        anyhow::bail!(
                            "eval token budget exceeded: used {} tokens, limit {}",
                            total_tokens,
                            max_cost
                        );
                    }
                    let filename = format!(
                        "{}-{}-r{}-{}.json",
                        sanitize_filename(&suite.manifest.name),
                        condition,
                        repeat_index,
                        Utc::now().format("%Y%m%d%H%M%S")
                    );
                    let path = args.out.join(filename);
                    mem_eval::write_json(&path, &run)?;
                    runs.push(serde_json::json!({
                    "condition": condition,
                    "profile": profile,
                    "repeat_index": repeat_index,
                    "run_group_id": run_group_id,
                    "path": path,
                    "items": run.results.len(),
                    "successes": run.results.iter().filter(|result| result.success).count(),
                    "skipped": run.results.iter().filter(|result| result.skipped).count(),
                    "tokens": run.results.iter().filter_map(|result| result.token_usage.as_ref()).map(|usage| usage.total_tokens).sum::<u64>(),
                }));
                }
            }
            let payload = serde_json::json!({
                "run_group_id": run_group_id,
                "profile": profile,
                "repeat": repeat,
                "write_transcripts": args.write_transcripts,
                "llm_judge": args.llm_judge,
                "allow_shell": args.allow_shell,
                "shell_required": mem_eval::suite_requires_shell(&suite),
                "total_tokens": total_tokens,
                "runs": runs,
            });
            if args.text {
                for run in &payload["runs"].as_array().cloned().unwrap_or_default() {
                    println!(
                        "{} [{} r{}]: {} item(s), {} success, {} skipped, {} tokens -> {}",
                        run["condition"].as_str().unwrap_or("?"),
                        run["profile"].as_str().unwrap_or("?"),
                        run["repeat_index"].as_u64().unwrap_or_default(),
                        run["items"].as_u64().unwrap_or_default(),
                        run["successes"].as_u64().unwrap_or_default(),
                        run["skipped"].as_u64().unwrap_or_default(),
                        run["tokens"].as_u64().unwrap_or_default(),
                        run["path"].as_str().unwrap_or("<path>")
                    );
                }
            } else {
                println!("{}", serde_json::to_string_pretty(&payload)?);
            }
        }
        EvalCommand::Compare(args) => {
            let baseline = load_eval_runs_from_patterns(&args.baseline)?;
            let candidate = load_eval_runs_from_patterns(&args.candidate)?;
            let comparison = mem_eval::compare_run_sets(&baseline, &candidate);
            if let Some(path) = args.out {
                mem_eval::write_json(&path, &comparison)?;
            }
            if args.text {
                println!("{}", mem_eval::comparison_text(&comparison));
            } else {
                println!("{}", serde_json::to_string_pretty(&comparison)?);
            }
        }
        EvalCommand::Report(args) => {
            let comparison: mem_eval::EvalComparison = serde_json::from_str(
                &fs::read_to_string(&args.comparison)
                    .with_context(|| format!("read {}", args.comparison.display()))?,
            )
            .with_context(|| format!("parse {}", args.comparison.display()))?;
            let rendered = if args.markdown {
                mem_eval::comparison_markdown(&comparison)
            } else if args.text {
                mem_eval::comparison_text(&comparison)
            } else {
                serde_json::to_string_pretty(&comparison)?
            };
            if let Some(path) = args.out {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("create {}", parent.display()))?;
                }
                fs::write(&path, rendered).with_context(|| format!("write {}", path.display()))?;
            } else {
                println!("{rendered}");
            }
        }
        EvalCommand::Gate(args) => {
            let comparison: mem_eval::EvalComparison = serde_json::from_str(
                &fs::read_to_string(&args.comparison)
                    .with_context(|| format!("read {}", args.comparison.display()))?,
            )
            .with_context(|| format!("parse {}", args.comparison.display()))?;
            let policy: mem_eval::EvalGatePolicy = toml::from_str(
                &fs::read_to_string(&args.policy)
                    .with_context(|| format!("read {}", args.policy.display()))?,
            )
            .with_context(|| format!("parse {}", args.policy.display()))?;
            let result = mem_eval::evaluate_gate(&comparison, &policy);
            if args.text {
                println!("gate: {}", if result.passed { "pass" } else { "fail" });
                for reason in &result.reasons {
                    println!("- {reason}");
                }
            } else {
                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            if !result.passed {
                anyhow::bail!("eval gate failed");
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub(crate) struct EvalRunContext {
    pub(crate) profile: mem_eval::EvalProfile,
    pub(crate) repeat_index: usize,
    pub(crate) run_group_id: uuid::Uuid,
    pub(crate) suite_checksum: Option<String>,
    pub(crate) dry_run: bool,
    pub(crate) artifacts_root: PathBuf,
    pub(crate) memory_command: String,
    pub(crate) memory_base_url: String,
    pub(crate) memory_config_path: Option<PathBuf>,
    pub(crate) llm_judge: bool,
    pub(crate) retriever_cmd: Option<String>,
    pub(crate) command_cwd: PathBuf,
}

pub(crate) fn ensure_eval_shell_allowed(
    suite: &mem_eval::EvalSuite,
    allow_shell: bool,
    dry_run: bool,
) -> Result<()> {
    if dry_run || allow_shell || !mem_eval::suite_requires_shell(suite) {
        return Ok(());
    }
    anyhow::bail!(
        "eval suite `{}` contains shell-executing items (command_task, agent_build_task, or agent_build_sequence). Review the suite files, then rerun with --allow-shell to execute them. Use --dry-run to validate parsing without shell execution.",
        suite.manifest.name
    )
}

pub(crate) fn ensure_eval_retriever_allowed(
    retriever_cmd: Option<&str>,
    allow_shell: bool,
) -> Result<()> {
    if retriever_cmd.is_none_or(|command| command.trim().is_empty()) || allow_shell {
        return Ok(());
    }
    anyhow::bail!(
        "--retriever-cmd executes an external command. Review the executable, then rerun with --allow-shell to opt in."
    )
}

pub(crate) async fn run_eval_suite(
    suite: &mem_eval::EvalSuite,
    default_project: &str,
    condition: mem_eval::EvalCondition,
    context: EvalRunContext,
    api: &ApiClient,
) -> Result<mem_eval::EvalRun> {
    let mut results = Vec::new();
    for item in &suite.items {
        if context.dry_run
            && !matches!(
                item,
                mem_eval::EvalItem::AgentBuildTask(_) | mem_eval::EvalItem::AgentBuildSequence(_)
            )
        {
            results.push(mem_eval::skipped_result(
                item,
                condition,
                "dry-run: execution skipped",
            ));
            continue;
        }
        let project = item.project(default_project);
        let mut result = match item {
            mem_eval::EvalItem::RetrievalQa(item) => {
                if condition == mem_eval::EvalCondition::NoMemory {
                    no_memory_retrieval_result(item, condition)
                } else if context.retriever_cmd.is_some() {
                    match run_external_retriever_for_retrieval_item(
                        suite, item, project, condition, &context,
                    ) {
                        Ok(response) => mem_eval::score_retrieval_qa(item, condition, &response),
                        Err(error) => external_retriever_failure_result(
                            item.id.clone(),
                            "retrieval_qa",
                            item.metadata.clone(),
                            condition,
                            error,
                        ),
                    }
                } else {
                    let response = api
                        .query(&QueryRequest {
                            project: project.to_string(),
                            query: item.question.clone(),
                            filters: QueryFilters::default(),
                            top_k: item.top_k,
                            min_confidence: None,
                            include_stale: false,
                            history: false,
                            retrieval_mode: Some(eval_condition_retrieval_mode(condition)),
                            answer_mode: Some(mem_api::QueryAnswerMode::Deterministic),
                        })
                        .await?;
                    mem_eval::score_retrieval_qa(item, condition, &response)
                }
            }
            mem_eval::EvalItem::GroundedAnswer(item) => {
                if condition == mem_eval::EvalCondition::NoMemory {
                    if context.profile == mem_eval::EvalProfile::Offline {
                        offline_no_memory_grounded_answer_eval_item(item, condition)
                    } else {
                        run_no_memory_grounded_answer_eval_item(api, item, condition).await?
                    }
                } else if context.retriever_cmd.is_some() {
                    match run_external_retriever_for_grounded_answer_item(
                        suite, item, project, condition, &context,
                    ) {
                        Ok(response) => mem_eval::score_grounded_answer(item, condition, &response),
                        Err(error) => external_retriever_failure_result(
                            item.id.clone(),
                            "grounded_answer",
                            item.metadata.clone(),
                            condition,
                            error,
                        ),
                    }
                } else {
                    let response = api
                        .query(&QueryRequest {
                            project: project.to_string(),
                            query: item.question.clone(),
                            filters: QueryFilters::default(),
                            top_k: item.top_k,
                            min_confidence: None,
                            include_stale: false,
                            history: false,
                            retrieval_mode: Some(eval_condition_retrieval_mode(condition)),
                            answer_mode: Some(match context.profile {
                                mem_eval::EvalProfile::Llm => mem_api::QueryAnswerMode::Llm,
                                mem_eval::EvalProfile::Offline => {
                                    mem_api::QueryAnswerMode::Deterministic
                                }
                            }),
                        })
                        .await?;
                    mem_eval::score_grounded_answer(item, condition, &response)
                }
            }
            mem_eval::EvalItem::ResumeQuality(item) => {
                if condition == mem_eval::EvalCondition::NoMemory {
                    if context.profile == mem_eval::EvalProfile::Offline {
                        offline_no_memory_resume_quality_eval_item(item, condition)
                    } else {
                        run_no_memory_resume_quality_eval_item(api, item, condition).await?
                    }
                } else {
                    let response = api
                        .up_to_speed(&UpToSpeedRequest {
                            project: project.to_string(),
                            include_llm_summary: false,
                            limit: 20,
                        })
                        .await?;
                    mem_eval::score_up_to_speed_quality(item, condition, &response)
                }
            }
            mem_eval::EvalItem::CommandTask(item) => run_command_eval_item(item, condition)?,
            mem_eval::EvalItem::AgentBuildTask(item) => {
                run_agent_build_eval_item(suite, item, condition, &context)?
            }
            mem_eval::EvalItem::AgentBuildSequence(item) => {
                run_agent_build_sequence_eval_item(suite, item, condition, &context)?
            }
        };
        if matches!(
            condition,
            mem_eval::EvalCondition::Lexical
                | mem_eval::EvalCondition::Semantic
                | mem_eval::EvalCondition::Graph
        ) {
            result
                .notes
                .push("retrieval mode was explicitly requested for eval isolation".to_string());
        }
        if context.llm_judge && context.profile == mem_eval::EvalProfile::Llm {
            add_llm_judge_scores(api, item, &mut result).await?;
        }
        results.push(result);
    }
    Ok(mem_eval::EvalRun {
        suite: suite.manifest.name.clone(),
        project: default_project.to_string(),
        condition,
        profile: context.profile,
        run_group_id: context.run_group_id,
        repeat_index: context.repeat_index,
        suite_checksum: context.suite_checksum,
        fixture_checksum: suite.manifest.fixture.clone(),
        config_fingerprint: None,
        dry_run: context.dry_run,
        created_at: Utc::now(),
        git_head: git_head(),
        service_version: None,
        results,
    })
}

pub(crate) fn load_eval_runs_from_patterns(patterns: &[PathBuf]) -> Result<Vec<mem_eval::EvalRun>> {
    if patterns.is_empty() {
        anyhow::bail!("at least one eval run path is required");
    }
    let mut paths = Vec::new();
    for pattern in patterns {
        let pattern_text = pattern.to_string_lossy();
        if pattern_text.contains('*') || pattern_text.contains('?') {
            paths.extend(expand_eval_run_pattern(pattern)?);
        } else {
            paths.push(pattern.clone());
        }
    }
    paths.sort();
    paths.dedup();
    if paths.is_empty() {
        anyhow::bail!("eval run pattern(s) matched no files");
    }
    paths
        .iter()
        .map(|path| mem_eval::load_run(path))
        .collect::<Result<Vec<_>>>()
}

pub(crate) fn expand_eval_run_pattern(pattern: &Path) -> Result<Vec<PathBuf>> {
    let file_pattern = pattern
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow::anyhow!("invalid eval run glob `{}`", pattern.display()))?;
    let dir = pattern
        .parent()
        .filter(|value| !value.as_os_str().is_empty());
    let dir = dir.unwrap_or_else(|| Path::new("."));
    let mut matches = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry.with_context(|| format!("read entry in {}", dir.display()))?;
        let path = entry.path();
        if path.is_file()
            && path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| wildcard_match(file_pattern, name))
        {
            matches.push(path);
        }
    }
    Ok(matches)
}

pub(crate) fn wildcard_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let (mut p, mut v) = (0usize, 0usize);
    let mut star = None;
    let mut star_value = 0usize;
    while v < value.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == value[v]) {
            p += 1;
            v += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            star_value = v;
        } else if let Some(star_index) = star {
            p = star_index + 1;
            star_value += 1;
            v = star_value;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

#[derive(Debug, Serialize)]
pub(crate) struct ExternalRetrieverRequest {
    pub(crate) schema_version: u8,
    pub(crate) project: String,
    pub(crate) query: String,
    pub(crate) limit: i64,
    pub(crate) item_id: String,
    pub(crate) condition: String,
    pub(crate) context: ExternalRetrieverContext,
}

#[derive(Debug, Serialize)]
pub(crate) struct ExternalRetrieverContext {
    pub(crate) fixture_path: String,
    pub(crate) hidden_facts: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ExternalRetrieverResponse {
    pub(crate) schema_version: u8,
    #[serde(default)]
    pub(crate) results: Vec<ExternalRetrieverResult>,
    #[serde(default)]
    pub(crate) answer: Option<String>,
    #[serde(default)]
    pub(crate) confidence: Option<f32>,
    #[serde(default)]
    pub(crate) diagnostics: ExternalRetrieverDiagnostics,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ExternalRetrieverResult {
    pub(crate) id: String,
    pub(crate) score: f64,
    pub(crate) text: String,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    #[serde(default)]
    pub(crate) citations: Vec<ExternalRetrieverCitation>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ExternalRetrieverDiagnostics {
    #[serde(default)]
    pub(crate) latency_ms: Option<u64>,
    #[serde(default)]
    pub(crate) tokens_in: Option<u64>,
    #[serde(default)]
    pub(crate) tokens_out: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum ExternalRetrieverCitation {
    Path(String),
    Object {
        #[serde(default)]
        file_path: Option<String>,
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        excerpt: Option<String>,
    },
}

pub(crate) fn run_external_retriever_for_retrieval_item(
    suite: &mem_eval::EvalSuite,
    item: &mem_eval::RetrievalQaItem,
    project: &str,
    condition: mem_eval::EvalCondition,
    context: &EvalRunContext,
) -> Result<QueryResponse> {
    let request = build_external_retriever_request(
        suite,
        &item.id,
        project,
        &item.question,
        item.top_k,
        &item.hidden_facts,
        condition,
    );
    run_external_retriever(context, request)
}

pub(crate) fn run_external_retriever_for_grounded_answer_item(
    suite: &mem_eval::EvalSuite,
    item: &mem_eval::GroundedAnswerItem,
    project: &str,
    condition: mem_eval::EvalCondition,
    context: &EvalRunContext,
) -> Result<QueryResponse> {
    let request = build_external_retriever_request(
        suite,
        &item.id,
        project,
        &item.question,
        item.top_k,
        &item.hidden_facts,
        condition,
    );
    run_external_retriever(context, request)
}

pub(crate) fn build_external_retriever_request(
    suite: &mem_eval::EvalSuite,
    item_id: &str,
    project: &str,
    query: &str,
    limit: i64,
    hidden_facts: &[String],
    condition: mem_eval::EvalCondition,
) -> ExternalRetrieverRequest {
    ExternalRetrieverRequest {
        schema_version: 1,
        project: project.to_string(),
        query: query.to_string(),
        limit,
        item_id: item_id.to_string(),
        condition: condition.to_string(),
        context: ExternalRetrieverContext {
            fixture_path: external_retriever_fixture_path(suite),
            hidden_facts: hidden_facts.to_vec(),
        },
    }
}

fn external_retriever_fixture_path(suite: &mem_eval::EvalSuite) -> String {
    let path = suite
        .manifest
        .fixture
        .as_deref()
        .map(|fixture| suite.root.join(fixture))
        .unwrap_or_else(|| suite.root.clone());
    absolute_eval_path(&path).to_string_lossy().into_owned()
}

pub(crate) fn run_external_retriever(
    context: &EvalRunContext,
    request: ExternalRetrieverRequest,
) -> Result<QueryResponse> {
    let command = context
        .retriever_cmd
        .as_deref()
        .filter(|command| !command.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("external retriever command is empty"))?;
    let request_json =
        serde_json::to_vec(&request).context("serialize external retriever request")?;
    let mut command_builder = ProcessCommand::new("sh");
    command_builder
        .arg("-c")
        .arg(command)
        .current_dir(&context.command_cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let started = Instant::now();
    let mut child = command_builder.spawn().with_context(|| {
        format!(
            "run external retriever `{command}` in {}",
            context.command_cwd.display()
        )
    })?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(&request_json)
            .context("write external retriever request")?;
    }
    let timeout = Duration::from_secs(60);
    let mut timed_out = false;
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            let _ = child.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let output = child
        .wait_with_output()
        .with_context(|| format!("collect external retriever `{command}` output"))?;
    if timed_out {
        anyhow::bail!("external retriever `{command}` timed out after 60s");
    }
    if !output.status.success() {
        anyhow::bail!(
            "external retriever `{}` exited with {}; stderr: {}",
            command,
            output
                .status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "signal".to_string()),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let elapsed_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    let response: ExternalRetrieverResponse = serde_json::from_slice(&output.stdout)
        .with_context(|| format!("parse external retriever `{command}` JSON stdout"))?;
    external_retriever_response_to_query_response(response, elapsed_ms)
}

pub(crate) fn external_retriever_response_to_query_response(
    response: ExternalRetrieverResponse,
    elapsed_ms: u64,
) -> Result<QueryResponse> {
    if response.schema_version != 1 {
        anyhow::bail!(
            "unsupported external retriever schema_version {}",
            response.schema_version
        );
    }
    let duration_ms = response.diagnostics.latency_ms.unwrap_or(elapsed_ms);
    let mut results = Vec::new();
    let mut answer_citations = Vec::new();
    for result in response.results {
        let memory_id = external_result_id_to_uuid(&result.id);
        let sources = result
            .citations
            .iter()
            .filter_map(external_citation_to_query_source)
            .collect::<Vec<_>>();
        let query_result = QueryResult {
            memory_id,
            project: None,
            project_name: None,
            repo_root: None,
            summary: result.text.chars().take(120).collect(),
            memory_type: MemoryType::Reference,
            score: result.score,
            snippet: result.text.clone(),
            match_kind: QueryMatchKind::Hybrid,
            score_explanation: vec!["external retriever result".to_string()],
            debug: QueryResultDebug::default(),
            tags: result.tags,
            sources,
            graph_connections: Vec::new(),
            needs_review: false,
        };
        let result_number = results.len() + 1;
        answer_citations.push(QueryAnswerCitation {
            result_number,
            memory_id,
            project: None,
            project_name: None,
            repo_root: None,
            memory_type: MemoryType::Reference,
            summary: query_result.summary.clone(),
            snippet: query_result.snippet.clone(),
        });
        results.push(query_result);
    }
    let answer = response.answer.unwrap_or_else(|| {
        if results.is_empty() {
            "External retriever returned no results.".to_string()
        } else {
            results
                .iter()
                .enumerate()
                .map(|(index, result)| format!("[{}] {}", index + 1, result.snippet))
                .collect::<Vec<_>>()
                .join("\n")
        }
    });
    let token_usage = match (
        response.diagnostics.tokens_in,
        response.diagnostics.tokens_out,
    ) {
        (Some(input_tokens), Some(output_tokens)) => Some(TokenUsage {
            input_tokens,
            output_tokens,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            total_tokens: input_tokens + output_tokens,
        }),
        _ => None,
    };
    Ok(QueryResponse {
        answer,
        confidence: response.confidence.unwrap_or(0.0),
        insufficient_evidence: results.is_empty(),
        answer_generation: QueryAnswerGeneration {
            method: QueryAnswerMethod::Deterministic,
            cited_result_numbers: (1..=results.len()).collect(),
            evidence_count: results.len(),
            duration_ms: 0,
            fallback_reason: None,
            token_usage,
        },
        answer_citations,
        diagnostics: QueryDiagnostics {
            retrieval_mode: mem_api::QueryRetrievalMode::FullMemory,
            lexical_enabled: false,
            semantic_enabled: false,
            graph_enabled: false,
            relation_boost_enabled: false,
            lexical_candidates: 0,
            semantic_candidates: 0,
            merged_candidates: results.len(),
            returned_results: results.len(),
            relation_augmented_candidates: 0,
            graph_candidates: 0,
            graph_augmented_candidates: 0,
            provenance_decayed_candidates: 0,
            provenance_unverified_candidates: 0,
            lexical_duration_ms: 0,
            semantic_duration_ms: 0,
            rerank_duration_ms: 0,
            graph_duration_ms: 0,
            total_duration_ms: duration_ms,
            semantic_status: "external".to_string(),
            graph_status: "external".to_string(),
            provenance_warnings: Vec::new(),
        },
        results,
    })
}

fn external_result_id_to_uuid(id: &str) -> Uuid {
    id.parse::<Uuid>().unwrap_or_else(|_| {
        let digest = Sha256::digest(id.as_bytes());
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&digest[..16]);
        bytes[6] = (bytes[6] & 0x0f) | 0x50;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        Uuid::from_bytes(bytes)
    })
}

fn external_citation_to_query_source(citation: &ExternalRetrieverCitation) -> Option<QuerySource> {
    let (file_path, excerpt) = match citation {
        ExternalRetrieverCitation::Path(path) => (Some(path.clone()), None),
        ExternalRetrieverCitation::Object {
            file_path,
            path,
            excerpt,
        } => (file_path.clone().or_else(|| path.clone()), excerpt.clone()),
    };
    file_path.map(|file_path| QuerySource {
        task_id: None,
        file_path: Some(file_path),
        symbol_name: None,
        symbol_kind: None,
        source_kind: SourceKind::File,
        excerpt,
        provenance: None,
    })
}

pub(crate) fn external_retriever_failure_result(
    item_id: String,
    eval_type: &str,
    metadata: mem_eval::EvalItemMetadata,
    condition: mem_eval::EvalCondition,
    error: anyhow::Error,
) -> mem_eval::EvalItemResult {
    let mut scores = BTreeMap::new();
    scores.insert("external_retriever_success".to_string(), 0.0);
    mem_eval::EvalItemResult {
        item_id,
        eval_type: eval_type.to_string(),
        condition,
        metadata,
        success: false,
        skipped: false,
        scores,
        duration_ms: None,
        token_usage: None,
        answer: None,
        notes: vec![format!("external retriever failed: {error:#}")],
        sub_results: Vec::new(),
    }
}

pub(crate) fn no_memory_retrieval_result(
    item: &mem_eval::RetrievalQaItem,
    condition: mem_eval::EvalCondition,
) -> mem_eval::EvalItemResult {
    let mut scores = std::collections::BTreeMap::new();
    scores.insert("recall_at_k".to_string(), 0.0);
    scores.insert("mrr".to_string(), 0.0);
    scores.insert("ndcg".to_string(), 0.0);
    scores.insert("citation_precision".to_string(), 1.0);
    scores.insert(
        "tag_recall_at_k".to_string(),
        if item.expected_tags.is_empty() {
            1.0
        } else {
            0.0
        },
    );
    scores.insert(
        "file_recall_at_k".to_string(),
        if item.expected_files.is_empty() {
            1.0
        } else {
            0.0
        },
    );
    mem_eval::EvalItemResult {
        item_id: item.id.clone(),
        eval_type: "retrieval_qa".to_string(),
        condition,
        metadata: item.metadata.clone(),
        success: item.expected_memory_ids.is_empty()
            && item.expected_tags.is_empty()
            && item.expected_files.is_empty(),
        skipped: false,
        scores,
        duration_ms: Some(0),
        token_usage: None,
        answer: None,
        notes: vec!["no-memory condition has no memory retrieval channel".to_string()],
        sub_results: Vec::new(),
    }
}

pub(crate) fn eval_condition_retrieval_mode(
    condition: mem_eval::EvalCondition,
) -> mem_api::QueryRetrievalMode {
    match condition {
        mem_eval::EvalCondition::NoMemory | mem_eval::EvalCondition::FullMemory => {
            mem_api::QueryRetrievalMode::FullMemory
        }
        mem_eval::EvalCondition::Lexical => mem_api::QueryRetrievalMode::Lexical,
        mem_eval::EvalCondition::Semantic => mem_api::QueryRetrievalMode::Semantic,
        mem_eval::EvalCondition::Graph => mem_api::QueryRetrievalMode::Graph,
    }
}

pub(crate) fn offline_no_memory_grounded_answer_eval_item(
    item: &mem_eval::GroundedAnswerItem,
    condition: mem_eval::EvalCondition,
) -> mem_eval::EvalItemResult {
    mem_eval::score_plain_llm_grounded_answer(
        item,
        condition,
        "Offline no-memory baseline: no Memory Layer context was supplied.".to_string(),
        Some(0.0),
        Some(0),
        None,
        vec!["answer_source: offline deterministic no-memory baseline".to_string()],
    )
}

pub(crate) fn offline_no_memory_resume_quality_eval_item(
    item: &mem_eval::ResumeQualityItem,
    condition: mem_eval::EvalCondition,
) -> mem_eval::EvalItemResult {
    mem_eval::score_resume_text_quality(
        item,
        condition,
        "Offline no-memory baseline: no Memory timeline or retrieval context was supplied."
            .to_string(),
        Some(0),
        None,
        vec!["answer_source: offline deterministic no-memory baseline".to_string()],
    )
}

#[derive(Debug)]
pub(crate) struct DirectLlmEvalResponse {
    pub(crate) content: String,
    pub(crate) duration_ms: u64,
    pub(crate) token_usage: Option<TokenUsage>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct NoMemoryGroundedAnswerPayload {
    pub(crate) answer: String,
    #[serde(default)]
    pub(crate) confidence: Option<f32>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct EvalJudgePayload {
    #[serde(default)]
    pub(crate) evidence_use: Option<f64>,
    #[serde(default)]
    pub(crate) reasoning_quality: Option<f64>,
    #[serde(default)]
    pub(crate) consistency: Option<f64>,
    #[serde(default)]
    pub(crate) maintainability: Option<f64>,
    #[serde(default)]
    pub(crate) notes: Option<String>,
}

pub(crate) async fn add_llm_judge_scores(
    api: &ApiClient,
    item: &mem_eval::EvalItem,
    result: &mut mem_eval::EvalItemResult,
) -> Result<()> {
    if !matches!(
        item,
        mem_eval::EvalItem::GroundedAnswer(_) | mem_eval::EvalItem::ResumeQuality(_)
    ) {
        return Ok(());
    }
    let Some(answer) = result.answer.as_deref() else {
        return Ok(());
    };
    let prompt = format!(
        "Eval item id: {}\nEval type: {}\nReasoning mode: {}\nMemory capability: {}\n\nAnswer or briefing:\n{}",
        result.item_id,
        result.eval_type,
        result
            .metadata
            .reasoning_mode
            .as_deref()
            .unwrap_or("unspecified"),
        result
            .metadata
            .memory_capability
            .as_deref()
            .unwrap_or("unspecified"),
        answer
    );
    let response = run_direct_llm_eval(
        api,
        "You are an eval judge. Return strict JSON with numeric 0..1 keys: evidence_use, reasoning_quality, consistency, maintainability, and a short notes string. Score only the supplied answer, not whether you personally know the facts.",
        &prompt,
        api.config.llm.max_output_tokens.min(700),
    )
    .await?;
    let (judge, mut notes) = parse_eval_judge_payload(&response.content);
    if let Some(value) = judge.evidence_use {
        result
            .scores
            .insert("judge_evidence_use".to_string(), value.clamp(0.0, 1.0));
    }
    if let Some(value) = judge.reasoning_quality {
        result
            .scores
            .insert("judge_reasoning_quality".to_string(), value.clamp(0.0, 1.0));
    }
    if let Some(value) = judge.consistency {
        result
            .scores
            .insert("judge_consistency".to_string(), value.clamp(0.0, 1.0));
    }
    if let Some(value) = judge.maintainability {
        result
            .scores
            .insert("judge_maintainability".to_string(), value.clamp(0.0, 1.0));
    }
    if let Some(note) = judge.notes.filter(|note| !note.trim().is_empty()) {
        notes.push(format!("llm_judge: {}", note.trim()));
    }
    result.notes.extend(notes);
    if let Some(usage) = response.token_usage {
        result
            .scores
            .insert("judge_total_tokens".to_string(), usage.total_tokens as f64);
    }
    Ok(())
}

pub(crate) fn parse_eval_judge_payload(content: &str) -> (EvalJudgePayload, Vec<String>) {
    let trimmed = content.trim();
    let json = trimmed
        .strip_prefix("```json")
        .and_then(|value| value.strip_suffix("```"))
        .or_else(|| {
            trimmed
                .strip_prefix("```")
                .and_then(|value| value.strip_suffix("```"))
        })
        .map(str::trim)
        .unwrap_or(trimmed);
    match serde_json::from_str::<EvalJudgePayload>(json) {
        Ok(payload) => (payload, vec!["llm_judge: scored answer".to_string()]),
        Err(_) => (
            EvalJudgePayload {
                evidence_use: None,
                reasoning_quality: None,
                consistency: None,
                maintainability: None,
                notes: None,
            },
            vec!["llm_judge: response was not strict judge JSON".to_string()],
        ),
    }
}

pub(crate) async fn run_no_memory_grounded_answer_eval_item(
    api: &ApiClient,
    item: &mem_eval::GroundedAnswerItem,
    condition: mem_eval::EvalCondition,
) -> Result<mem_eval::EvalItemResult> {
    let response = run_direct_llm_eval(
        api,
        "You answer evaluation questions without Memory Layer context. Return strict JSON with keys: answer (string), confidence (0..1). If you do not know, say so in the answer and use low confidence.",
        &format!("Question: {}", item.question),
        api.config.llm.max_output_tokens.min(800),
    )
    .await?;
    let (answer, confidence, mut notes) = parse_no_memory_grounded_answer(&response.content);
    notes.push("answer_source: direct no-memory LLM call".to_string());
    Ok(mem_eval::score_plain_llm_grounded_answer(
        item,
        condition,
        answer,
        confidence,
        Some(response.duration_ms),
        response.token_usage,
        notes,
    ))
}

pub(crate) async fn run_no_memory_resume_quality_eval_item(
    api: &ApiClient,
    item: &mem_eval::ResumeQualityItem,
    condition: mem_eval::EvalCondition,
) -> Result<mem_eval::EvalItemResult> {
    let prompt = if item.prompt.trim().is_empty() {
        "Get me up to speed on this project. You do not have access to Memory Layer context, repository history, or persisted project timeline data.".to_string()
    } else {
        item.prompt.clone()
    };
    let response = run_direct_llm_eval(
        api,
        "You write concise project resume briefings without Memory Layer context. Be explicit when the prompt lacks enough project evidence.",
        &prompt,
        api.config.llm.max_output_tokens.min(800),
    )
    .await?;
    Ok(mem_eval::score_resume_text_quality(
        item,
        condition,
        response.content,
        Some(response.duration_ms),
        response.token_usage,
        vec!["answer_source: direct no-memory LLM call".to_string()],
    ))
}

pub(crate) async fn run_direct_llm_eval(
    api: &ApiClient,
    system_prompt: &str,
    user_prompt: &str,
    max_output_tokens: u32,
) -> Result<DirectLlmEvalResponse> {
    ensure_direct_llm_eval_config(&api.config)?;
    let api_key = resolve_llm_api_key(&api.config.llm);
    let url = format!(
        "{}/chat/completions",
        effective_llm_base_url(&api.config.llm)
    );
    let mut request = serde_json::json!({
        "model": api.config.llm.model,
        "temperature": 0.0,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_prompt }
        ]
    });
    request[llm_max_output_tokens_field(&api.config.llm.provider)] =
        serde_json::json!(max_output_tokens);
    let started = std::time::Instant::now();
    let mut builder = api.client.post(url);
    if let Some(api_key) = api_key {
        builder = builder.bearer_auth(api_key);
    }
    let http_response = builder
        .json(&request)
        .send()
        .await
        .context("send no-memory eval llm request")?;
    let status = http_response.status();
    let body = http_response
        .text()
        .await
        .context("read no-memory eval llm body")?;
    if !status.is_success() {
        anyhow::bail!("no-memory eval llm request failed: {status} {body}");
    }
    let content = chat_completion_content(&body)?;
    Ok(DirectLlmEvalResponse {
        content,
        duration_ms: started.elapsed().as_millis() as u64,
        token_usage: token_usage_from_chat_body(&body),
    })
}

pub(crate) fn ensure_direct_llm_eval_config(config: &AppConfig) -> Result<()> {
    if !is_supported_llm_provider(&config.llm.provider) {
        anyhow::bail!(
            "no-memory eval requires [llm].provider = openai_compatible or ollama; got `{}`",
            config.llm.provider
        );
    }
    if config.llm.model.trim().is_empty() {
        anyhow::bail!("no-memory eval requires [llm].model to be configured");
    }
    if llm_requires_api_key(&config.llm) && resolve_llm_api_key(&config.llm).is_none() {
        anyhow::bail!(
            "no-memory eval requires {} to be set",
            config.llm.api_key_env
        );
    }
    Ok(())
}

pub(crate) fn chat_completion_content(body: &str) -> Result<String> {
    let payload: serde_json::Value = serde_json::from_str(body).context("parse llm response")?;
    payload
        .get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_str())
        .map(str::trim)
        .filter(|content| !content.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("llm response missing content"))
}

pub(crate) fn parse_no_memory_grounded_answer(content: &str) -> (String, Option<f32>, Vec<String>) {
    let trimmed = content.trim();
    let json = trimmed
        .strip_prefix("```json")
        .and_then(|value| value.strip_suffix("```"))
        .or_else(|| {
            trimmed
                .strip_prefix("```")
                .and_then(|value| value.strip_suffix("```"))
        })
        .map(str::trim)
        .unwrap_or(trimmed);
    match serde_json::from_str::<NoMemoryGroundedAnswerPayload>(json) {
        Ok(payload) if !payload.answer.trim().is_empty() => (
            payload.answer.trim().to_string(),
            payload.confidence.map(|value| value.clamp(0.0, 1.0)),
            Vec::new(),
        ),
        _ => (
            trimmed.to_string(),
            None,
            vec!["plain_llm response was not strict answer/confidence JSON".to_string()],
        ),
    }
}

pub(crate) fn token_usage_from_chat_body(body: &str) -> Option<TokenUsage> {
    let payload: serde_json::Value = serde_json::from_str(body).ok()?;
    let usage = payload.get("usage")?;
    let input_tokens = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(|value| value.as_u64())
        .unwrap_or_default();
    let output_tokens = usage
        .get("completion_tokens")
        .or_else(|| usage.get("output_tokens"))
        .and_then(|value| value.as_u64())
        .unwrap_or_default();
    let cache_read_tokens = usage
        .get("cache_read_input_tokens")
        .or_else(|| usage.get("cached_input_tokens"))
        .and_then(|value| value.as_u64())
        .unwrap_or_default();
    let cache_write_tokens = usage
        .get("cache_creation_input_tokens")
        .and_then(|value| value.as_u64())
        .unwrap_or_default();
    let total_tokens = usage
        .get("total_tokens")
        .and_then(|value| value.as_u64())
        .unwrap_or(input_tokens + output_tokens + cache_read_tokens + cache_write_tokens);
    if input_tokens == 0
        && output_tokens == 0
        && cache_read_tokens == 0
        && cache_write_tokens == 0
        && total_tokens == 0
    {
        return None;
    }
    Some(TokenUsage {
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        total_tokens,
    })
}

pub(crate) fn run_command_eval_item(
    item: &mem_eval::CommandTaskItem,
    condition: mem_eval::EvalCondition,
) -> Result<mem_eval::EvalItemResult> {
    let started = std::time::Instant::now();
    let status = ProcessCommand::new("sh")
        .arg("-c")
        .arg(&item.command)
        .status()
        .with_context(|| format!("run eval command `{}`", item.command))?;
    Ok(mem_eval::score_command_task(
        item,
        condition,
        status.code(),
        Some(started.elapsed().as_millis() as u64),
        Vec::new(),
    ))
}

pub(crate) fn run_agent_build_eval_item(
    suite: &mem_eval::EvalSuite,
    item: &mem_eval::AgentBuildTaskItem,
    condition: mem_eval::EvalCondition,
    context: &EvalRunContext,
) -> Result<mem_eval::EvalItemResult> {
    let started = Instant::now();
    let fixture_dir = suite.root.join(&item.fixture);
    if !fixture_dir.is_dir() {
        anyhow::bail!(
            "agent build task `{}` fixture is not a directory: {}",
            item.id,
            fixture_dir.display()
        );
    }
    validate_agent_build_paths(item)?;
    if context.dry_run {
        return Ok(mem_eval::score_agent_build_task(
            item,
            condition,
            mem_eval::AgentBuildScoreInput {
                agent_exit_code: None,
                setup_exit_codes: Vec::new(),
                score_exit_codes: Vec::new(),
                required_files_present: 0,
                required_files_total: item.required_files.len(),
                forbidden_files_absent: 0,
                forbidden_files_total: item.forbidden_files.len(),
                content_assertions_passed: 0,
                content_assertions_total: item.required_content.len(),
                memory_queries_required: item.memory_questions.len(),
                memory_queries_verified: 0,
                memory_evidence_required: condition != mem_eval::EvalCondition::NoMemory
                    && !item.memory_questions.is_empty(),
                memory_evidence_ok: false,
                token_usage_required: false,
                token_usage_ok: true,
                token_usage: None,
                duration_ms: Some(0),
                notes: vec![
                    "dry-run: validated fixture and command templates without execution"
                        .to_string(),
                ],
                sub_results: Vec::new(),
                skipped: true,
            },
        ));
    }

    let run_dir = context.artifacts_root.join("build-runs").join(format!(
        "{}-{}-{}-r{}-{}",
        sanitize_filename(&suite.manifest.name),
        sanitize_filename(&item.id),
        condition,
        context.repeat_index,
        context.run_group_id.simple()
    ));
    if run_dir.exists() {
        fs::remove_dir_all(&run_dir)
            .with_context(|| format!("remove previous build run {}", run_dir.display()))?;
    }
    let workspace = run_dir.join("workspace");
    fs::create_dir_all(&run_dir).with_context(|| format!("create {}", run_dir.display()))?;
    copy_dir_recursive(&fixture_dir, &workspace)?;
    let project = item
        .project
        .as_deref()
        .or(suite.manifest.project.as_deref())
        .unwrap_or("");
    if condition != mem_eval::EvalCondition::NoMemory && !item.memory_questions.is_empty() {
        write_agent_build_memory_helper(&workspace, item, context)?;
    }

    let prompt = agent_build_prompt(item, condition, context);
    let prompt_file = run_dir.join("prompt.md");
    fs::write(&prompt_file, &prompt).with_context(|| format!("write {}", prompt_file.display()))?;

    let mut notes = vec![format!("artifacts: {}", run_dir.display())];
    let mut setup_exit_codes = Vec::new();
    for (index, command) in item.setup_commands.iter().enumerate() {
        let command = expand_agent_build_template(
            command,
            suite,
            condition,
            &run_dir,
            &workspace,
            &prompt_file,
            project,
        );
        let output = run_eval_shell_command(
            &command,
            &workspace,
            item.timeout_seconds,
            Some(condition),
            Some(project),
            Some(context),
        )?;
        write_command_artifacts(&run_dir, &format!("setup-{index}"), &output)?;
        setup_exit_codes.push(output.exit_code);
    }

    let agent_command = expand_agent_build_template(
        &item.agent_command,
        suite,
        condition,
        &run_dir,
        &workspace,
        &prompt_file,
        project,
    );
    let agent_output = run_eval_shell_command(
        &agent_command,
        &workspace,
        item.timeout_seconds,
        Some(condition),
        Some(project),
        Some(context),
    )?;
    write_command_artifacts(&run_dir, "agent", &agent_output)?;
    if agent_output.timed_out {
        notes.push(format!(
            "agent command timed out after {} second(s)",
            item.timeout_seconds
        ));
    }
    let memory_evidence = validate_agent_build_memory_evidence(&workspace, item, condition)?;
    notes.extend(memory_evidence.notes.clone());

    let mut score_exit_codes = Vec::new();
    for (index, command) in item.score_commands.iter().enumerate() {
        let command = expand_agent_build_template(
            command,
            suite,
            condition,
            &run_dir,
            &workspace,
            &prompt_file,
            project,
        );
        let output = run_eval_shell_command(
            &command,
            &workspace,
            item.timeout_seconds,
            Some(condition),
            Some(project),
            Some(context),
        )?;
        write_command_artifacts(&run_dir, &format!("score-{index}"), &output)?;
        score_exit_codes.push(output.exit_code);
    }

    let required_files_present = item
        .required_files
        .iter()
        .filter(|path| workspace.join(path).is_file())
        .count();
    let forbidden_files_absent = item
        .forbidden_files
        .iter()
        .filter(|path| !workspace.join(path).exists())
        .count();
    let content_assertions_passed = item
        .required_content
        .iter()
        .filter(|assertion| {
            fs::read_to_string(workspace.join(&assertion.file))
                .map(|contents| contents.contains(&assertion.contains))
                .unwrap_or(false)
        })
        .count();

    let summary = serde_json::json!({
        "item_id": item.id,
        "condition": condition,
        "run_dir": run_dir,
        "workspace": workspace,
        "agent_exit_code": agent_output.exit_code,
        "agent_timed_out": agent_output.timed_out,
        "setup_exit_codes": setup_exit_codes,
        "score_exit_codes": score_exit_codes,
        "required_files_present": required_files_present,
        "required_files_total": item.required_files.len(),
        "forbidden_files_absent": forbidden_files_absent,
        "forbidden_files_total": item.forbidden_files.len(),
        "content_assertions_passed": content_assertions_passed,
        "content_assertions_total": item.required_content.len(),
        "memory_queries_required": memory_evidence.required,
        "memory_queries_verified": memory_evidence.verified,
        "memory_evidence_required": true,
        "memory_evidence_ok": memory_evidence.ok,
        "memory_evidence_notes": memory_evidence.notes,
    });
    fs::write(
        run_dir.join("summary.json"),
        serde_json::to_string_pretty(&summary)?,
    )
    .with_context(|| format!("write {}", run_dir.join("summary.json").display()))?;

    Ok(mem_eval::score_agent_build_task(
        item,
        condition,
        mem_eval::AgentBuildScoreInput {
            agent_exit_code: agent_output.exit_code,
            setup_exit_codes,
            score_exit_codes,
            required_files_present,
            required_files_total: item.required_files.len(),
            forbidden_files_absent,
            forbidden_files_total: item.forbidden_files.len(),
            content_assertions_passed,
            content_assertions_total: item.required_content.len(),
            memory_queries_required: memory_evidence.required,
            memory_queries_verified: memory_evidence.verified,
            memory_evidence_required: true,
            memory_evidence_ok: memory_evidence.ok,
            token_usage_required: false,
            token_usage_ok: true,
            token_usage: codex_token_usage_from_run_dir(&run_dir)?,
            duration_ms: Some(started.elapsed().as_millis() as u64),
            notes,
            sub_results: Vec::new(),
            skipped: false,
        },
    ))
}

pub(crate) fn run_agent_build_sequence_eval_item(
    suite: &mem_eval::EvalSuite,
    item: &mem_eval::AgentBuildSequenceItem,
    condition: mem_eval::EvalCondition,
    context: &EvalRunContext,
) -> Result<mem_eval::EvalItemResult> {
    let started = Instant::now();
    let fixture_dir = suite.root.join(&item.fixture);
    if !fixture_dir.is_dir() {
        anyhow::bail!(
            "agent build sequence `{}` fixture is not a directory: {}",
            item.id,
            fixture_dir.display()
        );
    }
    validate_agent_build_sequence_paths(item)?;
    if context.dry_run {
        return Ok(mem_eval::score_agent_build_sequence(
            item,
            condition,
            mem_eval::AgentBuildScoreInput {
                agent_exit_code: None,
                setup_exit_codes: Vec::new(),
                score_exit_codes: Vec::new(),
                required_files_present: 0,
                required_files_total: item
                    .steps
                    .iter()
                    .map(|step| step.required_files.len())
                    .sum(),
                forbidden_files_absent: 0,
                forbidden_files_total: item
                    .steps
                    .iter()
                    .map(|step| step.forbidden_files.len())
                    .sum(),
                content_assertions_passed: 0,
                content_assertions_total: item
                    .steps
                    .iter()
                    .map(|step| step.required_content.len())
                    .sum(),
                memory_queries_required: item
                    .steps
                    .iter()
                    .map(|step| step.memory_questions.len())
                    .sum(),
                memory_queries_verified: 0,
                memory_evidence_required: condition != mem_eval::EvalCondition::NoMemory,
                memory_evidence_ok: false,
                token_usage_required: false,
                token_usage_ok: true,
                token_usage: None,
                duration_ms: Some(0),
                notes: vec![
                    "dry-run: validated sequence fixture and command templates without execution"
                        .to_string(),
                ],
                sub_results: Vec::new(),
                skipped: true,
            },
        ));
    }

    let run_dir = context.artifacts_root.join("build-runs").join(format!(
        "{}-{}-{}-r{}-{}",
        sanitize_filename(&suite.manifest.name),
        sanitize_filename(&item.id),
        condition,
        context.repeat_index,
        context.run_group_id.simple()
    ));
    if run_dir.exists() {
        fs::remove_dir_all(&run_dir)
            .with_context(|| format!("remove previous sequence run {}", run_dir.display()))?;
    }
    let workspace = run_dir.join("workspace");
    let steps_dir = run_dir.join("steps");
    fs::create_dir_all(&steps_dir).with_context(|| format!("create {}", steps_dir.display()))?;
    copy_dir_recursive(&fixture_dir, &workspace)?;
    let project = item
        .project
        .as_deref()
        .or(suite.manifest.project.as_deref())
        .unwrap_or("");

    let mut notes = vec![format!("artifacts: {}", run_dir.display())];
    let mut setup_exit_codes = Vec::new();
    for (index, command) in item.setup_commands.iter().enumerate() {
        let output = run_eval_shell_command(
            &expand_agent_build_template(
                command,
                suite,
                condition,
                &run_dir,
                &workspace,
                &run_dir.join("setup-prompt.md"),
                project,
            ),
            &workspace,
            item.timeout_seconds,
            Some(condition),
            Some(project),
            Some(context),
        )?;
        write_command_artifacts(&run_dir, &format!("setup-{index}"), &output)?;
        setup_exit_codes.push(output.exit_code);
    }

    let mut agent_exit_codes = Vec::new();
    let mut score_exit_codes = Vec::new();
    let mut required_files_present = 0usize;
    let mut required_files_total = 0usize;
    let mut forbidden_files_absent = 0usize;
    let mut forbidden_files_total = 0usize;
    let mut content_assertions_passed = 0usize;
    let mut content_assertions_total = 0usize;
    let mut memory_queries_required = 0usize;
    let mut memory_queries_verified = 0usize;
    let mut memory_evidence_ok = true;
    let mut token_usage = TokenUsage::default();
    let mut saw_token_usage = false;
    let mut step_summaries = Vec::new();
    let mut sub_results = Vec::new();

    for (index, step) in item.steps.iter().enumerate() {
        let step_started = Instant::now();
        let step_label = format!("{:02}-{}", index + 1, sanitize_filename(&step.id));
        let step_dir = steps_dir.join(&step_label);
        fs::create_dir_all(&step_dir).with_context(|| format!("create {}", step_dir.display()))?;
        let step_timeout = step.timeout_seconds.unwrap_or(item.timeout_seconds);
        let step_task = sequence_step_as_task(item, step, step_timeout);
        if workspace.join(".memory-eval").exists() {
            fs::remove_dir_all(workspace.join(".memory-eval"))
                .with_context(|| format!("clear step Memory evidence for {}", step.id))?;
        }
        if condition != mem_eval::EvalCondition::NoMemory && !step.memory_questions.is_empty() {
            write_agent_build_memory_helper(&workspace, &step_task, context)?;
        }
        let prompt = agent_build_prompt(&step_task, condition, context);
        let prompt_file = step_dir.join("prompt.md");
        fs::write(&prompt_file, &prompt)
            .with_context(|| format!("write {}", prompt_file.display()))?;
        let agent_command = expand_agent_build_template(
            &item.agent_command,
            suite,
            condition,
            &step_dir,
            &workspace,
            &prompt_file,
            project,
        );
        let agent_output = run_eval_shell_command(
            &agent_command,
            &workspace,
            step_timeout,
            Some(condition),
            Some(project),
            Some(context),
        )?;
        write_command_artifacts(&step_dir, "agent", &agent_output)?;
        agent_exit_codes.push(agent_output.exit_code);
        if agent_output.timed_out {
            notes.push(format!(
                "step {} agent command timed out after {} second(s)",
                step.id, step_timeout
            ));
        }
        let memory_evidence =
            validate_agent_build_memory_evidence(&workspace, &step_task, condition)?;
        notes.extend(
            memory_evidence
                .notes
                .iter()
                .map(|note| format!("step {}: {note}", step.id)),
        );
        memory_queries_required += memory_evidence.required;
        memory_queries_verified += memory_evidence.verified;
        memory_evidence_ok &= memory_evidence.ok;
        if workspace.join(".memory-eval").is_dir() {
            copy_dir_recursive(
                &workspace.join(".memory-eval"),
                &step_dir.join("memory-eval"),
            )?;
        }

        let mut step_score_exit_codes = Vec::new();
        for (score_index, command) in step.score_commands.iter().enumerate() {
            let output = run_eval_shell_command(
                &expand_agent_build_template(
                    command,
                    suite,
                    condition,
                    &step_dir,
                    &workspace,
                    &prompt_file,
                    project,
                ),
                &workspace,
                step_timeout,
                Some(condition),
                Some(project),
                Some(context),
            )?;
            write_command_artifacts(&step_dir, &format!("score-{score_index}"), &output)?;
            step_score_exit_codes.push(output.exit_code);
            score_exit_codes.push(output.exit_code);
        }

        let step_required_present = step
            .required_files
            .iter()
            .filter(|path| workspace.join(path).is_file())
            .count();
        let step_forbidden_absent = step
            .forbidden_files
            .iter()
            .filter(|path| !workspace.join(path).exists())
            .count();
        let step_content_passed = step
            .required_content
            .iter()
            .filter(|assertion| {
                fs::read_to_string(workspace.join(&assertion.file))
                    .map(|contents| contents.contains(&assertion.contains))
                    .unwrap_or(false)
            })
            .count();
        required_files_present += step_required_present;
        required_files_total += step.required_files.len();
        forbidden_files_absent += step_forbidden_absent;
        forbidden_files_total += step.forbidden_files.len();
        content_assertions_passed += step_content_passed;
        content_assertions_total += step.required_content.len();

        let step_token_usage = codex_token_usage_from_run_dir(&step_dir)?;
        if let Some(usage) = &step_token_usage {
            saw_token_usage = true;
            add_token_usage(&mut token_usage, usage);
        }
        let step_success = agent_output.exit_code == Some(0)
            && step_score_exit_codes.iter().all(|code| *code == Some(0))
            && step_required_present == step.required_files.len()
            && step_forbidden_absent == step.forbidden_files.len()
            && step_content_passed == step.required_content.len()
            && memory_evidence.ok;
        let mut step_scores = BTreeMap::new();
        step_scores.insert(
            "agent_exit_code".to_string(),
            agent_output.exit_code.unwrap_or(-1) as f64,
        );
        step_scores.insert(
            "score_commands_passed".to_string(),
            step_score_exit_codes
                .iter()
                .filter(|code| **code == Some(0))
                .count() as f64,
        );
        step_scores.insert(
            "score_commands_total".to_string(),
            step_score_exit_codes.len() as f64,
        );
        step_scores.insert(
            "required_files_present".to_string(),
            step_required_present as f64,
        );
        step_scores.insert(
            "required_files_total".to_string(),
            step.required_files.len() as f64,
        );
        step_scores.insert(
            "forbidden_files_absent".to_string(),
            step_forbidden_absent as f64,
        );
        step_scores.insert(
            "forbidden_files_total".to_string(),
            step.forbidden_files.len() as f64,
        );
        step_scores.insert(
            "content_assertions_passed".to_string(),
            step_content_passed as f64,
        );
        step_scores.insert(
            "content_assertions_total".to_string(),
            step.required_content.len() as f64,
        );
        step_scores.insert(
            "memory_queries_required".to_string(),
            memory_evidence.required as f64,
        );
        step_scores.insert(
            "memory_queries_verified".to_string(),
            memory_evidence.verified as f64,
        );
        step_scores.insert(
            "memory_evidence_ok".to_string(),
            if memory_evidence.ok { 1.0 } else { 0.0 },
        );
        step_scores.insert(
            "total_score".to_string(),
            if step_success { 1.0 } else { 0.0 },
        );
        sub_results.push(mem_eval::EvalSubResult {
            id: step.id.clone(),
            eval_type: "agent_build_sequence_step".to_string(),
            metadata: step.metadata.clone(),
            success: step_success,
            skipped: false,
            scores: step_scores,
            duration_ms: Some(step_started.elapsed().as_millis() as u64),
            token_usage: step_token_usage.clone(),
            notes: memory_evidence.notes.clone(),
        });
        step_summaries.push(serde_json::json!({
            "id": step.id,
            "metadata": step.metadata,
            "success": step_success,
            "agent_exit_code": agent_output.exit_code,
            "score_exit_codes": step_score_exit_codes,
            "memory_queries_required": memory_evidence.required,
            "memory_queries_verified": memory_evidence.verified,
            "memory_evidence_ok": memory_evidence.ok,
            "token_usage": step_token_usage,
        }));
    }

    let token_usage_required = agent_build_command_requires_token_usage(&item.agent_command);
    let token_usage_ok = !token_usage_required || saw_token_usage;
    if !token_usage_ok {
        notes.push(
            "Codex sequence run did not emit parseable token usage; expected codex-events.jsonl or codex-token-usage.json"
                .to_string(),
        );
    }

    let summary = serde_json::json!({
        "item_id": item.id,
        "condition": condition,
        "run_dir": run_dir,
        "workspace": workspace,
        "steps": step_summaries,
        "setup_exit_codes": setup_exit_codes,
        "memory_queries_required": memory_queries_required,
        "memory_queries_verified": memory_queries_verified,
        "memory_evidence_ok": memory_evidence_ok,
        "token_usage_required": token_usage_required,
        "token_usage_ok": token_usage_ok,
        "token_usage": if saw_token_usage { Some(&token_usage) } else { None },
    });
    fs::write(
        run_dir.join("summary.json"),
        serde_json::to_string_pretty(&summary)?,
    )
    .with_context(|| format!("write {}", run_dir.join("summary.json").display()))?;

    let agent_exit_code = if agent_exit_codes.iter().all(|code| *code == Some(0)) {
        Some(0)
    } else {
        agent_exit_codes
            .iter()
            .copied()
            .find(|code| *code != Some(0))
            .flatten()
    };
    Ok(mem_eval::score_agent_build_sequence(
        item,
        condition,
        mem_eval::AgentBuildScoreInput {
            agent_exit_code,
            setup_exit_codes,
            score_exit_codes,
            required_files_present,
            required_files_total,
            forbidden_files_absent,
            forbidden_files_total,
            content_assertions_passed,
            content_assertions_total,
            memory_queries_required,
            memory_queries_verified,
            memory_evidence_required: true,
            memory_evidence_ok,
            token_usage_required,
            token_usage_ok,
            token_usage: saw_token_usage.then_some(token_usage),
            duration_ms: Some(started.elapsed().as_millis() as u64),
            notes,
            sub_results,
            skipped: false,
        },
    ))
}

pub(crate) fn agent_build_command_requires_token_usage(command: &str) -> bool {
    command.contains("run-codex") || command.split_whitespace().any(|part| part == "codex")
}

pub(crate) fn sequence_step_as_task(
    item: &mem_eval::AgentBuildSequenceItem,
    step: &mem_eval::AgentBuildSequenceStep,
    timeout_seconds: u64,
) -> mem_eval::AgentBuildTaskItem {
    mem_eval::AgentBuildTaskItem {
        id: step.id.clone(),
        metadata: step.metadata.clone(),
        project: item.project.clone(),
        prompt: step.prompt.clone(),
        fixture: item.fixture.clone(),
        agent_command: item.agent_command.clone(),
        memory_questions: step.memory_questions.clone(),
        setup_commands: Vec::new(),
        score_commands: step.score_commands.clone(),
        timeout_seconds,
        required_files: step.required_files.clone(),
        forbidden_files: step.forbidden_files.clone(),
        required_content: step.required_content.clone(),
    }
}

#[derive(Debug)]
pub(crate) struct AgentBuildMemoryEvidence {
    pub(crate) required: usize,
    pub(crate) verified: usize,
    pub(crate) ok: bool,
    pub(crate) notes: Vec<String>,
}

pub(crate) fn write_agent_build_memory_helper(
    workspace: &Path,
    item: &mem_eval::AgentBuildTaskItem,
    _context: &EvalRunContext,
) -> Result<()> {
    let evidence_dir = workspace.join(".memory-eval");
    fs::create_dir_all(&evidence_dir)
        .with_context(|| format!("create {}", evidence_dir.display()))?;
    let helper_binary = evidence_dir.join("memory");
    let current_exe = env::current_exe()?;
    let copy_source = if Path::new("/proc/self/exe").is_file() {
        Path::new("/proc/self/exe")
    } else {
        current_exe.as_path()
    };
    fs::copy(copy_source, &helper_binary)
        .with_context(|| format!("copy Memory CLI to {}", helper_binary.display()))?;
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&helper_binary)
            .with_context(|| format!("stat {}", helper_binary.display()))?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&helper_binary, permissions)
            .with_context(|| format!("chmod {}", helper_binary.display()))?;
    }
    let questions = item
        .memory_questions
        .iter()
        .enumerate()
        .map(|(index, question)| {
            serde_json::json!({
                "id": agent_build_memory_question_id(index),
                "question": question,
            })
        })
        .collect::<Vec<_>>();
    fs::write(
        evidence_dir.join("required-questions.json"),
        serde_json::to_vec_pretty(&questions)?,
    )
    .with_context(|| {
        format!(
            "write {}",
            evidence_dir.join("required-questions.json").display()
        )
    })?;
    let helper = r#"#!/usr/bin/env sh
set -eu

if [ "$#" -lt 2 ]; then
  echo "usage: ./.memory-eval/query-memory <question-id> <question>" >&2
  exit 64
fi

question_id="$1"
shift
question="$*"

case "$question_id" in
  q[0-9]*) ;;
  *)
    echo "invalid Memory eval question id: $question_id" >&2
    exit 64
    ;;
esac

mkdir -p .memory-eval
out=".memory-eval/${question_id}.json"
err=".memory-eval/${question_id}.stderr.txt"
status=".memory-eval/${question_id}.status.json"
cmd="./.memory-eval/memory"

set +e
"$cmd" query --project "${MEMORY_LAYER_PROJECT:?}" --question "$question" --json > "$out" 2> "$err"
code=$?
set -e
if [ "$code" -eq 0 ] && [ ! -s "$out" ]; then
  echo "Memory query wrote an empty JSON payload" >> "$err"
  code=65
fi

printf '{"question_id":"%s","exit_code":%s,"output_file":"%s"}\n' "$question_id" "$code" "$out" > "$status"
exit "$code"
"#;
    let helper_path = evidence_dir.join("query-memory");
    fs::write(&helper_path, helper).with_context(|| format!("write {}", helper_path.display()))?;
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&helper_path)
            .with_context(|| format!("stat {}", helper_path.display()))?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&helper_path, permissions)
            .with_context(|| format!("chmod {}", helper_path.display()))?;
    }
    Ok(())
}

pub(crate) fn validate_agent_build_memory_evidence(
    workspace: &Path,
    item: &mem_eval::AgentBuildTaskItem,
    condition: mem_eval::EvalCondition,
) -> Result<AgentBuildMemoryEvidence> {
    if condition == mem_eval::EvalCondition::NoMemory {
        let forbidden = [
            workspace.join("memory-evidence.md"),
            workspace.join("memory-evidence.json"),
            workspace.join(".memory-eval"),
        ];
        let leaked = forbidden
            .iter()
            .filter(|path| path.exists())
            .map(|path| {
                path.strip_prefix(workspace)
                    .unwrap_or(path)
                    .display()
                    .to_string()
            })
            .collect::<Vec<_>>();
        let ok = leaked.is_empty();
        return Ok(AgentBuildMemoryEvidence {
            required: 0,
            verified: 0,
            ok,
            notes: if ok {
                vec!["no-memory run left no Memory evidence artifacts".to_string()]
            } else {
                vec![format!(
                    "no-memory run produced forbidden Memory evidence artifact(s): {}",
                    leaked.join(", ")
                )]
            },
        });
    }

    if item.memory_questions.is_empty() {
        return Ok(AgentBuildMemoryEvidence {
            required: 0,
            verified: 0,
            ok: true,
            notes: vec!["memory-enabled run had no required Memory questions".to_string()],
        });
    }

    let mut verified = 0usize;
    let mut notes = Vec::new();
    for (index, question) in item.memory_questions.iter().enumerate() {
        let question_id = agent_build_memory_question_id(index);
        let output_path = workspace
            .join(".memory-eval")
            .join(format!("{question_id}.json"));
        let status_path = workspace
            .join(".memory-eval")
            .join(format!("{question_id}.status.json"));
        let status_ok = if !status_path.is_file() {
            notes.push(format!("missing Memory query status for {question_id}"));
            false
        } else {
            match read_json_file(&status_path) {
                Ok(status) => {
                    if status.get("exit_code").and_then(serde_json::Value::as_i64) != Some(0) {
                        notes.push(format!("Memory query {question_id} exited non-zero"));
                        false
                    } else {
                        true
                    }
                }
                Err(error) => {
                    notes.push(format!(
                        "Memory query {question_id} status is invalid: {error}"
                    ));
                    false
                }
            }
        };
        let result_count = if !output_path.is_file() {
            notes.push(format!("missing Memory query output for {question_id}"));
            0
        } else {
            match read_json_file(&output_path) {
                Ok(output) => {
                    let result_count = output
                        .get("results")
                        .and_then(serde_json::Value::as_array)
                        .map(Vec::len)
                        .unwrap_or(0);
                    if result_count == 0 {
                        notes.push(format!("Memory query {question_id} returned no memories"));
                    }
                    result_count
                }
                Err(error) => {
                    notes.push(format!(
                        "Memory query {question_id} output is invalid: {error}"
                    ));
                    0
                }
            }
        };
        if status_ok && result_count > 0 {
            verified += 1;
            notes.push(format!(
                "verified Memory query {question_id} ({result_count} result(s)): {question}"
            ));
        }
    }
    let required = item.memory_questions.len();
    Ok(AgentBuildMemoryEvidence {
        required,
        verified,
        ok: verified == required,
        notes,
    })
}

pub(crate) fn read_json_file(path: &Path) -> Result<serde_json::Value> {
    let contents = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&contents).with_context(|| format!("parse {}", path.display()))
}

pub(crate) fn agent_build_memory_question_id(index: usize) -> String {
    format!("q{}", index + 1)
}

#[derive(Debug)]
pub(crate) struct EvalShellOutput {
    pub(crate) exit_code: Option<i32>,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) timed_out: bool,
}

pub(crate) fn run_eval_shell_command(
    command: &str,
    cwd: &Path,
    timeout_seconds: u64,
    condition: Option<mem_eval::EvalCondition>,
    project: Option<&str>,
    context: Option<&EvalRunContext>,
) -> Result<EvalShellOutput> {
    let mut command_builder = ProcessCommand::new("sh");
    command_builder
        .arg("-c")
        .arg(command)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("MEMORY_EVAL_WORKSPACE", cwd)
        .env("MEMORY_EVAL_TIMEOUT_SECONDS", timeout_seconds.to_string());
    if let Some(condition) = condition {
        command_builder.env("MEMORY_EVAL_CONDITION", condition.to_string());
        match condition {
            mem_eval::EvalCondition::NoMemory => {
                command_builder
                    .env("MEMORY_EVAL_MEMORY_ENABLED", "0")
                    .env_remove("MEMORY_LAYER_API_TOKEN")
                    .env_remove("MEMORY_LAYER_AGENT_ID")
                    .env_remove("MEMORY_LAYER_PROJECT")
                    .env_remove("MEMORY_CONFIG")
                    .env_remove("MEMORY_LAYER_CONFIG")
                    .env_remove("MEMORY_BASE_URL");
            }
            _ => {
                command_builder.env("MEMORY_EVAL_MEMORY_ENABLED", "1");
                if let Some(project) = project {
                    command_builder.env("MEMORY_LAYER_PROJECT", project);
                }
                if let Some(context) = context {
                    command_builder
                        .env("MEMORY_EVAL_MEMORY_COMMAND", &context.memory_command)
                        .env("MEMORY_BASE_URL", &context.memory_base_url);
                    if let Some(config_path) = &context.memory_config_path {
                        command_builder
                            .env("MEMORY_CONFIG", config_path)
                            .env("MEMORY_LAYER_CONFIG", config_path);
                        if config_path
                            .parent()
                            .map(|parent| parent.join("config.dev.toml").is_file())
                            .unwrap_or(false)
                        {
                            command_builder.env("MEMORY_LAYER_PROFILE", "dev");
                        }
                    }
                }
            }
        }
    }
    let mut child = command_builder
        .spawn()
        .with_context(|| format!("run eval command `{command}` in {}", cwd.display()))?;
    let timeout = Duration::from_secs(timeout_seconds.max(1));
    let started = Instant::now();
    let mut timed_out = false;
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            let _ = child.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let output = child
        .wait_with_output()
        .with_context(|| format!("collect eval command `{command}` output"))?;
    Ok(EvalShellOutput {
        exit_code: output.status.code(),
        stdout: output.stdout,
        stderr: output.stderr,
        timed_out,
    })
}

pub(crate) fn write_command_artifacts(
    run_dir: &Path,
    stem: &str,
    output: &EvalShellOutput,
) -> Result<()> {
    fs::write(run_dir.join(format!("{stem}.stdout.txt")), &output.stdout)
        .with_context(|| format!("write {stem} stdout"))?;
    fs::write(run_dir.join(format!("{stem}.stderr.txt")), &output.stderr)
        .with_context(|| format!("write {stem} stderr"))?;
    fs::write(
        run_dir.join(format!("{stem}.status.json")),
        serde_json::to_string_pretty(&serde_json::json!({
            "exit_code": output.exit_code,
            "timed_out": output.timed_out,
        }))?,
    )
    .with_context(|| format!("write {stem} status"))?;
    Ok(())
}

pub(crate) fn codex_token_usage_from_run_dir(run_dir: &Path) -> Result<Option<TokenUsage>> {
    let usage_path = run_dir.join("codex-token-usage.json");
    if usage_path.is_file() {
        let value: serde_json::Value = read_json_file(&usage_path)?;
        return Ok(token_usage_from_json_value(&value));
    }
    let events_path = run_dir.join("codex-events.jsonl");
    if !events_path.is_file() {
        return Ok(None);
    }
    let contents = fs::read_to_string(&events_path)
        .with_context(|| format!("read {}", events_path.display()))?;
    let mut usage = TokenUsage::default();
    let mut found = false;
    for line in contents.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if let Some(candidate) = token_usage_from_json_value(&value)
            && candidate.total_tokens >= usage.total_tokens
        {
            usage = candidate;
            found = true;
        }
    }
    if found {
        fs::write(&usage_path, serde_json::to_vec_pretty(&usage)?)
            .with_context(|| format!("write {}", usage_path.display()))?;
        Ok(Some(usage))
    } else {
        Ok(None)
    }
}

pub(crate) fn token_usage_from_json_value(value: &serde_json::Value) -> Option<TokenUsage> {
    let usage = value
        .get("usage")
        .or_else(|| value.get("token_usage"))
        .or_else(|| value.get("tokenUsage"))
        .or_else(|| value.get("total_token_usage"))
        .or_else(|| value.get("totalTokenUsage"))
        .unwrap_or(value);
    let input_tokens = json_u64_any(
        usage,
        &[
            "input_tokens",
            "prompt_tokens",
            "inputTokens",
            "promptTokens",
        ],
    );
    let output_tokens = json_u64_any(
        usage,
        &[
            "output_tokens",
            "completion_tokens",
            "outputTokens",
            "completionTokens",
        ],
    );
    let cache_read_tokens = json_u64_any(
        usage,
        &[
            "cache_read_tokens",
            "cached_input_tokens",
            "cacheReadTokens",
            "cachedInputTokens",
        ],
    );
    let cache_write_tokens = json_u64_any(
        usage,
        &[
            "cache_write_tokens",
            "cache_creation_input_tokens",
            "cacheWriteTokens",
            "cacheCreationInputTokens",
        ],
    );
    let total_tokens = json_u64_any(usage, &["total_tokens", "totalTokens", "tokens_used"])
        .unwrap_or(
            input_tokens.unwrap_or(0)
                + output_tokens.unwrap_or(0)
                + cache_read_tokens.unwrap_or(0)
                + cache_write_tokens.unwrap_or(0),
        );
    if total_tokens == 0 {
        for child in value_children(value) {
            if let Some(nested) = token_usage_from_json_value(child)
                && nested.total_tokens > 0
            {
                return Some(nested);
            }
        }
        return None;
    }
    Some(TokenUsage {
        input_tokens: input_tokens.unwrap_or(0),
        output_tokens: output_tokens.unwrap_or(0),
        cache_read_tokens: cache_read_tokens.unwrap_or(0),
        cache_write_tokens: cache_write_tokens.unwrap_or(0),
        total_tokens,
    })
}

pub(crate) fn json_u64_any(value: &serde_json::Value, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(serde_json::Value::as_u64))
}

pub(crate) fn value_children(value: &serde_json::Value) -> Vec<&serde_json::Value> {
    match value {
        serde_json::Value::Array(values) => values.iter().collect(),
        serde_json::Value::Object(map) => map.values().collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn add_token_usage(total: &mut TokenUsage, usage: &TokenUsage) {
    total.input_tokens += usage.input_tokens;
    total.output_tokens += usage.output_tokens;
    total.cache_read_tokens += usage.cache_read_tokens;
    total.cache_write_tokens += usage.cache_write_tokens;
    total.total_tokens += usage.total_tokens;
}

pub(crate) fn agent_build_prompt(
    item: &mem_eval::AgentBuildTaskItem,
    condition: mem_eval::EvalCondition,
    context: &EvalRunContext,
) -> String {
    let mut prompt = item.prompt.trim().to_string();
    prompt.push_str("\n\n");
    match condition {
        mem_eval::EvalCondition::NoMemory => prompt.push_str(
            "Evaluation condition: no-memory. Do not query, read, or use Memory Layer context. Do not create memory-evidence.md, memory-evidence.json, or .memory-eval artifacts. Work only from the repository files and this prompt.\n",
        ),
        _ => {
            prompt.push_str(
                "Evaluation condition: memory-enabled. Use Memory Layer context before implementing, then make the requested code changes in the workspace.\n",
            );
            prompt.push_str("\nUse this Memory CLI command from the shell:\n\n```bash\n");
            prompt.push_str(&context.memory_command);
            prompt.push_str("\n```\n\n");
            prompt.push_str("A harness-provided helper exists at `./.memory-eval/query-memory`. Use that helper for every required Memory question so the eval can verify real Memory service access. Do not fabricate Memory evidence; if a helper command fails, stop and report the failure.\n");
            prompt.push_str("Write a file named memory-evidence.md that summarizes the useful facts you used after the helper commands succeed.\n");
            if !item.memory_questions.is_empty() {
                prompt.push_str("\nRequired Memory questions:\n");
                for (index, question) in item.memory_questions.iter().enumerate() {
                    let question_id = agent_build_memory_question_id(index);
                    prompt.push_str("- ");
                    prompt.push_str(&question_id);
                    prompt.push_str(": ");
                    prompt.push_str(question);
                    prompt.push('\n');
                }
                prompt.push_str("\nRun these exact helper commands before editing files:\n\n```bash\n");
                for (index, question) in item.memory_questions.iter().enumerate() {
                    let question_id = agent_build_memory_question_id(index);
                    prompt.push_str("./.memory-eval/query-memory ");
                    prompt.push_str(&question_id);
                    prompt.push(' ');
                    prompt.push_str(&shell_quote_value(question));
                    prompt.push('\n');
                }
                prompt.push_str("```\n");
            }
        }
    }
    prompt
}

pub(crate) fn expand_agent_build_template(
    template: &str,
    suite: &mem_eval::EvalSuite,
    condition: mem_eval::EvalCondition,
    run_dir: &Path,
    workspace: &Path,
    prompt_file: &Path,
    project: &str,
) -> String {
    template
        .replace("{suite_dir}", &shell_quote_path(&suite.root))
        .replace("{run_dir}", &shell_quote_path(run_dir))
        .replace("{workspace}", &shell_quote_path(workspace))
        .replace("{prompt_file}", &shell_quote_path(prompt_file))
        .replace("{condition}", &condition.to_string())
        .replace("{project}", project)
}

pub(crate) fn shell_quote_value(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(crate) fn shell_quote_path(path: &Path) -> String {
    let absolute = absolute_eval_path(path);
    let value = absolute.to_string_lossy();
    shell_quote_value(&value)
}

pub(crate) fn absolute_eval_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

pub(crate) fn validate_agent_build_paths(item: &mem_eval::AgentBuildTaskItem) -> Result<()> {
    for path in item
        .required_files
        .iter()
        .chain(item.forbidden_files.iter())
        .chain(
            item.required_content
                .iter()
                .map(|assertion| &assertion.file),
        )
    {
        let candidate = Path::new(path);
        if candidate.is_absolute()
            || candidate
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            anyhow::bail!(
                "agent build task `{}` path must be workspace-relative without `..`: {}",
                item.id,
                path
            );
        }
    }
    Ok(())
}

pub(crate) fn validate_agent_build_suite_item(
    suite: &mem_eval::EvalSuite,
    item: &mem_eval::AgentBuildTaskItem,
) -> Result<()> {
    let fixture_dir = suite.root.join(&item.fixture);
    if !fixture_dir.is_dir() {
        anyhow::bail!("fixture is not a directory: {}", fixture_dir.display());
    }
    if item.agent_command.trim().is_empty() {
        anyhow::bail!("agent_command must not be empty");
    }
    validate_agent_build_paths(item)?;
    Ok(())
}

pub(crate) fn validate_agent_build_sequence_paths(
    item: &mem_eval::AgentBuildSequenceItem,
) -> Result<()> {
    if item.steps.is_empty() {
        anyhow::bail!(
            "agent build sequence `{}` must contain at least one step",
            item.id
        );
    }
    for step in &item.steps {
        let task = sequence_step_as_task(
            item,
            step,
            step.timeout_seconds.unwrap_or(item.timeout_seconds),
        );
        validate_agent_build_paths(&task)?;
    }
    Ok(())
}

pub(crate) fn validate_agent_build_sequence_suite_item(
    suite: &mem_eval::EvalSuite,
    item: &mem_eval::AgentBuildSequenceItem,
) -> Result<()> {
    let fixture_dir = suite.root.join(&item.fixture);
    if !fixture_dir.is_dir() {
        anyhow::bail!("fixture is not a directory: {}", fixture_dir.display());
    }
    if item.agent_command.trim().is_empty() {
        anyhow::bail!("agent_command must not be empty");
    }
    validate_agent_build_sequence_paths(item)?;
    Ok(())
}

pub(crate) fn eval_memory_command() -> String {
    if let (Ok(exe), Ok(cwd)) = (env::current_exe(), env::current_dir()) {
        let manifest_path = cwd.join("Cargo.toml");
        let is_cargo_target_binary = exe
            .components()
            .any(|component| component.as_os_str() == "target")
            && manifest_path.is_file();
        if is_cargo_target_binary {
            return format!(
                "cargo run --quiet --manifest-path {} --bin memory --",
                shell_quote_value(&manifest_path.to_string_lossy())
            );
        }
        return exe.to_string_lossy().to_string();
    }
    "memory".to_string()
}

pub(crate) fn eval_memory_config_path(cwd: &Path) -> Option<PathBuf> {
    env::var_os("MEMORY_CONFIG")
        .map(PathBuf::from)
        .or_else(|| env::var_os("MEMORY_LAYER_CONFIG").map(PathBuf::from))
        .or_else(|| {
            mem_platform::discover_project_root(cwd)
                .and_then(|repo_root| mem_api::project_paths_for_repo(&repo_root))
                .map(|paths| paths.config_path())
                .filter(|path| path.exists())
        })
        .or_else(|| {
            let candidate = cwd.join(".mem").join("config.toml");
            candidate.exists().then_some(candidate)
        })
}

pub(crate) fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination).with_context(|| format!("create {}", destination.display()))?;
    for entry in fs::read_dir(source).with_context(|| format!("read {}", source.display()))? {
        let entry = entry.with_context(|| format!("read entry in {}", source.display()))?;
        let file_type = entry
            .file_type()
            .with_context(|| format!("read file type for {}", entry.path().display()))?;
        let target = destination.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), &target).with_context(|| {
                format!("copy {} to {}", entry.path().display(), target.display())
            })?;
        } else if file_type.is_symlink() {
            anyhow::bail!(
                "agent build fixtures may not contain symlinks: {}",
                entry.path().display()
            );
        }
    }
    Ok(())
}

pub(crate) fn git_head() -> Option<String> {
    ProcessCommand::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) fn sanitize_filename(value: &str) -> String {
    let slug = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    slug.trim_matches('-').to_string()
}
