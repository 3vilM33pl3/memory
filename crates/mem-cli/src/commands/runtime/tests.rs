#[cfg(test)]
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

use clap::{Command, CommandFactory, Parser, error::ErrorKind};
use uuid::Uuid;

use crate::commands::eval_support::{
    EvalRunContext, ExternalRetrieverResponse, agent_build_prompt,
    build_external_retriever_request, chat_completion_content, ensure_direct_llm_eval_config,
    ensure_eval_retriever_allowed, ensure_eval_shell_allowed, external_retriever_failure_result,
    external_retriever_response_to_query_response, parse_no_memory_grounded_answer,
    run_external_retriever, run_external_retriever_for_retrieval_item, token_usage_from_chat_body,
    token_usage_from_json_value, validate_agent_build_memory_evidence,
};
use crate::commands::{
    init_support::{initialize_dev_overlay, initialize_repo},
    memory_ops::{
        PlanExecutionFinishReport, build_finish_execution_implementation_request,
        build_plan_execution_finish_report, build_plan_execution_request, build_remember_request,
        build_task_start_request, parse_memory_type_arg, resolve_project_slug,
    },
    output::{build_graph_activity_request, write_headers},
    service_support::{
        TuiRestartMarker, newest_tui_restart_notice, parse_systemd_unit_names,
        set_cluster_enabled_in_shared_config,
    },
    skill_support::{
        MEMORY_SKILL_NAMES, SkillBundleStatus, SkillUpgradeAction, SkillVersionStatus,
        project_skill_inventory_with_template, read_skill_version, render_agent_project_config,
        render_claude_md_memory_section, resolve_repo_root, upgrade_project_skills_with_template,
    },
    status_support::{
        is_placeholder_database_url, mask_database_url, repair_repo_bootstrap,
        root_gitignore_contains_mem,
    },
    watch_support::{
        should_start_agent_watcher, watcher_command_requires_config_load, write_file_if_changed,
    },
};

use crate::plan_execution::{
    derive_plan_thread_key, derive_plan_title, durable_plan_source_path, ensure_checkbox_plan,
    parse_plan_checkboxes,
};
use crate::writer_identity::{WriterIdentity, resolve_writer_identity};

use super::{
    Cli, DEV_API_TOKEN, RememberArgs, SERVICE_API_TOKEN_KEY, ServiceApiTokenAction, WatcherCommand,
    WatcherManagerArgs, WatcherManagerCommand, ensure_shared_service_api_token, shared_env_lookup,
};
use mem_api::AppConfig;

#[cfg(target_os = "macos")]
use chrono::Utc;

#[cfg(target_os = "macos")]
use mem_agenttop::{AgentSession, SessionStatus as AgentSessionStatus};

#[cfg(target_os = "macos")]
use super::{
    backend_launch_agent_label, default_global_config_path, managed_watch_launch_agent_label,
    render_backend_launch_agent, render_managed_watch_launch_agent, render_watch_launch_agent,
    render_watch_manager_launch_agent, sanitize_service_fragment, watch_launch_agent_label,
    watch_manager_launch_agent_label,
};

#[cfg(not(target_os = "macos"))]
use crate::commands::watch_support::{
    render_watch_manager_unit, render_watch_unit, watch_unit_name,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn restore_env_var(key: &str, value: Option<String>) {
    unsafe {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}

#[test]
fn project_flag_wins() {
    let cwd = PathBuf::from("/tmp/example");
    assert_eq!(
        resolve_project_slug(Some("override".to_string()), &cwd).unwrap(),
        "override"
    );
}

#[test]
fn project_defaults_to_cwd_name() {
    let cwd = PathBuf::from("/tmp/memory");
    assert_eq!(resolve_project_slug(None, &cwd).unwrap(), "memory");
}

#[test]
fn project_defaults_to_repo_metadata_when_present() {
    let repo_root = unique_temp_dir("mem-cli-project-slug");
    fs::create_dir_all(repo_root.join(".mem")).unwrap();
    fs::write(
        repo_root.join(".mem").join("project.toml"),
        "slug = \"sctp\"\nrepo_root = \"/tmp/sctp-conformance\"\n",
    )
    .unwrap();

    assert_eq!(resolve_project_slug(None, &repo_root).unwrap(), "sctp");

    let _ = fs::remove_dir_all(repo_root);
}

#[test]
fn bundle_import_rejects_preview_flag() {
    let result = Cli::try_parse_from([
        "memory",
        "bundle",
        "import",
        "--project",
        "memory",
        "bundle.zip",
        "--preview",
    ]);
    assert!(result.is_err());
}

#[test]
fn init_rejects_print_flag() {
    let result = Cli::try_parse_from(["memory", "init", "--print"]);
    assert!(result.is_err());
}

fn rendered_help(args: &[&str]) -> String {
    let err = Cli::try_parse_from(args).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::DisplayHelp);
    err.to_string()
}

fn assert_command_metadata(cmd: &Command, path: &str) {
    if cmd.get_name() != "help" {
        let about = cmd
            .get_about()
            .map(|value| value.to_string())
            .or_else(|| cmd.get_long_about().map(|value| value.to_string()));
        assert!(
            about
                .as_deref()
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false),
            "command {path} is missing help metadata"
        );
        let after_help = cmd
            .get_after_help()
            .map(|value| value.to_string())
            .or_else(|| cmd.get_after_long_help().map(|value| value.to_string()));
        assert!(
            after_help
                .as_deref()
                .map(|value| value.contains("Agent notes:") || value.contains("Agent contract:"))
                .unwrap_or(false),
            "command {path} is missing agent-oriented after_help"
        );
    }
    for subcommand in cmd.get_subcommands() {
        let sub_path = if path.is_empty() {
            subcommand.get_name().to_string()
        } else {
            format!("{path} {}", subcommand.get_name())
        };
        assert_command_metadata(subcommand, &sub_path);
    }
}

#[test]
fn all_public_commands_have_help_metadata() {
    let command = Cli::command();
    assert_command_metadata(&command, "memory");
}

fn repo_root_for_tests() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn root_command_doc_name(command_name: &str) -> Option<&'static str> {
    match command_name {
        "help" => None,
        "bundle" => Some("bundles.md"),
        "watcher" => Some("watchers.md"),
        "wizard" => Some("wizard.md"),
        "init" => Some("init.md"),
        "upgrade" => Some("upgrade.md"),
        "service" => Some("service.md"),
        "mcp" => Some("mcp.md"),
        "doctor" => Some("doctor.md"),
        "status" => Some("status.md"),
        "commits" => Some("commits.md"),
        "repo" => Some("repo.md"),
        "graph" => Some("graph.md"),
        "checkpoint" => Some("checkpoint.md"),
        "resume" => Some("resume.md"),
        "activities" => Some("activities.md"),
        "up-to-speed" => Some("up-to-speed.md"),
        "eval" => Some("eval.md"),
        "query" => Some("query.md"),
        "verify-provenance" => Some("verify-provenance.md"),
        "history" => Some("history.md"),
        "prune-history" => Some("prune-history.md"),
        "scan" => Some("scan.md"),
        "capture" => Some("capture.md"),
        "remember" => Some("remember.md"),
        "curate" => Some("curate.md"),
        "proposals" => Some("proposals.md"),
        "embeddings" => Some("embeddings.md"),
        "health" => Some("health.md"),
        "stats" => Some("stats.md"),
        "archive" => Some("archive.md"),
        "automation" => Some("automation.md"),
        "tui" => Some("tui.md"),
        "completion" => Some("completion.md"),
        "dev" => Some("dev.md"),
        other => panic!("root command {other} must declare a docs mapping"),
    }
}

#[test]
fn root_commands_have_user_cli_reference_pages() {
    let docs_dir = repo_root_for_tests().join("docs").join("user").join("cli");
    for command in Cli::command().get_subcommands() {
        let Some(doc_name) = root_command_doc_name(command.get_name()) else {
            continue;
        };
        assert!(
            docs_dir.join(doc_name).is_file(),
            "missing docs/user/cli/{doc_name} for root command {}",
            command.get_name()
        );
    }
}

fn collect_markdown_files(root: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_markdown_files(&path, files);
        } else if path.extension().and_then(|value| value.to_str()) == Some("md") {
            files.push(path);
        }
    }
}

fn markdown_link_targets(content: &str) -> Vec<String> {
    let mut links = Vec::new();
    let bytes = content.as_bytes();
    let mut cursor = 0;
    while let Some(open_rel) = content[cursor..].find("](") {
        let target_start = cursor + open_rel + 2;
        if target_start >= bytes.len() {
            break;
        }
        let Some(close_rel) = content[target_start..].find(')') else {
            break;
        };
        let target = &content[target_start..target_start + close_rel];
        links.push(target.to_string());
        cursor = target_start + close_rel + 1;
    }
    links
}

#[test]
fn markdown_local_links_and_images_resolve() {
    let repo_root = repo_root_for_tests();
    let mut markdown_files = Vec::new();
    collect_markdown_files(&repo_root.join("docs"), &mut markdown_files);
    markdown_files.push(repo_root.join("README.md"));

    for file in markdown_files {
        let content = fs::read_to_string(&file).unwrap();
        assert!(
            !content.contains("/home/olivier/"),
            "{} contains a machine-local absolute path",
            file.display()
        );
        for raw_target in markdown_link_targets(&content) {
            let target = raw_target.trim();
            if target.is_empty()
                || target.starts_with('#')
                || target.starts_with("http://")
                || target.starts_with("https://")
                || target.starts_with("mailto:")
                || target.contains("://")
            {
                continue;
            }
            let path_part = target.split('#').next().unwrap_or_default();
            if path_part.is_empty() {
                continue;
            }
            let resolved = file.parent().unwrap().join(path_part);
            assert!(
                resolved.exists(),
                "{} links to missing target {}",
                file.display(),
                target
            );
        }
    }
}

fn rendered_completion(shell: clap_complete::Shell) -> String {
    let mut command = Cli::command();
    let mut output = Vec::new();
    clap_complete::generate(shell, &mut command, "memory", &mut output);
    String::from_utf8(output).unwrap()
}

#[test]
fn completions_include_root_and_nested_commands() {
    let bash = rendered_completion(clap_complete::Shell::Bash);
    assert!(bash.contains("wizard"));
    assert!(bash.contains("completion"));
    assert!(bash.contains("proposals"));
    assert!(bash.contains("watcher"));
    assert!(bash.contains("manager"));
    assert!(bash.contains("--project"));

    let zsh = rendered_completion(clap_complete::Shell::Zsh);
    assert!(zsh.contains("_memory"));
    assert!(zsh.contains("proposals"));
    assert!(zsh.contains("watcher"));
    assert!(zsh.contains("manager"));
    assert!(zsh.contains("completion"));

    let fish = rendered_completion(clap_complete::Shell::Fish);
    assert!(fish.contains("complete -c memory"));
    assert!(fish.contains("proposals"));
    assert!(fish.contains("watcher"));
    assert!(fish.contains("manager"));
    assert!(fish.contains("completion"));
}

#[test]
fn root_help_includes_examples_and_docs_hint() {
    let output = rendered_help(&["memory", "--help"]);
    assert!(output.contains("Project memory CLI"));
    assert!(output.contains("Agent contract:"));
    assert!(output.contains("Prefer --json"));
    assert!(output.contains("checkpoint start-execution"));
    assert!(output.contains("checkpoint start-task"));
    assert!(output.contains("checkpoint finish-execution"));
    assert!(output.contains("memory status --project memory"));
    assert!(output.contains("Examples:"));
    assert!(output.contains("docs/user/README.md"));
    assert!(output.contains("Ask a project-specific question against curated memory"));
}

#[test]
fn grouped_help_includes_subcommand_descriptions() {
    let output = rendered_help(&["memory", "service", "--help"]);
    assert!(output.contains("Manage the Memory Layer backend service"));
    assert!(output.contains("Run the backend service in the foreground"));
    assert!(output.contains("Restart active Memory Layer services after an install or upgrade"));
    assert!(output.contains("Agent notes:"));
    assert!(output.contains("browser web UI"));
    assert!(output.contains("127.0.0.1:4250"));
    assert!(output.contains("Examples:"));
    assert!(output.contains("docs/user/cli/service.md"));
}

#[test]
fn leaf_help_includes_flag_help_examples_and_docs_hint() {
    let output = rendered_help(&["memory", "checkpoint", "start-execution", "--help"]);
    assert!(output.contains("Save a checkpoint and record the approved execution plan"));
    assert!(output.contains("Read the approved plan markdown from a file"));
    assert!(output.contains("Agent notes:"));
    assert!(output.contains("approved plan moves into implementation"));
    assert!(output.contains("Examples:"));
    assert!(output.contains("docs/user/cli/checkpoint.md"));
}

#[test]
fn start_task_help_includes_agent_workflow_guidance() {
    let output = rendered_help(&["memory", "checkpoint", "start-task", "--help"]);
    assert!(output.contains("Record a direct no-plan task"));
    assert!(output.contains("Original user instruction"));
    assert!(output.contains("direct no-plan task"));
    assert!(output.contains("--dry-run --json"));
    assert!(output.contains("docs/user/cli/checkpoint.md"));
}

#[test]
fn query_help_includes_argument_descriptions() {
    let output = rendered_help(&["memory", "query", "--help"]);
    assert!(output.contains("Natural-language question to answer from project memory"));
    assert!(output.contains("Restrict results to one or more memory types"));
    assert!(output.contains("Use before answering project-specific questions"));
    assert!(output.contains("insufficient_evidence"));
    assert!(output.contains("docs/user/cli/query.md"));
}

#[test]
fn verify_provenance_help_includes_dry_run_guidance() {
    let output = rendered_help(&["memory", "verify-provenance", "--help"]);
    assert!(output.contains("Verify memory source provenance against the filesystem"));
    assert!(output.contains("Repository root used to resolve relative source paths"));
    assert!(output.contains("--dry-run --json"));
    assert!(output.contains("docs/user/cli/verify-provenance.md"));
}

#[test]
fn status_help_defines_agent_output_contract() {
    let output = rendered_help(&["memory", "status", "--help"]);
    assert!(output.contains("Show the aggregate Memory Layer diagnostic status"));
    assert!(output.contains("Recommended first diagnostic command"));
    assert!(output.contains("aggregate payload on stdout"));
    assert!(output.contains("warnings must go to stderr"));
    assert!(output.contains("docs/user/cli/status.md"));
}

#[test]
fn eval_shell_suites_require_explicit_allow_shell() {
    let suite = mem_eval::EvalSuite {
        manifest: mem_eval::EvalSuiteManifest {
            name: "shell suite".to_string(),
            description: None,
            suite_version: None,
            label_status: None,
            fixture: None,
            default_profile: None,
            min_items: None,
            default_repeats: None,
            project: Some("memory".to_string()),
            items: "items.jsonl".to_string(),
        },
        root: PathBuf::from("."),
        items: vec![mem_eval::EvalItem::CommandTask(mem_eval::CommandTaskItem {
            id: "cmd".to_string(),
            metadata: mem_eval::EvalItemMetadata::default(),
            project: None,
            prompt: "Run command".to_string(),
            command: "echo ok".to_string(),
            expected_exit_code: 0,
        })],
    };

    let error = ensure_eval_shell_allowed(&suite, false, false).unwrap_err();
    assert!(error.to_string().contains("--allow-shell"));
    assert!(ensure_eval_shell_allowed(&suite, true, false).is_ok());
    assert!(ensure_eval_shell_allowed(&suite, false, true).is_ok());
}

#[test]
fn eval_retriever_cmd_requires_explicit_allow_shell() {
    let error = ensure_eval_retriever_allowed(Some("./retriever"), false).unwrap_err();

    assert!(error.to_string().contains("--allow-shell"));
    assert!(ensure_eval_retriever_allowed(Some("./retriever"), true).is_ok());
    assert!(ensure_eval_retriever_allowed(None, false).is_ok());
}

#[test]
fn external_retriever_response_normalizes_to_query_response() {
    let response: ExternalRetrieverResponse = serde_json::from_str(
        r#"{
            "schema_version": 1,
            "results": [
                {
                    "id": "external-release-rule",
                    "score": 0.82,
                    "text": "The release gate requires a green gate and paired benchmark.",
                    "tags": ["mi-release"],
                    "citations": [{"file_path": "docs/release-gate.md", "excerpt": "green gate"}]
                }
            ],
            "diagnostics": {"latency_ms": 123, "tokens_in": 50, "tokens_out": 20}
        }"#,
    )
    .unwrap();

    let normalized = external_retriever_response_to_query_response(response, 999).unwrap();

    assert_eq!(normalized.results.len(), 1);
    assert_eq!(normalized.results[0].score, 0.82);
    assert_eq!(normalized.results[0].tags, vec!["mi-release"]);
    assert_eq!(
        normalized.results[0].sources[0].file_path.as_deref(),
        Some("docs/release-gate.md")
    );
    assert!(normalized.answer.contains("green gate"));
    assert_eq!(normalized.diagnostics.total_duration_ms, 123);
    assert_eq!(
        normalized
            .answer_generation
            .token_usage
            .unwrap()
            .total_tokens,
        70
    );
}

#[test]
fn external_retriever_fake_script_scores_with_existing_retrieval_scorer() {
    let dir = unique_temp_dir("mem-external-retriever");
    fs::create_dir_all(&dir).unwrap();
    let script = dir.join("retriever.sh");
    fs::write(
        &script,
        r#"#!/usr/bin/env sh
cat > request.json
printf '%s\n' '{"schema_version":1,"results":[{"id":"external-release-rule","score":0.92,"text":"The release rule requires a green gate and paired benchmark.","tags":["mi-release"],"citations":["docs/release-gate.md"]}],"diagnostics":{"latency_ms":12,"tokens_in":3,"tokens_out":4}}'
"#,
    )
    .unwrap();
    let suite = mem_eval::EvalSuite {
        manifest: mem_eval::EvalSuiteManifest {
            name: "external suite".to_string(),
            description: None,
            suite_version: None,
            label_status: None,
            fixture: Some("fixtures/research".to_string()),
            default_profile: None,
            min_items: None,
            default_repeats: None,
            project: Some("memory".to_string()),
            items: "items.jsonl".to_string(),
        },
        root: dir.clone(),
        items: Vec::new(),
    };
    let item = mem_eval::RetrievalQaItem {
        id: "rq-release-rule".to_string(),
        metadata: mem_eval::EvalItemMetadata::default(),
        project: Some("memory".to_string()),
        question: "What release rule applies?".to_string(),
        top_k: 8,
        hidden_facts: vec!["The hidden gate is green.".to_string()],
        expected_memory_ids: Vec::new(),
        expected_tags: vec!["mi-release".to_string()],
        expected_files: vec!["docs/release-gate.md".to_string()],
    };
    let context = EvalRunContext {
        profile: mem_eval::EvalProfile::Offline,
        repeat_index: 0,
        run_group_id: Uuid::nil(),
        suite_checksum: None,
        dry_run: false,
        artifacts_root: dir.join("artifacts"),
        memory_command: "/tmp/memory".to_string(),
        memory_base_url: "http://127.0.0.1:4250".to_string(),
        memory_config_path: None,
        llm_judge: false,
        retriever_cmd: Some(format!("sh {}", script.display())),
        command_cwd: dir.clone(),
    };

    let response = run_external_retriever_for_retrieval_item(
        &suite,
        &item,
        "memory",
        mem_eval::EvalCondition::FullMemory,
        &context,
    )
    .unwrap();
    let result =
        mem_eval::score_retrieval_qa(&item, mem_eval::EvalCondition::FullMemory, &response);
    let request = fs::read_to_string(dir.join("request.json")).unwrap();

    assert!(result.success, "{result:?}");
    assert_eq!(result.scores["tag_recall_at_k"], 1.0);
    assert_eq!(result.scores["file_recall_at_k"], 1.0);
    assert!(request.contains("\"item_id\":\"rq-release-rule\""));
    assert!(request.contains("\"hidden_facts\":[\"The hidden gate is green.\"]"));

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn external_retriever_failures_become_failed_eval_results() {
    let dir = unique_temp_dir("mem-external-retriever-failure");
    fs::create_dir_all(&dir).unwrap();
    let context = EvalRunContext {
        profile: mem_eval::EvalProfile::Offline,
        repeat_index: 0,
        run_group_id: Uuid::nil(),
        suite_checksum: None,
        dry_run: false,
        artifacts_root: dir.join("artifacts"),
        memory_command: "/tmp/memory".to_string(),
        memory_base_url: "http://127.0.0.1:4250".to_string(),
        memory_config_path: None,
        llm_judge: false,
        retriever_cmd: Some("printf 'not-json'".to_string()),
        command_cwd: dir.clone(),
    };
    let suite = mem_eval::EvalSuite {
        manifest: mem_eval::EvalSuiteManifest {
            name: "external suite".to_string(),
            description: None,
            suite_version: None,
            label_status: None,
            fixture: None,
            default_profile: None,
            min_items: None,
            default_repeats: None,
            project: Some("memory".to_string()),
            items: "items.jsonl".to_string(),
        },
        root: dir.clone(),
        items: Vec::new(),
    };
    let request = build_external_retriever_request(
        &suite,
        "rq",
        "memory",
        "question?",
        8,
        &[],
        mem_eval::EvalCondition::FullMemory,
    );

    let error = run_external_retriever(&context, request).unwrap_err();
    let result = external_retriever_failure_result(
        "rq".to_string(),
        "retrieval_qa",
        mem_eval::EvalItemMetadata::default(),
        mem_eval::EvalCondition::FullMemory,
        error,
    );

    assert!(!result.success);
    assert!(result.notes[0].contains("external retriever failed"));
    assert_eq!(result.scores["external_retriever_success"], 0.0);

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn proposals_help_includes_review_guidance() {
    let output = rendered_help(&["memory", "proposals", "--help"]);
    assert!(output.contains("Review pending memory replacement proposals"));
    assert!(output.contains("Approve/reject mutate memory state"));
    assert!(output.contains("docs/user/cli/proposals.md"));
}

#[test]
fn proposals_show_parses_uuid_and_json_flag() {
    let cli = Cli::parse_from([
        "memory",
        "proposals",
        "show",
        "--project",
        "memory",
        "--id",
        "00000000-0000-0000-0000-000000000000",
        "--json",
    ]);
    let super::Command::Proposals(args) = cli.command else {
        panic!("expected proposals command");
    };
    let super::ProposalsCommand::Show(args) = args.command else {
        panic!("expected proposals show command");
    };
    assert_eq!(args.project, "memory");
    assert_eq!(args.id, Uuid::nil());
    assert!(args.json);
}

#[cfg(not(target_os = "macos"))]
#[test]
fn systemd_unit_parser_finds_memory_watch_services() {
    let units = parse_systemd_unit_names(
        "memory-watch-manager.service loaded active running Manager\n\
             memory-watch-codex-abc.service loaded active running Watcher\n\
             ssh.service loaded active running SSH\n",
    );

    assert_eq!(
        units,
        vec![
            "memory-watch-manager.service".to_string(),
            "memory-watch-codex-abc.service".to_string()
        ]
    );
}

#[test]
fn restart_notice_detects_newer_or_different_marker() {
    let dir = std::env::temp_dir().join(format!("memory-tui-restart-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let marker_path = dir.join("tui-restart-required.json");
    let startup_at = chrono::Utc::now();
    let marker = TuiRestartMarker {
        version: "9.9.9".to_string(),
        marked_at: startup_at - chrono::Duration::seconds(30),
        reason: "install-or-upgrade".to_string(),
        binary_path: "memory".to_string(),
        restarted_services: vec!["memory-layer.service".to_string()],
    };
    fs::write(&marker_path, serde_json::to_string(&marker).unwrap()).unwrap();

    let notice = newest_tui_restart_notice(startup_at, "0.1.0", vec![marker_path.clone()]).unwrap();

    assert_eq!(notice.marker_path, marker_path);
    assert_eq!(notice.version, "9.9.9");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn restart_notice_ignores_prod_marker_for_dev_tui() {
    let dir = std::env::temp_dir().join(format!(
        "memory-tui-restart-dev-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let marker_path = dir.join("tui-restart-required.json");
    let startup_at = chrono::Utc::now();
    let marker = TuiRestartMarker {
        version: "0.8.2".to_string(),
        marked_at: startup_at + chrono::Duration::seconds(30),
        reason: "install-or-upgrade".to_string(),
        binary_path: "memory".to_string(),
        restarted_services: vec!["memory-layer.service".to_string()],
    };
    fs::write(&marker_path, serde_json::to_string(&marker).unwrap()).unwrap();

    let notice = newest_tui_restart_notice(startup_at, "0.8.2-dev", vec![marker_path.clone()]);

    assert!(notice.is_none());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn packaging_hooks_restart_services_and_mark_tui_restart() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap();
    let postinst = fs::read_to_string(workspace.join("packaging/debian/postinst")).unwrap();
    let pkg = fs::read_to_string(workspace.join("packaging/build-pkg.sh")).unwrap();
    let formula = fs::read_to_string(workspace.join("Formula/memory-layer.rb")).unwrap();

    for contents in [postinst, pkg, formula] {
        assert!(contents.contains("service restart-all"));
        assert!(contents.contains("--mark-tui-restart"));
    }
}

#[test]
fn packaging_installs_shell_completions() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap();
    let local = fs::read_to_string(workspace.join("scripts/install-local.sh")).unwrap();
    let deb = fs::read_to_string(workspace.join("packaging/build-deb.sh")).unwrap();
    let pkg = fs::read_to_string(workspace.join("packaging/build-pkg.sh")).unwrap();
    let formula = fs::read_to_string(workspace.join("Formula/memory-layer.rb")).unwrap();

    for contents in [local, deb, pkg, formula] {
        assert!(contents.contains("completion\", \"bash") || contents.contains("completion bash"));
        assert!(contents.contains("completion\", \"zsh") || contents.contains("completion zsh"));
        assert!(contents.contains("completion\", \"fish") || contents.contains("completion fish"));
        assert!(contents.contains("bash-completion") || contents.contains("bash_completion"));
        assert!(contents.contains("_memory"));
        assert!(contents.contains("memory.fish"));
    }
}

#[test]
fn agent_critical_help_mentions_current_surfaces() {
    let resume = rendered_help(&["memory", "resume", "--help"]);
    assert!(resume.contains("after interruptions"));
    assert!(resume.contains("--json"));

    let remember = rendered_help(&["memory", "remember", "--help"]);
    assert!(remember.contains("after meaningful completed work"));
    assert!(remember.contains("--file-changed"));

    let dev = rendered_help(&["memory", "dev", "init", "--help"]);
    assert!(dev.contains("127.0.0.1:4250"));
    assert!(dev.contains("--copy-from-global"));
    assert!(dev.contains("docs/developer/dev-stack.md"));

    let watcher_manager = rendered_help(&["memory", "watcher", "manager", "--help"]);
    assert!(watcher_manager.contains("Preferred automation surface"));
    assert!(watcher_manager.contains("one watcher per repo/session"));

    let embeddings_list = rendered_help(&["memory", "embeddings", "list", "--help"]);
    assert!(embeddings_list.contains("Active backends are marked with *"));
    assert!(embeddings_list.contains("avoid guessing backend names"));

    let embeddings_activate = rendered_help(&["memory", "embeddings", "activate", "--help"]);
    assert!(embeddings_activate.contains("does not recompute embeddings"));
    assert!(embeddings_activate.contains("memory embeddings list"));

    let history = rendered_help(&["memory", "prune-history", "--help"]);
    assert!(history.contains("Always use --dry-run and --json first"));
}

#[test]
fn watcher_manager_run_requires_config_load() {
    let command = WatcherCommand::Manager(WatcherManagerArgs {
        command: WatcherManagerCommand::Run,
    });
    assert!(watcher_command_requires_config_load(&command));
    let status = WatcherCommand::Manager(WatcherManagerArgs {
        command: WatcherManagerCommand::Status,
    });
    assert!(!watcher_command_requires_config_load(&status));
}

#[test]
fn remember_request_uses_defaults() {
    let request = build_remember_request(
        RememberArgs {
            project: None,
            title: None,
            memory_type: None,
            prompt: None,
            summary: None,
            notes: vec!["durable fact".to_string()],
            files_changed: vec!["src/main.rs".to_string()],
            tests_passed: vec![],
            tests_failed: vec![],
            command_output_file: None,
            auto_files: false,
            dry_run: false,
        },
        "memory",
        "codex-writer",
        Some("Codex"),
    )
    .unwrap();

    assert_eq!(request.task_title, "Memory update for memory");
    assert_eq!(request.writer_id, "codex-writer");
    assert!(request.user_prompt.contains("Auto-captured"));
    assert!(request.agent_summary.contains("src/main.rs"));
    assert_eq!(request.structured_candidates.len(), 1);
    assert_eq!(
        request.structured_candidates[0].memory_type,
        mem_api::MemoryType::Implementation
    );
}

#[test]
fn remember_accepts_file_alias_for_provenance() {
    let args = Cli::try_parse_from([
        "memory",
        "remember",
        "--project",
        "memory",
        "--note",
        "query match types",
        "--file",
        "crates/mem-search/src/lib.rs",
    ])
    .unwrap();

    let super::Command::Remember(args) = args.command else {
        panic!("expected remember command");
    };
    assert_eq!(args.files_changed, vec!["crates/mem-search/src/lib.rs"]);
}

#[test]
fn derive_plan_title_prefers_markdown_heading() {
    let title = derive_plan_title(None, "# Resume Redesign\n\n- step", "memory");
    assert_eq!(title, "Resume Redesign");
}

#[test]
fn derive_plan_thread_key_sanitizes_title() {
    let thread_key = derive_plan_thread_key(None, "Resume Redesign!", "memory");
    assert_eq!(thread_key, "resume-redesign");
}

#[test]
fn plan_execution_request_uses_plan_type_and_thread_tag() {
    let writer = WriterIdentity {
        id: "writer".to_string(),
        name: Some("Writer".to_string()),
    };
    let request = build_plan_execution_request(
        "memory",
        &writer,
        "Resume Redesign",
        "resume-redesign",
        "# Resume Redesign\n\n- step",
        None,
        Path::new("/tmp/memory"),
        Some("abc123"),
    );

    assert_eq!(request.task_title, "Approved plan: Resume Redesign");
    assert_eq!(request.structured_candidates.len(), 1);
    let candidate = &request.structured_candidates[0];
    assert_eq!(candidate.memory_type, mem_api::MemoryType::Plan);
    assert!(candidate.tags.contains(&"plan".to_string()));
    assert!(
        candidate
            .tags
            .contains(&"plan-thread:resume-redesign".to_string())
    );
}

#[test]
fn newer_memory_types_parse_from_cli_args() {
    assert_eq!(
        parse_memory_type_arg("task").unwrap(),
        mem_api::MemoryType::Task
    );
    assert_eq!(
        parse_memory_type_arg("documentation").unwrap(),
        mem_api::MemoryType::Documentation
    );
    assert_eq!(
        parse_memory_type_arg("refactor").unwrap(),
        mem_api::MemoryType::Refactor
    );
}

#[test]
fn task_start_request_uses_task_type_and_prompt_source() {
    let writer = WriterIdentity {
        id: "writer".to_string(),
        name: Some("Writer".to_string()),
    };
    let request = build_task_start_request(
        "memory",
        &writer,
        "Fix query input",
        "Make the query input easier to use.",
        "fix-query-input",
        Some("abc123"),
    );

    assert_eq!(request.task_title, "Task started: Fix query input");
    assert_eq!(request.user_prompt, "Make the query input easier to use.");
    assert_eq!(request.structured_candidates.len(), 1);
    assert!(
        request
            .idempotency_key
            .as_deref()
            .is_some_and(|key| key.starts_with("task-start:"))
    );
    let candidate = &request.structured_candidates[0];
    assert_eq!(candidate.memory_type, mem_api::MemoryType::Task);
    assert!(candidate.canonical_text.contains("# Task: Fix query input"));
    assert!(candidate.canonical_text.contains("Status: started"));
    assert!(candidate.canonical_text.contains("Git head: abc123"));
    assert!(candidate.tags.contains(&"task".to_string()));
    assert!(
        candidate
            .tags
            .contains(&"task-thread:fix-query-input".to_string())
    );
    assert!(candidate.tags.contains(&"direct-execution".to_string()));
    assert!(candidate.tags.contains(&"no-approved-plan".to_string()));
    assert!(candidate.sources.iter().any(|source| {
        source.source_kind == mem_api::SourceKind::TaskPrompt
            && source
                .excerpt
                .as_deref()
                .is_some_and(|excerpt| excerpt.contains("query input"))
    }));
}

#[test]
fn graph_activity_request_copies_extraction_report_counts() {
    let run_id = Uuid::new_v4();
    let report = mem_graph::GraphExtractionReport {
        project: "memory".to_string(),
        repo_root: "/repo".to_string(),
        git_head: Some("abc123".to_string()),
        since: Some("HEAD~1".to_string()),
        analyzer_version: "mem-analyze-v2".to_string(),
        strategy_version: "code-graph-resolution-v1".to_string(),
        extraction_run_id: Some(run_id),
        reused_existing_run: true,
        dry_run: false,
        index_reused: true,
        symbol_count: 10,
        reference_count: 20,
        resolved_reference_count: 12,
        unresolved_reference_count: 7,
        ambiguous_reference_count: 1,
        graph_node_count: 10,
        graph_edge_count: 9,
        evidence_count: 19,
        sample_unresolved_references: Vec::new(),
    };

    let request = build_graph_activity_request(&report);

    assert_eq!(request.project, "memory");
    assert_eq!(request.extraction_run_id, Some(run_id));
    assert!(request.reused_existing_run);
    assert_eq!(request.reference_count, 20);
    assert_eq!(request.graph_edge_count, 9);
    assert!(request.validate().is_ok());
}

#[test]
fn durable_plan_source_path_keeps_repo_files_and_drops_outside_paths() {
    let repo_root = unique_temp_dir("mem-plan-source-repo");
    let plans_dir = repo_root.join("plans");
    fs::create_dir_all(&plans_dir).unwrap();
    let repo_plan = plans_dir.join("approved-plan.md");
    fs::write(&repo_plan, "# Plan\n\n- [ ] step").unwrap();

    let outside_root = unique_temp_dir("mem-plan-source-outside");
    fs::create_dir_all(&outside_root).unwrap();
    let outside_plan = outside_root.join("approved-plan.md");
    fs::write(&outside_plan, "# Plan\n\n- [ ] step").unwrap();

    assert_eq!(
        durable_plan_source_path(&repo_plan, &repo_root),
        Some(fs::canonicalize(&repo_plan).unwrap())
    );
    assert_eq!(durable_plan_source_path(&outside_plan, &repo_root), None);

    let _ = fs::remove_dir_all(repo_root);
    let _ = fs::remove_dir_all(outside_root);
}

#[test]
fn plan_execution_request_omits_outside_repo_plan_file_source() {
    let repo_root = unique_temp_dir("mem-plan-request-repo");
    fs::create_dir_all(&repo_root).unwrap();
    let outside_root = unique_temp_dir("mem-plan-request-outside");
    fs::create_dir_all(&outside_root).unwrap();
    let outside_plan = outside_root.join("approved-plan.md");
    fs::write(&outside_plan, "# Plan\n\n- [ ] step").unwrap();
    let writer = WriterIdentity {
        id: "writer".to_string(),
        name: Some("Writer".to_string()),
    };

    let request = build_plan_execution_request(
        "memory",
        &writer,
        "Resume Redesign",
        "resume-redesign",
        "# Resume Redesign\n\n- [ ] step",
        Some(outside_plan.as_path()),
        &repo_root,
        Some("abc123"),
    );

    assert!(
        !request.structured_candidates[0]
            .sources
            .iter()
            .any(|source| source.source_kind == mem_api::SourceKind::File)
    );

    let _ = fs::remove_dir_all(repo_root);
    let _ = fs::remove_dir_all(outside_root);
}

#[test]
fn parse_plan_checkboxes_extracts_checked_and_unchecked_items() {
    let items = parse_plan_checkboxes(
        "# Plan\n\n- [ ] first task\n* [x] second task\n+ [X] third task\nplain bullet",
    );
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].text, "first task");
    assert!(!items[0].checked);
    assert_eq!(items[1].text, "second task");
    assert!(items[1].checked);
    assert_eq!(items[2].text, "third task");
    assert!(items[2].checked);
}

#[test]
fn no_memory_grounded_answer_parser_accepts_strict_json() {
    let (answer, confidence, notes) = parse_no_memory_grounded_answer(
        "```json\n{\"answer\":\"Token usage is reported.\",\"confidence\":1.4}\n```",
    );

    assert_eq!(answer, "Token usage is reported.");
    assert_eq!(confidence, Some(1.0));
    assert!(notes.is_empty());
}

#[test]
fn no_memory_grounded_answer_parser_falls_back_to_text() {
    let (answer, confidence, notes) = parse_no_memory_grounded_answer("I do not know.");

    assert_eq!(answer, "I do not know.");
    assert_eq!(confidence, None);
    assert_eq!(
        notes,
        vec!["plain_llm response was not strict answer/confidence JSON".to_string()]
    );
}

#[test]
fn token_usage_from_chat_body_supports_openai_and_cache_fields() {
    let usage = token_usage_from_chat_body(
        r#"{
                "choices":[{"message":{"content":"ok"}}],
                "usage":{
                    "prompt_tokens":10,
                    "completion_tokens":5,
                    "cached_input_tokens":2,
                    "cache_creation_input_tokens":3
                }
            }"#,
    )
    .unwrap();

    assert_eq!(usage.input_tokens, 10);
    assert_eq!(usage.output_tokens, 5);
    assert_eq!(usage.cache_read_tokens, 2);
    assert_eq!(usage.cache_write_tokens, 3);
    assert_eq!(usage.total_tokens, 20);
}

#[test]
fn token_usage_from_json_value_supports_codex_nested_events() {
    let value: serde_json::Value = serde_json::from_str(
        r#"{
                "type":"token_count",
                "msg":{
                    "total_token_usage":{
                        "input_tokens":100,
                        "output_tokens":25,
                        "cached_input_tokens":10,
                        "total_tokens":135
                    }
                }
            }"#,
    )
    .unwrap();

    let usage = token_usage_from_json_value(&value).unwrap();

    assert_eq!(usage.input_tokens, 100);
    assert_eq!(usage.output_tokens, 25);
    assert_eq!(usage.cache_read_tokens, 10);
    assert_eq!(usage.total_tokens, 135);
}

#[test]
fn chat_completion_content_rejects_missing_content() {
    let error = chat_completion_content(r#"{"choices":[{"message":{}}]}"#)
        .expect_err("missing content should fail");

    assert!(error.to_string().contains("missing content"));
}

#[test]
fn agent_build_prompt_adds_memory_questions_for_memory_conditions() {
    let item = mem_eval::AgentBuildTaskItem {
        id: "build".to_string(),
        metadata: mem_eval::EvalItemMetadata::default(),
        project: Some("memory".to_string()),
        prompt: "Build the app.".to_string(),
        fixture: "fixtures/app".to_string(),
        agent_command: "codex exec - < {prompt_file}".to_string(),
        memory_questions: vec!["What changed recently?".to_string()],
        setup_commands: Vec::new(),
        score_commands: Vec::new(),
        timeout_seconds: 60,
        required_files: Vec::new(),
        forbidden_files: Vec::new(),
        required_content: Vec::new(),
    };
    let context = EvalRunContext {
        profile: mem_eval::EvalProfile::Llm,
        repeat_index: 0,
        run_group_id: Uuid::nil(),
        suite_checksum: None,
        dry_run: false,
        artifacts_root: PathBuf::from("target/memory-evals"),
        memory_command: "/tmp/memory".to_string(),
        memory_base_url: "http://127.0.0.1:4250".to_string(),
        memory_config_path: Some(PathBuf::from(".mem/config.toml")),
        llm_judge: false,
        retriever_cmd: None,
        command_cwd: PathBuf::from("."),
    };

    let prompt = agent_build_prompt(&item, mem_eval::EvalCondition::FullMemory, &context);

    assert!(prompt.contains("memory-enabled"));
    assert!(prompt.contains("/tmp/memory"));
    assert!(prompt.contains("What changed recently?"));
    assert!(prompt.contains("memory-evidence.md"));
}

#[test]
fn agent_build_prompt_forbids_memory_for_no_memory_condition() {
    let item = mem_eval::AgentBuildTaskItem {
        id: "build".to_string(),
        metadata: mem_eval::EvalItemMetadata::default(),
        project: Some("memory".to_string()),
        prompt: "Build the app.".to_string(),
        fixture: "fixtures/app".to_string(),
        agent_command: "codex exec - < {prompt_file}".to_string(),
        memory_questions: vec!["What changed recently?".to_string()],
        setup_commands: Vec::new(),
        score_commands: Vec::new(),
        timeout_seconds: 60,
        required_files: Vec::new(),
        forbidden_files: Vec::new(),
        required_content: Vec::new(),
    };
    let context = EvalRunContext {
        profile: mem_eval::EvalProfile::Llm,
        repeat_index: 0,
        run_group_id: Uuid::nil(),
        suite_checksum: None,
        dry_run: false,
        artifacts_root: PathBuf::from("target/memory-evals"),
        memory_command: "/tmp/memory".to_string(),
        memory_base_url: "http://127.0.0.1:4250".to_string(),
        memory_config_path: None,
        llm_judge: false,
        retriever_cmd: None,
        command_cwd: PathBuf::from("."),
    };

    let prompt = agent_build_prompt(&item, mem_eval::EvalCondition::NoMemory, &context);

    assert!(prompt.contains("no-memory"));
    assert!(prompt.contains("Do not query"));
    assert!(prompt.contains("Do not create memory-evidence.md"));
    assert!(prompt.contains(".memory-eval"));
    assert!(!prompt.contains("What changed recently?"));
}

#[test]
fn agent_build_memory_evidence_verifies_required_queries() {
    let workspace = std::env::temp_dir().join(format!("memory-eval-test-{}", Uuid::new_v4()));
    fs::create_dir_all(workspace.join(".memory-eval")).unwrap();
    fs::write(
        workspace.join(".memory-eval/q1.status.json"),
        r#"{"question_id":"q1","exit_code":0,"output_file":".memory-eval/q1.json"}"#,
    )
    .unwrap();
    fs::write(
        workspace.join(".memory-eval/q1.json"),
        r#"{"results":[{"memory_id":"00000000-0000-0000-0000-000000000000"}]}"#,
    )
    .unwrap();
    let item = mem_eval::AgentBuildTaskItem {
        id: "build".to_string(),
        metadata: mem_eval::EvalItemMetadata::default(),
        project: Some("memory".to_string()),
        prompt: "Build the app.".to_string(),
        fixture: "fixtures/app".to_string(),
        agent_command: "codex exec - < {prompt_file}".to_string(),
        memory_questions: vec!["What changed recently?".to_string()],
        setup_commands: Vec::new(),
        score_commands: Vec::new(),
        timeout_seconds: 60,
        required_files: Vec::new(),
        forbidden_files: Vec::new(),
        required_content: Vec::new(),
    };

    let evidence = validate_agent_build_memory_evidence(
        &workspace,
        &item,
        mem_eval::EvalCondition::FullMemory,
    )
    .unwrap();

    assert!(evidence.ok);
    assert_eq!(evidence.required, 1);
    assert_eq!(evidence.verified, 1);
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn agent_build_no_memory_fails_on_memory_evidence_artifacts() {
    let workspace = std::env::temp_dir().join(format!("memory-eval-test-{}", Uuid::new_v4()));
    fs::create_dir_all(&workspace).unwrap();
    fs::write(workspace.join("memory-evidence.md"), "query").unwrap();
    let item = mem_eval::AgentBuildTaskItem {
        id: "build".to_string(),
        metadata: mem_eval::EvalItemMetadata::default(),
        project: Some("memory".to_string()),
        prompt: "Build the app.".to_string(),
        fixture: "fixtures/app".to_string(),
        agent_command: "codex exec - < {prompt_file}".to_string(),
        memory_questions: vec!["What changed recently?".to_string()],
        setup_commands: Vec::new(),
        score_commands: Vec::new(),
        timeout_seconds: 60,
        required_files: Vec::new(),
        forbidden_files: Vec::new(),
        required_content: Vec::new(),
    };

    let evidence =
        validate_agent_build_memory_evidence(&workspace, &item, mem_eval::EvalCondition::NoMemory)
            .unwrap();

    assert!(!evidence.ok);
    assert!(evidence.notes[0].contains("forbidden Memory evidence"));
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn ensure_checkbox_plan_rejects_plans_without_checkboxes() {
    let items = parse_plan_checkboxes("# Plan\n\n- regular bullet");
    let error = ensure_checkbox_plan(&items).expect_err("should reject non-checkbox plan");
    assert!(
        error
            .to_string()
            .contains("approved plans must contain Markdown checkbox items")
    );
}

#[test]
fn finish_report_lists_remaining_items() {
    let report = build_plan_execution_finish_report(
        "memory",
        &mem_api::MemoryEntryResponse {
            id: Uuid::new_v4(),
            project: "memory".to_string(),
            canonical_text: "# Plan\n\n- [x] done\n- [ ] remaining".to_string(),
            summary: "Execution plan".to_string(),
            memory_type: mem_api::MemoryType::Plan,
            importance: 4,
            confidence: 0.95,
            status: mem_api::MemoryStatus::Active,
            tags: vec![
                "plan".to_string(),
                "plan-thread:resume-redesign".to_string(),
            ],
            sources: Vec::new(),
            related_memories: Vec::new(),
            embedding_spaces: Vec::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            canonical_id: Uuid::nil(),
            version_no: 1,
            is_tombstone: false,
        },
    )
    .expect("build finish report");

    assert_eq!(report.total_items, 2);
    assert_eq!(report.completed_items, 1);
    assert_eq!(report.completed_item_texts, vec!["done".to_string()]);
    assert!(!report.verified_complete);
    assert_eq!(report.remaining_items, vec!["remaining".to_string()]);
}

#[test]
fn finish_execution_implementation_request_uses_completed_items() {
    let writer = WriterIdentity {
        id: "writer".to_string(),
        name: Some("Writer".to_string()),
    };
    let report = PlanExecutionFinishReport {
        project: "memory".to_string(),
        thread_key: "footer-fix".to_string(),
        plan_title: "Footer Fix".to_string(),
        total_items: 2,
        completed_items: 2,
        completed_item_texts: vec![
            "Add implementation memory type".to_string(),
            "Record finish-execution implementation outcomes".to_string(),
        ],
        remaining_items: Vec::new(),
        verified_complete: true,
    };

    let request = build_finish_execution_implementation_request(
        "memory",
        &writer,
        &report,
        "Implemented 2 items for Footer Fix",
        &["The memories view now shows implemented outcomes.".to_string()],
        Some("abc123"),
    );

    assert_eq!(request.structured_candidates.len(), 1);
    let candidate = &request.structured_candidates[0];
    assert_eq!(candidate.memory_type, mem_api::MemoryType::Implementation);
    assert!(candidate.tags.contains(&"implemented".to_string()));
    assert!(candidate.canonical_text.contains("Implemented items:"));
    assert!(
        candidate
            .canonical_text
            .contains("Add implementation memory type")
    );
    assert!(request.idempotency_key.is_some());
}

#[test]
fn remember_request_auto_detects_refactor_type() {
    let writer_id = "writer";
    let args = RememberArgs {
        project: Some("memory".to_string()),
        title: Some("Refactor query helpers".to_string()),
        prompt: Some("Refactor query helpers with no functional change.".to_string()),
        summary: Some("Moved query helper code without behavior change.".to_string()),
        notes: vec!["Refactored crates/mem-search/src/lib.rs.".to_string()],
        files_changed: vec!["crates/mem-search/src/lib.rs".to_string()],
        auto_files: false,
        tests_passed: Vec::new(),
        tests_failed: Vec::new(),
        command_output_file: None,
        memory_type: None,
        dry_run: false,
    };

    let request = build_remember_request(args, "memory", writer_id, None).unwrap();

    assert_eq!(
        request.structured_candidates[0].memory_type,
        mem_api::MemoryType::Refactor
    );
    assert!(
        request.structured_candidates[0]
            .tags
            .contains(&"refactor".to_string())
    );
}

#[test]
fn mixed_fix_and_refactor_remember_request_stays_implementation() {
    let args = RememberArgs {
        project: Some("memory".to_string()),
        title: Some("Fix and refactor query helpers".to_string()),
        prompt: Some("Fix ranking and refactor helper layout.".to_string()),
        summary: Some("Fixed ranking while extracting helpers.".to_string()),
        notes: Vec::new(),
        files_changed: vec!["crates/mem-search/src/lib.rs".to_string()],
        auto_files: false,
        tests_passed: Vec::new(),
        tests_failed: Vec::new(),
        command_output_file: None,
        memory_type: None,
        dry_run: false,
    };

    let request = build_remember_request(args, "memory", "writer", None).unwrap();

    assert_eq!(
        request.structured_candidates[0].memory_type,
        mem_api::MemoryType::Implementation
    );
}

#[test]
fn finish_execution_request_auto_detects_refactor_type() {
    let writer = WriterIdentity {
        id: "writer".to_string(),
        name: Some("Writer".to_string()),
    };
    let report = PlanExecutionFinishReport {
        project: "memory".to_string(),
        thread_key: "query-refactor".to_string(),
        plan_title: "Refactor query helpers".to_string(),
        total_items: 1,
        completed_items: 1,
        completed_item_texts: vec!["Extract query helpers with no functional change".to_string()],
        remaining_items: Vec::new(),
        verified_complete: true,
    };

    let request = build_finish_execution_implementation_request(
        "memory",
        &writer,
        &report,
        "Refactored query helpers without behavior change",
        &[],
        Some("abc123"),
    );

    assert_eq!(
        request.structured_candidates[0].memory_type,
        mem_api::MemoryType::Refactor
    );
}

#[test]
fn writer_identity_falls_back_to_derived_value() {
    let _guard = ENV_LOCK.lock().unwrap();
    let old_user = std::env::var("MEMORY_LAYER_WRITER_IDENTITY_USER").ok();
    let old_host = std::env::var("MEMORY_LAYER_WRITER_IDENTITY_HOST").ok();
    let old_writer = std::env::var("MEMORY_LAYER_WRITER_ID").ok();
    let old_agent = std::env::var("MEMORY_LAYER_AGENT_ID").ok();

    unsafe {
        std::env::set_var("MEMORY_LAYER_WRITER_IDENTITY_USER", "cli-user");
        std::env::set_var("MEMORY_LAYER_WRITER_IDENTITY_HOST", "cli-host");
        std::env::remove_var("MEMORY_LAYER_WRITER_ID");
        std::env::remove_var("MEMORY_LAYER_AGENT_ID");
    }

    let config: AppConfig =
        toml::from_str("[service]\n\n[database]\nurl = \"postgres://example\"\n")
            .expect("parse minimal config");
    let writer = resolve_writer_identity(&config, None).expect("resolve writer identity");

    restore_env_var("MEMORY_LAYER_WRITER_IDENTITY_USER", old_user);
    restore_env_var("MEMORY_LAYER_WRITER_IDENTITY_HOST", old_host);
    restore_env_var("MEMORY_LAYER_WRITER_ID", old_writer);
    restore_env_var("MEMORY_LAYER_AGENT_ID", old_agent);

    assert_eq!(writer.id, "memory-cli-user-cli-host");
    assert_eq!(writer.name, None);
}

#[test]
fn init_print_describes_repo_layout() {
    let repo_root = PathBuf::from("/tmp/memory");
    let summary = initialize_repo(&repo_root, "memory", false, true).unwrap();

    assert!(summary.contains("user-local project config"));
    assert!(summary.contains(".mem/project.toml"));
    assert!(summary.contains(".agents/memory-layer.toml"));
    assert!(summary.contains(".agents/skills"));
    assert!(summary.contains("bundled memory skills"));
    if cfg!(target_os = "macos") {
        assert!(summary.contains("memory watcher enable --project memory"));
    } else {
        assert!(summary.contains("memory watcher manager enable"));
    }
    assert!(summary.contains("memory service run"));
}

#[test]
fn init_creates_repo_files_and_gitignore_entry() {
    let _guard = ENV_LOCK.lock().unwrap();
    let repo_root = unique_temp_dir("mem-init");
    let xdg_root = unique_temp_dir("mem-init-xdg");
    let old_config = std::env::var("XDG_CONFIG_HOME").ok();
    let old_state = std::env::var("XDG_STATE_HOME").ok();
    let old_cache = std::env::var("XDG_CACHE_HOME").ok();
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", xdg_root.join("config"));
        std::env::set_var("XDG_STATE_HOME", xdg_root.join("state"));
        std::env::set_var("XDG_CACHE_HOME", xdg_root.join("cache"));
    }
    fs::create_dir_all(&repo_root).unwrap();

    initialize_repo(&repo_root, "memory", false, false).unwrap();
    let paths = mem_platform::project_paths(&repo_root, "memory").unwrap();

    assert!(paths.config_path().is_file());
    assert!(paths.project_path().is_file());
    assert!(repo_root.join(".mem/project.toml").is_file());
    assert!(repo_root.join(".agents/memory-layer.toml").is_file());
    assert!(paths.runtime_dir().is_dir());
    assert!(
        repo_root
            .join(".agents/skills/memory-layer/SKILL.md")
            .is_file()
    );
    assert!(
        repo_root
            .join(".agents/skills/memory-query-resume/SKILL.md")
            .is_file()
    );
    assert!(
        repo_root
            .join(".agents/skills/memory-project-init/SKILL.md")
            .is_file()
    );
    assert!(
        repo_root
            .join(".agents/skills/memory-github-init/SKILL.md")
            .is_file()
    );
    assert!(
        repo_root
            .join(".agents/skills/memory-review-proposals/SKILL.md")
            .is_file()
    );
    assert!(
        repo_root
            .join(".agents/skills/memory-plan-execution/SKILL.md")
            .is_file()
    );
    assert!(
        repo_root
            .join(".agents/skills/memory-direct-task-start/SKILL.md")
            .is_file()
    );
    assert!(
        repo_root
            .join(".agents/skills/memory-remember/SKILL.md")
            .is_file()
    );
    assert!(
        repo_root
            .join(".agents/skills/memory-layer/scripts/go.mod")
            .is_file()
    );
    assert!(
        repo_root
            .join(".agents/skills/memory-layer/scripts/main.go")
            .is_file()
    );
    assert!(
        fs::read_to_string(paths.config_path())
            .unwrap()
            .contains("[automation]")
    );
    assert_eq!(
        fs::read_to_string(repo_root.join(".mem/.gitignore")).unwrap(),
        "*\n!.gitignore\n!project.toml\n"
    );
    assert!(!root_gitignore_contains_mem(&repo_root).unwrap());

    restore_env_var("XDG_CONFIG_HOME", old_config);
    restore_env_var("XDG_STATE_HOME", old_state);
    restore_env_var("XDG_CACHE_HOME", old_cache);
    let _ = fs::remove_dir_all(repo_root);
    let _ = fs::remove_dir_all(xdg_root);
}

#[test]
fn init_migrates_legacy_mem_gitignore() {
    let _guard = ENV_LOCK.lock().unwrap();
    let repo_root = unique_temp_dir("mem-init-gitignore");
    let xdg_root = unique_temp_dir("mem-init-gitignore-xdg");
    let old_config = std::env::var("XDG_CONFIG_HOME").ok();
    let old_state = std::env::var("XDG_STATE_HOME").ok();
    let old_cache = std::env::var("XDG_CACHE_HOME").ok();
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", xdg_root.join("config"));
        std::env::set_var("XDG_STATE_HOME", xdg_root.join("state"));
        std::env::set_var("XDG_CACHE_HOME", xdg_root.join("cache"));
    }
    fs::create_dir_all(repo_root.join(".mem")).unwrap();
    fs::write(repo_root.join(".mem/.gitignore"), "runtime/\n").unwrap();

    initialize_repo(&repo_root, "memory", false, false).unwrap();

    assert_eq!(
        fs::read_to_string(repo_root.join(".mem/.gitignore")).unwrap(),
        "*\n!.gitignore\n!project.toml\n"
    );

    restore_env_var("XDG_CONFIG_HOME", old_config);
    restore_env_var("XDG_STATE_HOME", old_state);
    restore_env_var("XDG_CACHE_HOME", old_cache);
    let _ = fs::remove_dir_all(repo_root);
    let _ = fs::remove_dir_all(xdg_root);
}

#[test]
fn dev_init_uses_short_capnp_unix_socket_path() {
    let _guard = ENV_LOCK.lock().unwrap();
    let repo_root = unique_temp_dir("mem-dev-init");
    let xdg_root = unique_temp_dir("mem-dev-init-xdg");
    let old_config = std::env::var("XDG_CONFIG_HOME").ok();
    let old_state = std::env::var("XDG_STATE_HOME").ok();
    let old_cache = std::env::var("XDG_CACHE_HOME").ok();
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", xdg_root.join("config"));
        std::env::set_var("XDG_STATE_HOME", xdg_root.join("state"));
        std::env::set_var("XDG_CACHE_HOME", xdg_root.join("cache"));
    }
    fs::create_dir_all(&repo_root).unwrap();
    initialize_repo(&repo_root, "memory", false, false).unwrap();

    initialize_dev_overlay(
        &repo_root,
        &super::DevInitArgs {
            force: false,
            dry_run: false,
            bind_addr: "127.0.0.1:4250".to_string(),
            capnp_tcp_addr: "127.0.0.1:4251".to_string(),
            copy_from_global: false,
            no_copy_from_global: true,
        },
    )
    .unwrap();

    let paths = mem_platform::project_paths(&repo_root, "memory").unwrap();
    let dev_config = fs::read_to_string(paths.dev_config_path()).unwrap();
    let socket_path = dev_config
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("capnp_unix_socket = ")
                .map(|value| value.trim_matches('"').to_string())
        })
        .unwrap();
    let socket_file_name = Path::new(&socket_path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap();
    assert!(socket_file_name.starts_with("memory-layer-dev-"));
    assert!(
        socket_path.len() < 100,
        "socket path should fit Unix SUN_LEN: {socket_path}"
    );

    restore_env_var("XDG_CONFIG_HOME", old_config);
    restore_env_var("XDG_STATE_HOME", old_state);
    restore_env_var("XDG_CACHE_HOME", old_cache);
    let _ = fs::remove_dir_all(repo_root);
    let _ = fs::remove_dir_all(xdg_root);
}

#[test]
fn resolve_repo_root_falls_back_to_cwd() {
    let cwd = PathBuf::from("/tmp/not-a-repo");
    assert_eq!(resolve_repo_root(&cwd).unwrap(), cwd);
}

#[test]
fn placeholder_database_url_is_detected() {
    assert!(is_placeholder_database_url(
        "postgresql://memory:<password>@localhost:5432/memory"
    ));
    assert!(!is_placeholder_database_url(
        "postgresql://memory:secret@localhost:5432/memory"
    ));
}

#[test]
fn database_url_is_masked_for_output() {
    assert_eq!(
        mask_database_url("postgresql://memory:secret@localhost:5432/memory"),
        "postgresql://<redacted>@localhost:5432/memory"
    );
}

#[test]
fn repair_repo_bootstrap_creates_missing_files() {
    let _guard = ENV_LOCK.lock().unwrap();
    let repo_root = unique_temp_dir("mem-doctor-fix");
    let xdg_root = unique_temp_dir("mem-doctor-fix-xdg");
    let old_config = std::env::var("XDG_CONFIG_HOME").ok();
    let old_state = std::env::var("XDG_STATE_HOME").ok();
    let old_cache = std::env::var("XDG_CACHE_HOME").ok();
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", xdg_root.join("config"));
        std::env::set_var("XDG_STATE_HOME", xdg_root.join("state"));
        std::env::set_var("XDG_CACHE_HOME", xdg_root.join("cache"));
    }
    fs::create_dir_all(&repo_root).unwrap();

    repair_repo_bootstrap(&repo_root, "memory").unwrap();
    let paths = mem_platform::project_paths(&repo_root, "memory").unwrap();

    assert!(paths.config_path().is_file());
    assert!(paths.project_path().is_file());
    assert!(repo_root.join(".mem/project.toml").is_file());
    assert!(repo_root.join(".agents/memory-layer.toml").is_file());
    assert!(paths.runtime_dir().is_dir());
    assert!(
        repo_root
            .join(".agents/skills/memory-layer/SKILL.md")
            .is_file()
    );
    assert!(
        repo_root
            .join(".agents/skills/memory-query-resume/SKILL.md")
            .is_file()
    );
    assert!(
        repo_root
            .join(".agents/skills/memory-project-init/SKILL.md")
            .is_file()
    );
    assert!(
        repo_root
            .join(".agents/skills/memory-github-init/SKILL.md")
            .is_file()
    );
    assert!(
        repo_root
            .join(".agents/skills/memory-plan-execution/SKILL.md")
            .is_file()
    );
    assert!(
        repo_root
            .join(".agents/skills/memory-direct-task-start/SKILL.md")
            .is_file()
    );
    assert!(
        repo_root
            .join(".agents/skills/memory-remember/SKILL.md")
            .is_file()
    );
    assert!(!root_gitignore_contains_mem(&repo_root).unwrap());

    restore_env_var("XDG_CONFIG_HOME", old_config);
    restore_env_var("XDG_STATE_HOME", old_state);
    restore_env_var("XDG_CACHE_HOME", old_cache);
    let _ = fs::remove_dir_all(repo_root);
    let _ = fs::remove_dir_all(xdg_root);
}

#[test]
fn init_preserves_existing_memory_skills_without_force() {
    let _guard = ENV_LOCK.lock().unwrap();
    let repo_root = unique_temp_dir("mem-init-skill-bundle");
    let xdg_root = unique_temp_dir("mem-init-skill-bundle-xdg");
    let old_config = std::env::var("XDG_CONFIG_HOME").ok();
    let old_state = std::env::var("XDG_STATE_HOME").ok();
    let old_cache = std::env::var("XDG_CACHE_HOME").ok();
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", xdg_root.join("config"));
        std::env::set_var("XDG_STATE_HOME", xdg_root.join("state"));
        std::env::set_var("XDG_CACHE_HOME", xdg_root.join("cache"));
    }
    fs::create_dir_all(repo_root.join(".agents/skills/memory-layer")).unwrap();
    fs::write(
        repo_root.join(".agents/skills/memory-layer/SKILL.md"),
        "custom umbrella skill\n",
    )
    .unwrap();

    initialize_repo(&repo_root, "memory", false, false).unwrap();

    assert_eq!(
        fs::read_to_string(repo_root.join(".agents/skills/memory-layer/SKILL.md")).unwrap(),
        "custom umbrella skill\n"
    );
    assert!(
        repo_root
            .join(".agents/skills/memory-query-resume/SKILL.md")
            .is_file()
    );
    assert!(
        repo_root
            .join(".agents/skills/memory-project-init/SKILL.md")
            .is_file()
    );
    assert!(
        repo_root
            .join(".agents/skills/memory-github-init/SKILL.md")
            .is_file()
    );
    assert!(
        repo_root
            .join(".agents/skills/memory-plan-execution/SKILL.md")
            .is_file()
    );
    assert!(
        repo_root
            .join(".agents/skills/memory-direct-task-start/SKILL.md")
            .is_file()
    );
    assert!(
        repo_root
            .join(".agents/skills/memory-remember/SKILL.md")
            .is_file()
    );

    restore_env_var("XDG_CONFIG_HOME", old_config);
    restore_env_var("XDG_STATE_HOME", old_state);
    restore_env_var("XDG_CACHE_HOME", old_cache);
    let _ = fs::remove_dir_all(repo_root);
    let _ = fs::remove_dir_all(xdg_root);
}

#[test]
fn agent_project_config_mentions_project_customization() {
    let repo_root = PathBuf::from("/tmp/memory");
    let content = render_agent_project_config("memory", &repo_root);

    assert!(content.contains("[capture]"));
    assert!(content.contains("include_paths"));
    assert!(content.contains("graph_enabled = false"));
}

#[test]
fn claude_memory_section_mentions_code_explanation_memory() {
    let content = render_claude_md_memory_section("memory");

    assert!(content.contains("Remember distilled code and codebase explanations"));
    assert!(content.contains("### Store code explanations"));
    assert!(content.contains("memory remember --project memory --type project"));
    assert!(content.contains("Do not store the full chat answer"));
    assert!(content.contains("Do not use `--file-changed` unless files actually changed"));
}

#[test]
fn init_copies_code_explanation_memory_rule_to_skills() {
    let repo_root = unique_temp_dir("mem-init-explanation-memory");
    fs::create_dir_all(&repo_root).unwrap();

    initialize_repo(&repo_root, "memory", false, false).unwrap();

    let umbrella =
        fs::read_to_string(repo_root.join(".agents/skills/memory-layer/SKILL.md")).unwrap();
    let github_init =
        fs::read_to_string(repo_root.join(".agents/skills/memory-github-init/SKILL.md")).unwrap();
    let query_resume =
        fs::read_to_string(repo_root.join(".agents/skills/memory-query-resume/SKILL.md")).unwrap();
    let remember =
        fs::read_to_string(repo_root.join(".agents/skills/memory-remember/SKILL.md")).unwrap();

    assert!(umbrella.contains("Code explanation memory rule"));
    assert!(umbrella.contains("memory-github-init"));
    assert!(github_init.contains("Memory GitHub Init Skill"));
    assert!(github_init.contains("dry-run"));
    assert!(umbrella.contains("Store a distilled memory, not the whole chat answer"));
    assert!(query_resume.contains("explain code, a file, a module"));
    assert!(query_resume.contains("store the distilled reusable explanation"));
    assert!(remember.contains("Remember a distilled code explanation"));
    assert!(remember.contains("--type project"));
    assert!(remember.contains("Do not use `--file-changed`"));

    let _ = fs::remove_dir_all(repo_root);
}

#[cfg(not(target_os = "macos"))]
#[test]
fn watch_unit_name_is_project_scoped() {
    assert_eq!(watch_unit_name("homelab"), "memory-watch-homelab.service");
    assert_eq!(
        watch_unit_name("customer portal"),
        "memory-watch-customer-portal.service"
    );
}

#[cfg(not(target_os = "macos"))]
#[test]
fn watch_unit_uses_repo_root_and_project() {
    let repo_root = unique_temp_dir("mem-watch-unit");
    fs::create_dir_all(&repo_root).unwrap();
    let unit = render_watch_unit(&repo_root, "homelab").unwrap();

    assert!(unit.contains("Description=Memory Layer Watcher (homelab)"));
    assert!(unit.contains(&format!("WorkingDirectory={}", repo_root.display())));
    assert!(unit.contains("EnvironmentFile=-"));
    assert!(unit.contains("run --project homelab"));

    let _ = fs::remove_dir_all(repo_root);
}

#[cfg(not(target_os = "macos"))]
#[test]
fn watch_manager_unit_uses_manager_subcommand() {
    let unit = render_watch_manager_unit(Path::new("/tmp/memory-layer.toml")).unwrap();
    assert!(unit.contains("watcher manager run"));
    assert!(unit.contains("Restart=always"));
}

#[cfg(not(target_os = "macos"))]
#[test]
fn agent_watcher_start_logic_reuses_loaded_active_units() {
    assert!(!should_start_agent_watcher(true, true, true));
    assert!(should_start_agent_watcher(true, true, false));
    assert!(should_start_agent_watcher(true, false, false));
    assert!(should_start_agent_watcher(false, true, true));
}

#[cfg(target_os = "macos")]
#[test]
fn launch_agent_labels_are_project_scoped() {
    assert_eq!(backend_launch_agent_label(), "com.memory-layer.mem-service");
    assert_eq!(
        watch_manager_launch_agent_label(),
        "com.memory-layer.memory-watch-manager"
    );
    assert_eq!(
        watch_launch_agent_label("customer portal"),
        "com.memory-layer.memory-watch.customer-portal"
    );
    assert_eq!(
        managed_watch_launch_agent_label("session 123"),
        "com.memory-layer.memory-watch.codex.session-123"
    );
    assert_eq!(
        sanitize_service_fragment("customer portal"),
        "customer-portal"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn backend_launch_agent_uses_global_config_path() {
    let plist = render_backend_launch_agent(&default_global_config_path()).unwrap();

    assert!(plist.contains("<string>com.memory-layer.mem-service</string>"));
    assert!(plist.contains("<string>/bin/zsh</string>"));
    assert!(plist.contains(&default_global_config_path().display().to_string()));
    assert!(plist.contains("mem-service.stdout.log"));
}

#[cfg(target_os = "macos")]
#[test]
fn watch_launch_agent_uses_repo_root_and_project() {
    let repo_root = unique_temp_dir("mem-watch-launch-agent");
    fs::create_dir_all(&repo_root).unwrap();
    let plist = render_watch_launch_agent(&repo_root, "homelab").unwrap();

    assert!(plist.contains("<string>com.memory-layer.memory-watch.homelab</string>"));
    assert!(plist.contains(&repo_root.display().to_string()));
    assert!(plist.contains("<string>/bin/zsh</string>"));
    assert!(plist.contains("memory-watch"));
    assert!(plist.contains("--project"));
    assert!(plist.contains("homelab"));

    let _ = fs::remove_dir_all(repo_root);
}

#[cfg(target_os = "macos")]
#[test]
fn watch_manager_launch_agent_uses_manager_subcommand() {
    let plist = render_watch_manager_launch_agent(&default_global_config_path()).unwrap();

    assert!(plist.contains("<string>com.memory-layer.memory-watch-manager</string>"));
    assert!(plist.contains("<string>/bin/zsh</string>"));
    assert!(plist.contains("watcher"));
    assert!(plist.contains("manager"));
    assert!(plist.contains("run"));
    assert!(plist.contains("memory-watch-manager.stdout.log"));
}

#[cfg(target_os = "macos")]
#[test]
fn managed_watch_launch_agent_uses_agent_metadata() {
    let repo_root = unique_temp_dir("mem-managed-watch-launch-agent");
    fs::create_dir_all(&repo_root).unwrap();
    let session = LightweightAgentSession {
        agent_cli: "codex",
        pid: 42,
        session_id: "session-123".to_string(),
        cwd: repo_root.display().to_string(),
        started_at: Utc::now().timestamp_millis() as u64,
    };
    let plist = render_managed_watch_launch_agent(
        &repo_root,
        "homelab",
        &session,
        "2026-04-10T00:00:00Z",
        None,
    )
    .unwrap();

    assert!(plist.contains("<string>com.memory-layer.memory-watch.codex.session-123</string>"));
    assert!(plist.contains("--agent-session-id"));
    assert!(plist.contains("session-123"));
    assert!(plist.contains("--agent-pid"));
    assert!(plist.contains("42"));
    assert!(plist.contains("--repo-root"));
    assert!(plist.contains(&repo_root.display().to_string()));

    let _ = fs::remove_dir_all(repo_root);
}

#[test]
fn shared_env_lookup_reads_key() {
    let dir = unique_temp_dir("mem-shared-env");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("memory-layer.env");
    fs::write(&path, "OPENAI_API_KEY=test-key\n").unwrap();

    assert_eq!(
        super::shared_env_lookup(&path, "OPENAI_API_KEY").as_deref(),
        Some("test-key")
    );

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn ensure_shared_service_api_token_creates_missing_token() {
    let dir = unique_temp_dir("mem-token-create");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("memory-layer.env");

    let result = ensure_shared_service_api_token(&path, None, true).unwrap();
    let token = shared_env_lookup(&path, SERVICE_API_TOKEN_KEY).unwrap();

    assert!(result.changed);
    assert!(matches!(result.action, ServiceApiTokenAction::Created));
    assert!(token.starts_with("ml_"));
    assert_ne!(token, DEV_API_TOKEN);

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn cluster_enabled_is_added_when_missing() {
    let dir = unique_temp_dir("mem-cluster-config");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("memory-layer.toml");
    fs::write(&path, "[service]\nbind_addr = \"127.0.0.1:4040\"\n").unwrap();

    set_cluster_enabled_in_shared_config(&path, true).unwrap();

    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("[cluster]"));
    assert!(content.contains("enabled = true"));

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn ensure_shared_service_api_token_rotates_placeholder() {
    let dir = unique_temp_dir("mem-token-rotate");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("memory-layer.env");
    fs::write(
        &path,
        format!("{SERVICE_API_TOKEN_KEY}={DEV_API_TOKEN}\nOPENAI_API_KEY=test-key\n"),
    )
    .unwrap();

    let result = ensure_shared_service_api_token(&path, None, true).unwrap();
    let token = shared_env_lookup(&path, SERVICE_API_TOKEN_KEY).unwrap();
    let content = fs::read_to_string(&path).unwrap();

    assert!(result.changed);
    assert!(matches!(result.action, ServiceApiTokenAction::Rotated));
    assert!(token.starts_with("ml_"));
    assert!(content.contains("OPENAI_API_KEY=test-key"));

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn cluster_enabled_is_updated_in_existing_section() {
    let dir = unique_temp_dir("mem-cluster-existing");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("memory-layer.toml");
    fs::write(
        &path,
        "[cluster]\nenabled = false\npriority = 50\n\n[service]\nbind_addr = \"127.0.0.1:4040\"\n",
    )
    .unwrap();

    set_cluster_enabled_in_shared_config(&path, true).unwrap();

    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("[cluster]\nenabled = true\npriority = 50"));

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn ensure_shared_service_api_token_preserves_existing_non_placeholder() {
    let dir = unique_temp_dir("mem-token-preserve");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("memory-layer.env");
    fs::write(&path, format!("{SERVICE_API_TOKEN_KEY}=ml_existingtoken\n")).unwrap();

    let result = ensure_shared_service_api_token(&path, None, true).unwrap();
    let token = shared_env_lookup(&path, SERVICE_API_TOKEN_KEY).unwrap();

    assert!(!result.changed);
    assert!(matches!(result.action, ServiceApiTokenAction::Preserved));
    assert_eq!(token, "ml_existingtoken");

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn write_headers_sends_token_for_loopback_service() {
    let mut config = test_app_config();
    config.service.bind_addr = "127.0.0.1:4040".to_string();
    config.service.api_token = "ml_testtoken".to_string();

    let headers = write_headers(&config).unwrap();

    assert_eq!(
        headers
            .get("x-api-token")
            .and_then(|value| value.to_str().ok()),
        Some("ml_testtoken")
    );
    assert!(headers.get("origin").is_none());
}

#[test]
fn write_headers_sends_token_for_non_loopback_service() {
    let mut config = test_app_config();
    config.service.bind_addr = "10.22.6.42:4140".to_string();
    config.service.api_token = "ml_testtoken".to_string();

    let headers = write_headers(&config).unwrap();

    assert_eq!(
        headers
            .get("x-api-token")
            .and_then(|value| value.to_str().ok()),
        Some("ml_testtoken")
    );
    assert!(headers.get("origin").is_none());
}

#[test]
fn direct_llm_eval_accepts_ollama_without_api_key() {
    let mut config = test_app_config();
    config.llm = mem_api::LlmConfig {
        provider: "ollama".to_string(),
        base_url: String::new(),
        api_key_env: "OPENAI_API_KEY".to_string(),
        model: "llama3.2".to_string(),
        ..mem_api::LlmConfig::default()
    };

    assert!(ensure_direct_llm_eval_config(&config).is_ok());
}

#[test]
fn watcher_manager_start_check_uses_cached_tracked_state() {
    assert!(!should_start_agent_watcher(true, true, true));
    assert!(should_start_agent_watcher(false, true, true));
    assert!(should_start_agent_watcher(true, false, true));
    assert!(should_start_agent_watcher(true, true, false));
}

#[test]
fn write_file_if_changed_skips_identical_content() {
    let dir = unique_temp_dir("mem-write-if-changed");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    write_file_if_changed(&path, b"same").unwrap();
    let first_modified = fs::metadata(&path).unwrap().modified().unwrap();
    std::thread::sleep(Duration::from_millis(5));
    write_file_if_changed(&path, b"same").unwrap();
    let second_modified = fs::metadata(&path).unwrap().modified().unwrap();
    assert_eq!(first_modified, second_modified);
    write_file_if_changed(&path, b"different").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "different");
    let _ = fs::remove_dir_all(&dir);
}

fn test_app_config() -> AppConfig {
    AppConfig {
        service: mem_api::ServiceConfig {
            bind_addr: "127.0.0.1:4040".to_string(),
            capnp_unix_socket: "/tmp/memory-layer.capnp.sock".to_string(),
            capnp_tcp_addr: "127.0.0.1:4041".to_string(),
            web_root: None,
            api_token: "ml_testtoken".to_string(),
            request_timeout: Duration::from_secs(30),
        },
        mcp: mem_api::McpConfig::default(),
        database: mem_api::DatabaseConfig {
            url: "postgresql://memory:test@localhost:5432/memory".to_string(),
        },
        features: mem_api::FeatureFlags::default(),
        llm: mem_api::LlmConfig::default(),
        llm_audit: mem_api::LlmAuditConfig::default(),
        embeddings: mem_api::EmbeddingsConfig::default(),
        cluster: mem_api::ClusterConfig::default(),
        writer: mem_api::WriterConfig::default(),
        automation: mem_api::AutomationConfig::default(),
        retention: mem_api::RetentionConfig::default(),
        provenance: mem_api::ProvenanceConfig::default(),
        profile: mem_api::Profile::Prod,
        resolved_config_path: None,
        resolved_dev_overlay_path: None,
    }
}

#[test]
fn skill_version_prefers_skill_frontmatter_over_agent_hint() {
    let repo = unique_temp_dir("mem-skill-version");
    let skill = repo.join("skill");
    fs::create_dir_all(skill.join("agents")).unwrap();
    fs::write(
        skill.join("SKILL.md"),
        "---\nname: example\nversion: 1.2.3\ndescription: test\n---\n",
    )
    .unwrap();
    fs::write(
        skill.join("agents/openai.yaml"),
        "name: example\nversion: 0.1.0\nentrypoint: SKILL.md\n",
    )
    .unwrap();

    assert_eq!(
        read_skill_version(&skill).unwrap(),
        Some("1.2.3".to_string())
    );

    let _ = fs::remove_dir_all(repo);
}

#[test]
fn skill_inventory_marks_outdated_and_missing_skills() {
    let repo = unique_temp_dir("mem-skill-inventory");
    let project_root = repo.join("project");
    let template_root = repo.join("memory-layer/skill-template");
    for name in MEMORY_SKILL_NAMES {
        write_test_skill(&template_root.join(name), "0.2.0");
    }
    write_test_skill(
        &project_root
            .join(".agents/skills")
            .join(MEMORY_SKILL_NAMES[0]),
        "0.1.0",
    );
    write_test_skill(
        &project_root
            .join(".agents/skills")
            .join(MEMORY_SKILL_NAMES[1]),
        "0.2.0",
    );

    let inventory =
        project_skill_inventory_with_template(&project_root, Some(template_root), false);

    assert_eq!(inventory.skills[0].status, SkillVersionStatus::Outdated);
    assert_eq!(inventory.skills[0].action, SkillUpgradeAction::Replace);
    assert_eq!(inventory.skills[1].status, SkillVersionStatus::UpToDate);
    assert_eq!(inventory.skills[1].action, SkillUpgradeAction::Skip);
    assert_eq!(inventory.skills[2].status, SkillVersionStatus::Missing);
    assert_eq!(inventory.skills[2].action, SkillUpgradeAction::Install);
    assert_eq!(inventory.bundle_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(inventory.status, SkillBundleStatus::Warn);
    assert!(inventory.summary.contains("project skill(s) need upgrade"));

    let _ = fs::remove_dir_all(repo);
}

#[test]
fn skill_upgrade_dry_run_does_not_replace_project_skill() {
    let repo = unique_temp_dir("mem-skill-upgrade-dry-run");
    let project_root = repo.join("project");
    let template_root = repo.join("memory-layer/skill-template");
    for name in MEMORY_SKILL_NAMES {
        write_test_skill(&template_root.join(name), "0.2.0");
        write_test_skill(&project_root.join(".agents/skills").join(name), "0.1.0");
    }
    let first_skill = project_root
        .join(".agents/skills")
        .join(MEMORY_SKILL_NAMES[0])
        .join("SKILL.md");
    let before = fs::read_to_string(&first_skill).unwrap();

    let report =
        upgrade_project_skills_with_template(&project_root, Some(template_root), false, true)
            .unwrap();

    assert!(report.dry_run);
    assert_eq!(report.backup_root, None);
    assert_eq!(report.inventory.bundle_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(report.inventory.status, SkillBundleStatus::Warn);
    assert_eq!(fs::read_to_string(&first_skill).unwrap(), before);

    let _ = fs::remove_dir_all(repo);
}

#[test]
fn skill_upgrade_replaces_outdated_skill_and_creates_backup() {
    let _guard = ENV_LOCK.lock().unwrap();
    let repo = unique_temp_dir("mem-skill-upgrade");
    let xdg_root = unique_temp_dir("mem-skill-upgrade-xdg");
    let old_config = std::env::var("XDG_CONFIG_HOME").ok();
    let old_state = std::env::var("XDG_STATE_HOME").ok();
    let old_cache = std::env::var("XDG_CACHE_HOME").ok();
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", xdg_root.join("config"));
        std::env::set_var("XDG_STATE_HOME", xdg_root.join("state"));
        std::env::set_var("XDG_CACHE_HOME", xdg_root.join("cache"));
    }
    let project_root = repo.join("project");
    let template_root = repo.join("memory-layer/skill-template");
    for name in MEMORY_SKILL_NAMES {
        write_test_skill(&template_root.join(name), "0.2.0");
        write_test_skill(&project_root.join(".agents/skills").join(name), "0.1.0");
    }

    let report =
        upgrade_project_skills_with_template(&project_root, Some(template_root), false, false)
            .unwrap();
    let backup_root = report.backup_root.clone().expect("backup root");

    assert!(!report.dry_run);
    assert_eq!(
        read_skill_version(
            &project_root
                .join(".agents/skills")
                .join(MEMORY_SKILL_NAMES[0])
        )
        .unwrap(),
        Some("0.2.0".to_string())
    );
    assert!(
        PathBuf::from(backup_root)
            .join(MEMORY_SKILL_NAMES[0])
            .join("SKILL.md")
            .is_file()
    );
    assert_eq!(report.inventory.status, SkillBundleStatus::Warn);

    restore_env_var("XDG_CONFIG_HOME", old_config);
    restore_env_var("XDG_STATE_HOME", old_state);
    restore_env_var("XDG_CACHE_HOME", old_cache);
    let _ = fs::remove_dir_all(repo);
    let _ = fs::remove_dir_all(xdg_root);
}

#[test]
fn skill_inventory_reports_ok_for_matching_bundle_version() {
    let repo = unique_temp_dir("mem-skill-bundle-ok");
    let project_root = repo.join("project");
    let template_root = repo.join("memory-layer/skill-template");
    for name in MEMORY_SKILL_NAMES {
        write_test_skill(&template_root.join(name), env!("CARGO_PKG_VERSION"));
        write_test_skill(
            &project_root.join(".agents/skills").join(name),
            env!("CARGO_PKG_VERSION"),
        );
    }

    let inventory =
        project_skill_inventory_with_template(&project_root, Some(template_root), false);

    assert_eq!(inventory.bundle_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(inventory.status, SkillBundleStatus::Ok);
    assert!(inventory.summary.contains("all project skills match"));

    let _ = fs::remove_dir_all(repo);
}

fn write_test_skill(path: &Path, version: &str) {
    fs::create_dir_all(path.join("agents")).unwrap();
    let name = path.file_name().and_then(|value| value.to_str()).unwrap();
    fs::write(
        path.join("SKILL.md"),
        format!("---\nname: {name}\nversion: {version}\ndescription: test\n---\n"),
    )
    .unwrap();
    fs::write(
        path.join("agents/openai.yaml"),
        format!("name: {name}\nversion: {version}\nentrypoint: SKILL.md\n"),
    )
    .unwrap();
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
