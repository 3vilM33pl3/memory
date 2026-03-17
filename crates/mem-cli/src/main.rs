mod tui;

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
};

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use mem_api::{
    AppConfig, ArchiveRequest, ArchiveResponse, CaptureTaskRequest, CurateRequest, CurateResponse,
    MemoryEntryResponse, ProjectMemoriesResponse, ProjectOverviewResponse, QueryFilters,
    QueryRequest, QueryResponse, ReindexRequest, ReindexResponse, TestResult,
    discover_global_config_path,
};
use mem_watch::{flush_path, load_state, run_once, to_status};
use reqwest::{Client, header::HeaderMap};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(name = "memctl")]
struct Cli {
    #[arg(long, env = "MEMORY_LAYER_CONFIG")]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Init(InitArgs),
    Doctor(DoctorArgs),
    Query(QueryArgs),
    CaptureTask(CaptureTaskArgs),
    Remember(RememberArgs),
    Curate(CurateArgs),
    Reindex(ProjectArgs),
    Health,
    Stats,
    Archive(ArchiveArgs),
    Automation(AutomationArgs),
    Tui(TuiArgs),
}

#[derive(Debug, Args)]
struct InitArgs {
    #[arg(long)]
    project: Option<String>,
    #[arg(long)]
    force: bool,
    #[arg(long)]
    print: bool,
}

#[derive(Debug, Args)]
struct DoctorArgs {
    #[arg(long)]
    project: Option<String>,
    #[arg(long)]
    fix: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct QueryArgs {
    #[arg(long)]
    project: String,
    #[arg(long)]
    question: String,
    #[arg(long = "type")]
    types: Vec<String>,
    #[arg(long = "tag")]
    tags: Vec<String>,
    #[arg(long, default_value_t = 8)]
    limit: i64,
    #[arg(long)]
    min_confidence: Option<f32>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct CaptureTaskArgs {
    #[arg(long)]
    file: PathBuf,
}

#[derive(Debug, Args)]
struct RememberArgs {
    #[arg(long)]
    project: Option<String>,
    #[arg(long)]
    title: Option<String>,
    #[arg(long)]
    prompt: Option<String>,
    #[arg(long)]
    summary: Option<String>,
    #[arg(long = "note")]
    notes: Vec<String>,
    #[arg(long = "file-changed")]
    files_changed: Vec<String>,
    #[arg(long = "test-passed")]
    tests_passed: Vec<String>,
    #[arg(long = "test-failed")]
    tests_failed: Vec<String>,
    #[arg(long)]
    command_output_file: Option<PathBuf>,
    #[arg(long, default_value_t = true)]
    auto_files: bool,
}

#[derive(Debug, Args)]
struct CurateArgs {
    #[arg(long)]
    project: String,
    #[arg(long)]
    batch_size: Option<i64>,
}

#[derive(Debug, Args)]
struct ProjectArgs {
    #[arg(long)]
    project: String,
}

#[derive(Debug, Args)]
struct ArchiveArgs {
    #[arg(long)]
    project: String,
    #[arg(long, default_value_t = 0.3)]
    max_confidence: f32,
    #[arg(long, default_value_t = 1)]
    max_importance: i32,
}

#[derive(Debug, Args)]
struct TuiArgs {
    #[arg(long)]
    project: Option<String>,
}

#[derive(Debug, Args)]
struct AutomationArgs {
    #[command(subcommand)]
    command: AutomationCommand,
}

#[derive(Debug, Subcommand)]
enum AutomationCommand {
    Status(ProjectArgs),
    Flush(ProjectArgs),
}

#[tokio::main]
async fn main() -> Result<()> {
    let Cli {
        config: cli_config,
        command,
    } = Cli::parse();

    match &command {
        Command::Init(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            let project = resolve_project_slug(args.project.clone(), &cwd)?;
            let repo_root = resolve_repo_root(&cwd)?;
            let output = initialize_repo(&repo_root, &project, args.force, args.print)?;
            println!("{output}");
            return Ok(());
        }
        Command::Doctor(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            let repo_root = resolve_repo_root(&cwd)?;
            let project = resolve_project_slug(args.project.clone(), &cwd)
                .unwrap_or_else(|_| repo_root.file_name().and_then(|v| v.to_str()).unwrap_or("memory").to_string());
            let report = run_doctor(cli_config.clone(), &repo_root, &project, args.fix).await?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_doctor_report(&report);
            }
            return Ok(());
        }
        _ => {}
    }

    let config = AppConfig::load_from_path(cli_config).context("load config")?;
    let client = Client::builder()
        .timeout(config.service.request_timeout)
        .build()
        .context("build http client")?;

    match command {
        Command::Init(_) => unreachable!("init is handled before config loading"),
        Command::Doctor(_) => unreachable!("doctor is handled before config loading"),
        Command::Query(args) => {
            let request = QueryRequest {
                project: args.project,
                query: args.question,
                filters: QueryFilters {
                    types: args
                        .types
                        .into_iter()
                        .map(parse_memory_type)
                        .collect::<Result<Vec<_>>>()?,
                    tags: args.tags,
                },
                top_k: args.limit,
                min_confidence: args.min_confidence,
            };
            let payload: QueryResponse = get_json(
                client
                    .post(service_url(&config, "/v1/query"))
                    .json(&request)
                    .send()
                    .await
                    .context("query request failed")?,
            )
            .await?;
            if args.json {
                println!("{}", serde_json::to_string(&payload)?);
            } else {
                print_query_response(payload);
            }
        }
        Command::CaptureTask(args) => {
            let request: CaptureTaskRequest =
                serde_json::from_str(&fs::read_to_string(args.file).context("read payload file")?)?;
            let response = client
                .post(service_url(&config, "/v1/capture/task"))
                .headers(write_headers(&config.service.api_token)?)
                .json(&request)
                .send()
                .await?;
            print_json_response(response).await?;
        }
        Command::Remember(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            let project = resolve_project_slug(args.project.clone(), &cwd)?;
            let request = build_remember_request(args, &project)?;
            let api = ApiClient::new(client, config);
            let capture = api.capture_task(&request).await?;
            let curate = api.curate(&project).await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "capture": capture,
                    "curate": curate
                }))?
            );
        }
        Command::Curate(args) => {
            let response = client
                .post(service_url(&config, "/v1/curate"))
                .headers(write_headers(&config.service.api_token)?)
                .json(&CurateRequest {
                    project: args.project,
                    batch_size: args.batch_size,
                })
                .send()
                .await?;
            print_json_response(response).await?;
        }
        Command::Reindex(args) => {
            let response = client
                .post(service_url(&config, "/v1/reindex"))
                .headers(write_headers(&config.service.api_token)?)
                .json(&ReindexRequest {
                    project: args.project,
                })
                .send()
                .await?;
            print_json_response(response).await?;
        }
        Command::Health => {
            let response = client.get(service_url(&config, "/healthz")).send().await?;
            print_json_response(response).await?;
        }
        Command::Stats => {
            let response = client.get(service_url(&config, "/v1/stats")).send().await?;
            print_json_response(response).await?;
        }
        Command::Archive(args) => {
            let response = client
                .post(service_url(&config, "/v1/archive"))
                .headers(write_headers(&config.service.api_token)?)
                .json(&ArchiveRequest {
                    project: args.project,
                    max_confidence: args.max_confidence,
                    max_importance: args.max_importance,
                })
                .send()
                .await?;
            print_json_response(response).await?;
        }
        Command::Automation(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            match args.command {
                AutomationCommand::Status(args) => {
                    let project = resolve_project_slug(Some(args.project), &cwd)?;
                    let repo_root = config
                        .automation
                        .repo_root
                        .as_ref()
                        .map(PathBuf::from)
                        .unwrap_or(cwd);
                    let state = load_state(&project, &repo_root, &config.automation).await?;
                    println!("{}", serde_json::to_string_pretty(&to_status(&state))?);
                }
                AutomationCommand::Flush(args) => {
                    let project = resolve_project_slug(Some(args.project), &cwd)?;
                    let repo_root = config
                        .automation
                        .repo_root
                        .as_ref()
                        .map(PathBuf::from)
                        .unwrap_or(cwd);
                    let api = ApiClient::new(client.clone(), config.clone());
                    tokio::fs::write(flush_path(&repo_root), b"flush\n")
                        .await
                        .ok();
                    run_once(&api.config, &api.client, &project, &repo_root, true).await?;
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "project": project,
                            "status": "flush_requested"
                        }))?
                    );
                }
            }
        }
        Command::Tui(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            let project = resolve_project_slug(args.project, &cwd)?;
            let api = ApiClient::new(client, config);
            tui::run(api, project).await?;
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize)]
struct DoctorReport {
    project: String,
    repo_root: String,
    config_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    global_config_path: Option<String>,
    fix_mode: bool,
    checks: Vec<DoctorCheckResult>,
}

#[derive(Debug, Clone, Serialize)]
struct DoctorCheckResult {
    id: String,
    status: DoctorStatus,
    summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    suggested_fix: Option<String>,
    #[serde(default)]
    fix_applied: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum DoctorStatus {
    Ok,
    Warn,
    Fail,
    Skipped,
}

impl DoctorReport {
    fn push(&mut self, result: DoctorCheckResult) {
        self.checks.push(result);
    }
}

fn doctor_check(
    id: &str,
    status: DoctorStatus,
    summary: impl Into<String>,
    details: Option<String>,
    suggested_fix: Option<String>,
    fix_applied: bool,
) -> DoctorCheckResult {
    DoctorCheckResult {
        id: id.to_string(),
        status,
        summary: summary.into(),
        details,
        suggested_fix,
        fix_applied,
    }
}

async fn run_doctor(
    cli_config: Option<PathBuf>,
    repo_root: &Path,
    project: &str,
    fix: bool,
) -> Result<DoctorReport> {
    let config_path = cli_config
        .clone()
        .unwrap_or_else(|| repo_root.join(".mem").join("config.toml"));
    let global_config_path = discover_global_config_path();
    let mut report = DoctorReport {
        project: project.to_string(),
        repo_root: repo_root.display().to_string(),
        config_path: config_path.display().to_string(),
        global_config_path: global_config_path
            .as_ref()
            .map(|path| path.display().to_string()),
        fix_mode: fix,
        checks: Vec::new(),
    };

    let mem_dir = repo_root.join(".mem");
    let project_path = mem_dir.join("project.toml");
    let root_gitignore_path = repo_root.join(".gitignore");

    let mut init_fix_applied = false;
    if !mem_dir.exists() && fix {
        initialize_repo(repo_root, project, false, false)?;
        init_fix_applied = true;
    }

    report.push(doctor_check(
        "repo.bootstrap_dir",
        if mem_dir.exists() || init_fix_applied {
            DoctorStatus::Ok
        } else {
            DoctorStatus::Fail
        },
        if mem_dir.exists() || init_fix_applied {
            "Repo-local .mem directory is present."
        } else {
            "Repo-local .mem directory is missing."
        },
        Some(mem_dir.display().to_string()),
        if mem_dir.exists() || init_fix_applied {
            None
        } else {
            Some("memctl init".to_string())
        },
        init_fix_applied,
    ));

    let config_fix_applied = if !config_path.exists() && fix {
        repair_repo_bootstrap(repo_root, project)?;
        true
    } else {
        false
    };
    report.push(doctor_check(
        "repo.config_file",
        if config_path.exists() || config_fix_applied {
            DoctorStatus::Ok
        } else {
            DoctorStatus::Fail
        },
        if config_path.exists() || config_fix_applied {
            "Config file is present."
        } else {
            "Config file is missing."
        },
        Some(config_path.display().to_string()),
        if config_path.exists() || config_fix_applied {
            None
        } else {
            Some("memctl init".to_string())
        },
        config_fix_applied,
    ));

    let project_fix_applied = if !project_path.exists() && fix {
        repair_repo_bootstrap(repo_root, project)?;
        true
    } else {
        false
    };
    report.push(doctor_check(
        "repo.project_file",
        if project_path.exists() || project_fix_applied {
            DoctorStatus::Ok
        } else {
            DoctorStatus::Fail
        },
        if project_path.exists() || project_fix_applied {
            "Project metadata file is present."
        } else {
            "Project metadata file is missing."
        },
        Some(project_path.display().to_string()),
        if project_path.exists() || project_fix_applied {
            None
        } else {
            Some("memctl init".to_string())
        },
        project_fix_applied,
    ));

    report.push(doctor_check(
        "global.config_file",
        if global_config_path.is_some() {
            DoctorStatus::Ok
        } else {
            DoctorStatus::Warn
        },
        if global_config_path.is_some() {
            "Global shared config is present."
        } else {
            "Global shared config is missing."
        },
        Some(
            global_config_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(default_global_config_path_label),
        ),
        if global_config_path.is_some() {
            None
        } else {
            Some("Create the global config and set shared defaults there.".to_string())
        },
        false,
    ));

    let gitignore_fix_applied = if !root_gitignore_contains_mem(repo_root)? && fix {
        ensure_root_gitignore_entry(&root_gitignore_path, "/.mem\n")?;
        true
    } else {
        false
    };
    report.push(doctor_check(
        "repo.gitignore",
        if root_gitignore_contains_mem(repo_root)? {
            DoctorStatus::Ok
        } else {
            DoctorStatus::Warn
        },
        if root_gitignore_contains_mem(repo_root)? {
            "Root .gitignore ignores .mem."
        } else {
            "Root .gitignore does not ignore .mem."
        },
        Some(root_gitignore_path.display().to_string()),
        if root_gitignore_contains_mem(repo_root)? {
            None
        } else {
            Some("memctl doctor --fix".to_string())
        },
        gitignore_fix_applied,
    ));

    let config = match AppConfig::load_from_path(cli_config.clone()) {
        Ok(config) => {
            report.push(doctor_check(
                "config.load",
                DoctorStatus::Ok,
                "Merged config loads successfully.",
                None,
                None,
                false,
            ));
            Some(config)
        }
        Err(error) => {
            report.push(doctor_check(
                "config.load",
                DoctorStatus::Fail,
                "Merged config failed to load.",
                Some(error.to_string()),
                Some(format!(
                    "Check {} and {}",
                    global_config_path
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(default_global_config_path_label),
                    config_path.display()
                )),
                false,
            ));
            None
        }
    };

    if let Some(config) = config {
        report.push(doctor_check(
            "config.database_url",
            if is_placeholder_database_url(&config.database.url) {
                DoctorStatus::Warn
            } else {
                DoctorStatus::Ok
            },
            if is_placeholder_database_url(&config.database.url) {
                "Database URL still uses the placeholder value."
            } else {
                "Database URL is configured."
            },
            Some(mask_database_url(&config.database.url)),
            if is_placeholder_database_url(&config.database.url) {
                Some(format!(
                    "Set [database].url in {}",
                    global_config_path
                        .as_ref()
                        .unwrap_or(&config_path)
                        .display()
                ))
            } else {
                None
            },
            false,
        ));

        report.push(doctor_check(
            "config.api_token",
            if config.service.api_token.trim().is_empty() {
                DoctorStatus::Fail
            } else if config.service.api_token == "dev-memory-token" {
                DoctorStatus::Warn
            } else {
                DoctorStatus::Ok
            },
            if config.service.api_token.trim().is_empty() {
                "API token is empty."
            } else if config.service.api_token == "dev-memory-token" {
                "API token is set to the development default."
            } else {
                "API token is configured."
            },
            None,
            if config.service.api_token.trim().is_empty() {
                Some(format!(
                    "Set [service].api_token in {}",
                    global_config_path
                        .as_ref()
                        .unwrap_or(&config_path)
                        .display()
                ))
            } else {
                None
            },
            false,
        ));

        let runtime_dir = automation_runtime_dir(&config, repo_root);
        let runtime_fix_applied = if !runtime_dir.exists() && fix {
            fs::create_dir_all(&runtime_dir)
                .with_context(|| format!("create {}", runtime_dir.display()))?;
            true
        } else {
            false
        };
        report.push(doctor_check(
            "automation.runtime_dir",
            if runtime_dir.exists() {
                DoctorStatus::Ok
            } else if config.automation.enabled {
                DoctorStatus::Warn
            } else {
                DoctorStatus::Warn
            },
            if runtime_dir.exists() {
                "Automation runtime directory is present."
            } else {
                "Automation runtime directory is missing."
            },
            Some(runtime_dir.display().to_string()),
            if runtime_dir.exists() {
                None
            } else {
                Some("memctl doctor --fix".to_string())
            },
            runtime_fix_applied,
        ));

        let resolved_repo_root = config
            .automation
            .repo_root
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| repo_root.to_path_buf());
        report.push(doctor_check(
            "automation.repo_root",
            if resolved_repo_root == repo_root {
                DoctorStatus::Ok
            } else {
                DoctorStatus::Warn
            },
            if resolved_repo_root == repo_root {
                "Automation repo_root matches the current repository."
            } else {
                "Automation repo_root differs from the current repository."
            },
            Some(resolved_repo_root.display().to_string()),
            if resolved_repo_root == repo_root {
                None
            } else {
                Some(format!("Edit {} and set [automation].repo_root", config_path.display()))
            },
            false,
        ));

        let client = Client::builder()
            .timeout(config.service.request_timeout)
            .build()
            .context("build doctor http client")?;
        let api = ApiClient::new(client, config.clone());

        match api.health().await {
            Ok(value) => {
                report.push(doctor_check(
                    "backend.health",
                    DoctorStatus::Ok,
                    "Backend health endpoint is reachable.",
                    Some(value.to_string()),
                    None,
                    false,
                ));
                match api.project_overview(project).await {
                    Ok(overview) => report.push(doctor_check(
                        "backend.project_overview",
                        DoctorStatus::Ok,
                        "Project overview endpoint is reachable.",
                        Some(format!(
                            "{} memories / {} raw captures",
                            overview.memory_entries_total, overview.raw_captures_total
                        )),
                        None,
                        false,
                    )),
                    Err(error) => report.push(doctor_check(
                        "backend.project_overview",
                        DoctorStatus::Warn,
                        "Project overview endpoint did not return data.",
                        Some(error.to_string()),
                        Some(format!("memctl init --project {}", project)),
                        false,
                    )),
                }
            }
            Err(error) => {
                report.push(doctor_check(
                    "backend.health",
                    DoctorStatus::Fail,
                    "Backend health endpoint is not reachable.",
                    Some(error.to_string()),
                    Some(format!("mem-service {}", config_path.display())),
                    false,
                ));
                report.push(doctor_check(
                    "backend.project_overview",
                    DoctorStatus::Skipped,
                    "Skipped project overview because the backend is unavailable.",
                    None,
                    None,
                    false,
                ));
            }
        }

        match load_state(project, &resolved_repo_root, &config.automation).await {
            Ok(state) => report.push(doctor_check(
                "automation.state",
                if config.automation.enabled {
                    DoctorStatus::Ok
                } else {
                    DoctorStatus::Skipped
                },
                if config.automation.enabled {
                    "Automation state can be loaded."
                } else {
                    "Skipped automation state because automation is disabled."
                },
                Some(format!(
                    "enabled={} dirty_files={}",
                    state.enabled,
                    state.current_session.changed_files.len()
                )),
                None,
                false,
            )),
            Err(error) => report.push(doctor_check(
                "automation.state",
                if config.automation.enabled {
                    DoctorStatus::Warn
                } else {
                    DoctorStatus::Skipped
                },
                if config.automation.enabled {
                    "Automation state could not be loaded."
                } else {
                    "Skipped automation state because automation is disabled."
                },
                Some(error.to_string()),
                Some("memctl doctor --fix".to_string()),
                false,
            )),
        }

        let remember_prereqs = detect_changed_files().is_ok();
        report.push(doctor_check(
            "workflow.remember_ready",
            if remember_prereqs {
                DoctorStatus::Ok
            } else {
                DoctorStatus::Warn
            },
            if remember_prereqs {
                "Remember workflow prerequisites look usable."
            } else {
                "Remember workflow could not inspect repo state."
            },
            None,
            if remember_prereqs {
                None
            } else {
                Some("Ensure git is available and run inside the repo".to_string())
            },
            false,
        ));
    } else {
        for (id, summary) in [
            ("config.database_url", "Skipped database URL validation because config could not load."),
            ("config.api_token", "Skipped API token validation because config could not load."),
            ("automation.runtime_dir", "Skipped automation runtime checks because config could not load."),
            ("automation.repo_root", "Skipped automation repo_root check because config could not load."),
            ("backend.health", "Skipped backend health check because config could not load."),
            ("backend.project_overview", "Skipped project overview check because config could not load."),
            ("automation.state", "Skipped automation state check because config could not load."),
            ("workflow.remember_ready", "Skipped remember readiness check because config could not load."),
        ] {
            report.push(doctor_check(
                id,
                DoctorStatus::Skipped,
                summary,
                None,
                None,
                false,
            ));
        }
    }

    Ok(report)
}

fn repair_repo_bootstrap(repo_root: &Path, project: &str) -> Result<()> {
    let mem_dir = repo_root.join(".mem");
    let runtime_dir = mem_dir.join("runtime");
    let config_path = mem_dir.join("config.toml");
    let project_path = mem_dir.join("project.toml");
    let local_gitignore_path = mem_dir.join(".gitignore");

    fs::create_dir_all(&runtime_dir).context("create .mem/runtime")?;
    if !config_path.exists() {
        fs::write(&config_path, render_repo_config(repo_root)).context("write .mem/config.toml")?;
    }
    if !project_path.exists() {
        fs::write(&project_path, render_project_metadata(project, repo_root))
            .context("write .mem/project.toml")?;
    }
    if !local_gitignore_path.exists() {
        fs::write(&local_gitignore_path, "runtime/\n").context("write .mem/.gitignore")?;
    }
    ensure_root_gitignore_entry(&repo_root.join(".gitignore"), "/.mem\n")?;
    Ok(())
}

fn root_gitignore_contains_mem(repo_root: &Path) -> Result<bool> {
    let path = repo_root.join(".gitignore");
    if !path.exists() {
        return Ok(false);
    }
    let content = fs::read_to_string(path)?;
    Ok(content.lines().any(|line| line.trim() == "/.mem"))
}

fn is_placeholder_database_url(value: &str) -> bool {
    value.contains("<password>") || value.trim().is_empty()
}

fn mask_database_url(value: &str) -> String {
    if let Some((prefix, rest)) = value.split_once("://") {
        if let Some((creds, suffix)) = rest.split_once('@') {
            if creds.contains(':') {
                return format!("{prefix}://<redacted>@{suffix}");
            }
        }
    }
    value.to_string()
}

fn automation_runtime_dir(config: &AppConfig, repo_root: &Path) -> PathBuf {
    if let Some(path) = &config.automation.state_file_path {
        PathBuf::from(path)
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| repo_root.join(".mem").join("runtime"))
    } else if let Some(path) = &config.automation.audit_log_path {
        PathBuf::from(path)
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| repo_root.join(".mem").join("runtime"))
    } else {
        repo_root.join(".mem").join("runtime")
    }
}

fn print_doctor_report(report: &DoctorReport) {
    println!(
        "Doctor report for project {} at {}\n",
        report.project, report.repo_root
    );
    if let Some(global_config_path) = &report.global_config_path {
        println!("Merged global config: {global_config_path}");
    } else {
        println!(
            "Merged global config: <not found> (expected at {})",
            default_global_config_path_label()
        );
    }
    println!("Repo-local config: {}\n", report.config_path);
    for check in &report.checks {
        let icon = match check.status {
            DoctorStatus::Ok => "OK",
            DoctorStatus::Warn => "WARN",
            DoctorStatus::Fail => "FAIL",
            DoctorStatus::Skipped => "SKIP",
        };
        println!("[{icon}] {} - {}", check.id, check.summary);
        if let Some(details) = &check.details {
            println!("  details: {details}");
        }
        if let Some(fix) = &check.suggested_fix {
            println!("  fix: {fix}");
        }
        if check.fix_applied {
            println!("  applied: true");
        }
    }

    let ok = report
        .checks
        .iter()
        .filter(|check| check.status == DoctorStatus::Ok)
        .count();
    let warn = report
        .checks
        .iter()
        .filter(|check| check.status == DoctorStatus::Warn)
        .count();
    let fail = report
        .checks
        .iter()
        .filter(|check| check.status == DoctorStatus::Fail)
        .count();
    let skipped = report
        .checks
        .iter()
        .filter(|check| check.status == DoctorStatus::Skipped)
        .count();
    println!("\nSummary: {ok} ok, {warn} warn, {fail} fail, {skipped} skipped");
}

fn default_global_config_path_label() -> String {
    if let Ok(config_home) = env::var("XDG_CONFIG_HOME") {
        format!("{config_home}/memory-layer/memory-layer.toml")
    } else if let Ok(home) = env::var("HOME") {
        format!("{home}/.config/memory-layer/memory-layer.toml")
    } else {
        "/etc/memory-layer/memory-layer.toml".to_string()
    }
}

fn initialize_repo(repo_root: &Path, project: &str, force: bool, print_only: bool) -> Result<String> {
    let mem_dir = repo_root.join(".mem");
    let runtime_dir = mem_dir.join("runtime");
    let config_path = mem_dir.join("config.toml");
    let project_path = mem_dir.join("project.toml");
    let local_gitignore_path = mem_dir.join(".gitignore");
    let root_gitignore_path = repo_root.join(".gitignore");

    if !force {
        for path in [&config_path, &project_path] {
            if path.exists() {
                anyhow::bail!(
                    "{} already exists; rerun with --force to overwrite generated files",
                    path.display()
                );
            }
        }
    }

    let config_contents = render_repo_config(repo_root);
    let project_contents = render_project_metadata(project, repo_root);
    let mem_gitignore_contents = "runtime/\n";
    let root_gitignore_line = "/.mem\n";

    if !print_only {
        fs::create_dir_all(&runtime_dir).context("create .mem/runtime")?;
        fs::write(&config_path, config_contents).context("write .mem/config.toml")?;
        fs::write(&project_path, project_contents).context("write .mem/project.toml")?;
        fs::write(&local_gitignore_path, mem_gitignore_contents).context("write .mem/.gitignore")?;
        ensure_root_gitignore_entry(&root_gitignore_path, root_gitignore_line)?;
    }

    Ok(render_init_summary(
        repo_root,
        project,
        &config_path,
        &project_path,
        print_only,
    ))
}

fn render_repo_config(repo_root: &Path) -> String {
    let repo_root = repo_root.display();
    format!(
        r#"# Repo-local overrides for this project.
# Put shared defaults and secrets in the global config:
#   {}

[automation]
enabled = false
mode = "suggest"
repo_root = "{repo_root}"
poll_interval = "10s"
idle_threshold = "5m"
min_changed_files = 2
require_passing_test = false
ignored_paths = [".git/", "target/", ".mem/"]
audit_log_path = "{repo_root}/.mem/runtime/automation.log"
state_file_path = "{repo_root}/.mem/runtime/automation-state.json"
"#,
        default_global_config_path_label()
    )
}

fn render_project_metadata(project: &str, repo_root: &Path) -> String {
    format!(
        r#"slug = "{project}"
repo_root = "{}"
"#,
        repo_root.display()
    )
}

fn ensure_root_gitignore_entry(path: &Path, line: &str) -> Result<()> {
    let mut content = if path.exists() {
        fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?
    } else {
        String::new()
    };

    if !content.lines().any(|existing| existing.trim() == line.trim()) {
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(line);
        fs::write(path, content).with_context(|| format!("write {}", path.display()))?;
    }

    Ok(())
}

fn render_init_summary(
    repo_root: &Path,
    project: &str,
    config_path: &Path,
    project_path: &Path,
    print_only: bool,
) -> String {
    let action = if print_only { "Would create" } else { "Created" };
    format!(
        "{action} repo-local memory bootstrap for project `{project}` at {}.\n\nFiles:\n- {}\n- {}\n- {}/runtime/\n\nNext steps:\n1. Set shared values like `database.url` and `service.api_token` in {}\n2. Use {} only for repo-specific overrides\n3. Start the backend from this repo:\n   mem-service\n4. Optional: start the watcher:\n   memory-watch run --project {}\n5. Open the TUI:\n   mem-cli tui --project {}",
        repo_root.display(),
        config_path.display(),
        project_path.display(),
        config_path.parent().unwrap_or(repo_root).display(),
        default_global_config_path_label(),
        config_path.display(),
        project,
        project
    )
}

fn resolve_repo_root(cwd: &Path) -> Result<PathBuf> {
    let output = ProcessCommand::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output();

    if let Ok(output) = output {
        if output.status.success() {
            let stdout = String::from_utf8(output.stdout).context("decode git rev-parse output")?;
            let root = stdout.trim();
            if !root.is_empty() {
                return Ok(PathBuf::from(root));
            }
        }
    }

    Ok(cwd.to_path_buf())
}

#[derive(Clone)]
pub(crate) struct ApiClient {
    client: Client,
    config: AppConfig,
}

impl ApiClient {
    pub(crate) fn new(client: Client, config: AppConfig) -> Self {
        Self { client, config }
    }

    pub(crate) async fn health(&self) -> Result<serde_json::Value> {
        get_json(
            self.client
                .get(service_url(&self.config, "/healthz"))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn project_memories(&self, project: &str) -> Result<ProjectMemoriesResponse> {
        get_json(
            self.client
                .get(service_url(
                    &self.config,
                    &format!("/v1/projects/{project}/memories"),
                ))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn project_overview(&self, project: &str) -> Result<ProjectOverviewResponse> {
        get_json(
            self.client
                .get(service_url(
                    &self.config,
                    &format!("/v1/projects/{project}/overview"),
                ))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn memory_detail(&self, memory_id: &str) -> Result<MemoryEntryResponse> {
        get_json(
            self.client
                .get(service_url(
                    &self.config,
                    &format!("/v1/memory/{memory_id}"),
                ))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn capture_task(
        &self,
        request: &CaptureTaskRequest,
    ) -> Result<mem_api::CaptureTaskResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/capture/task"))
                .headers(write_headers(&self.config.service.api_token)?)
                .json(request)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn curate(&self, project: &str) -> Result<CurateResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/curate"))
                .headers(write_headers(&self.config.service.api_token)?)
                .json(&CurateRequest {
                    project: project.to_string(),
                    batch_size: None,
                })
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn reindex(&self, project: &str) -> Result<ReindexResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/reindex"))
                .headers(write_headers(&self.config.service.api_token)?)
                .json(&ReindexRequest {
                    project: project.to_string(),
                })
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn archive_low_value(&self, project: &str) -> Result<ArchiveResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/archive"))
                .headers(write_headers(&self.config.service.api_token)?)
                .json(&ArchiveRequest {
                    project: project.to_string(),
                    max_confidence: 0.3,
                    max_importance: 1,
                })
                .send()
                .await?,
        )
        .await
    }
}

async fn get_json<T: serde::de::DeserializeOwned>(response: reqwest::Response) -> Result<T> {
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        anyhow::bail!("{status} {body}");
    }
    Ok(serde_json::from_str(&body)?)
}

async fn print_json_response(response: reqwest::Response) -> Result<()> {
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        anyhow::bail!("{status} {body}");
    }
    println!("{body}");
    Ok(())
}

fn print_query_response(payload: QueryResponse) {
    println!("Answer:\n{}\n", payload.answer);
    println!("Confidence: {:.2}\n", payload.confidence);
    for result in payload.results {
        println!(
            "- {} [{}] score={:.2}",
            result.summary, result.memory_type, result.score
        );
        println!("  {}", result.snippet);
        for source in result.sources {
            let path = source.file_path.unwrap_or_else(|| "<no-file>".to_string());
            println!(
                "  source: {} {}",
                path,
                source.source_kind.source_kind_string()
            );
        }
    }
}

fn parse_memory_type(input: String) -> Result<mem_api::MemoryType> {
    match input.as_str() {
        "architecture" => Ok(mem_api::MemoryType::Architecture),
        "convention" => Ok(mem_api::MemoryType::Convention),
        "decision" => Ok(mem_api::MemoryType::Decision),
        "incident" => Ok(mem_api::MemoryType::Incident),
        "debugging" => Ok(mem_api::MemoryType::Debugging),
        "environment" => Ok(mem_api::MemoryType::Environment),
        "domain_fact" => Ok(mem_api::MemoryType::DomainFact),
        _ => anyhow::bail!("unknown memory type: {input}"),
    }
}

fn write_headers(token: &str) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert("x-api-token", token.parse()?);
    Ok(headers)
}

fn service_url(config: &AppConfig, path: &str) -> String {
    format!("http://{}{}", config.service.bind_addr, path)
}

fn resolve_project_slug(project: Option<String>, cwd: &Path) -> Result<String> {
    if let Some(project) = project {
        return Ok(project);
    }
    let Some(name) = cwd.file_name().and_then(|value| value.to_str()) else {
        anyhow::bail!("could not determine project slug from current directory");
    };
    Ok(name.to_string())
}

fn build_remember_request(args: RememberArgs, project: &str) -> Result<CaptureTaskRequest> {
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
        .collect();

    let title = args
        .title
        .unwrap_or_else(|| format!("Memory update for {project}"));
    let prompt = args
        .prompt
        .unwrap_or_else(|| format!("Auto-captured repository work in project {project}."));
    let summary = args
        .summary
        .unwrap_or_else(|| derive_summary(project, &files_changed));

    Ok(CaptureTaskRequest {
        project: project.to_string(),
        task_title: title,
        user_prompt: prompt,
        agent_summary: summary,
        files_changed,
        git_diff_summary: None,
        tests,
        notes: args.notes,
        command_output,
        idempotency_key: None,
    })
}

fn derive_summary(project: &str, files_changed: &[String]) -> String {
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

fn detect_changed_files() -> Result<Vec<String>> {
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

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::{
        RememberArgs, build_remember_request, initialize_repo, is_placeholder_database_url,
        mask_database_url, repair_repo_bootstrap, resolve_project_slug, resolve_repo_root,
        root_gitignore_contains_mem,
    };

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
    fn remember_request_uses_defaults() {
        let request = build_remember_request(
            RememberArgs {
                project: None,
                title: None,
                prompt: None,
                summary: None,
                notes: vec!["durable fact".to_string()],
                files_changed: vec!["src/main.rs".to_string()],
                tests_passed: vec![],
                tests_failed: vec![],
                command_output_file: None,
                auto_files: false,
            },
            "memory",
        )
        .unwrap();

        assert_eq!(request.task_title, "Memory update for memory");
        assert!(request.user_prompt.contains("Auto-captured"));
        assert!(request.agent_summary.contains("src/main.rs"));
    }

    #[test]
    fn init_print_describes_repo_layout() {
        let repo_root = PathBuf::from("/tmp/memory");
        let summary = initialize_repo(&repo_root, "memory", false, true).unwrap();

        assert!(summary.contains(".mem/config.toml"));
        assert!(summary.contains("memory-watch run --project memory"));
        assert!(summary.contains("mem-service"));
    }

    #[test]
    fn init_creates_repo_files_and_gitignore_entry() {
        let repo_root = unique_temp_dir("mem-init");
        fs::create_dir_all(&repo_root).unwrap();

        initialize_repo(&repo_root, "memory", false, false).unwrap();

        assert!(repo_root.join(".mem/config.toml").is_file());
        assert!(repo_root.join(".mem/project.toml").is_file());
        assert!(repo_root.join(".mem/runtime").is_dir());
        assert!(
            fs::read_to_string(repo_root.join(".mem/config.toml"))
                .unwrap()
                .contains("[automation]")
        );
        assert_eq!(
            fs::read_to_string(repo_root.join(".mem/.gitignore")).unwrap(),
            "runtime/\n"
        );
        assert!(
            fs::read_to_string(repo_root.join(".gitignore"))
                .unwrap()
                .contains("/.mem")
        );

        let _ = fs::remove_dir_all(repo_root);
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
        let repo_root = unique_temp_dir("mem-doctor-fix");
        fs::create_dir_all(&repo_root).unwrap();

        repair_repo_bootstrap(&repo_root, "memory").unwrap();

        assert!(repo_root.join(".mem/config.toml").is_file());
        assert!(repo_root.join(".mem/project.toml").is_file());
        assert!(repo_root.join(".mem/runtime").is_dir());
        assert!(root_gitignore_contains_mem(&repo_root).unwrap());

        let _ = fs::remove_dir_all(repo_root);
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
