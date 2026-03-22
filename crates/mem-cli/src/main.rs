mod commits;
mod scan;
mod tui;
mod wizard;

use std::{
    collections::BTreeMap,
    env, fs,
    net::{SocketAddr, TcpStream},
    os::unix::fs::PermissionsExt,
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    time::Duration,
};

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use mem_api::{
    AppConfig, ArchiveRequest, ArchiveResponse, CaptureTaskRequest, CommitDetailResponse,
    CommitSyncRequest, CommitSyncResponse, CurateRequest, CurateResponse, DeleteMemoryRequest,
    DeleteMemoryResponse, MemoryEntryResponse, ProjectCommitsResponse, ProjectMemoriesResponse,
    ProjectOverviewResponse, QueryFilters, QueryRequest, QueryResponse, ReindexRequest,
    ReindexResponse, TestResult, discover_global_config_path, discover_repo_env_path,
};
use mem_watch::{flush_path, load_state, run_once, to_status};
use reqwest::{Client, header::HeaderMap};
use serde::Serialize;
use sqlx::{Row, postgres::PgPoolOptions};
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(name = "memctl", version)]
struct Cli {
    #[arg(long, env = "MEMORY_LAYER_CONFIG")]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Wizard(WizardArgs),
    Init(InitArgs),
    Service(ServiceArgs),
    Watch(WatchArgs),
    Doctor(DoctorArgs),
    Commits(CommitsArgs),
    Query(QueryArgs),
    Scan(ScanArgs),
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
struct WizardArgs {
    #[arg(long)]
    project: Option<String>,
    #[arg(long)]
    global: bool,
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
struct ServiceArgs {
    #[command(subcommand)]
    command: ServiceCommand,
}

#[derive(Debug, Subcommand)]
enum ServiceCommand {
    Enable,
    Disable,
    Status,
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
struct WatchArgs {
    #[command(subcommand)]
    command: WatchCommand,
}

#[derive(Debug, Subcommand)]
enum WatchCommand {
    Enable(WatchProjectArgs),
    Disable(WatchProjectArgs),
    Status(WatchProjectArgs),
}

#[derive(Debug, Args)]
struct WatchProjectArgs {
    #[arg(long)]
    project: Option<String>,
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
struct CommitsArgs {
    #[command(subcommand)]
    command: CommitsCommand,
}

#[derive(Debug, Subcommand)]
enum CommitsCommand {
    Sync(CommitSyncArgs),
    List(CommitListArgs),
    Show(CommitShowArgs),
}

#[derive(Debug, Args)]
struct CommitSyncArgs {
    #[arg(long)]
    project: Option<String>,
    #[arg(long)]
    since: Option<String>,
    #[arg(long)]
    limit: Option<usize>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct CommitListArgs {
    #[arg(long)]
    project: Option<String>,
    #[arg(long, default_value_t = 20)]
    limit: i64,
    #[arg(long, default_value_t = 0)]
    offset: i64,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct CommitShowArgs {
    commit: String,
    #[arg(long)]
    project: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct CaptureTaskArgs {
    #[arg(long)]
    file: PathBuf,
}

#[derive(Debug, Args)]
struct ScanArgs {
    #[arg(long)]
    project: Option<String>,
    #[arg(long)]
    since: Option<String>,
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    json: bool,
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
    Flush(AutomationFlushArgs),
}

#[derive(Debug, Args)]
struct AutomationFlushArgs {
    #[command(flatten)]
    project: ProjectArgs,
    #[arg(long)]
    curate: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let Cli {
        config: cli_config,
        command,
    } = Cli::parse();

    match &command {
        Command::Wizard(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            let repo_root = resolve_repo_root(&cwd)?;
            let project = if repo_root == cwd || repo_root.join(".git").exists() {
                resolve_project_slug(args.project.clone(), &repo_root).ok()
            } else {
                args.project.clone()
            };
            wizard::run(&cwd, &repo_root, project, args.global).await?;
            return Ok(());
        }
        Command::Init(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            let project = resolve_project_slug(args.project.clone(), &cwd)?;
            let repo_root = resolve_repo_root(&cwd)?;
            let output = initialize_repo(&repo_root, &project, args.force, args.print)?;
            println!("{output}");
            return Ok(());
        }
        Command::Service(args) => {
            let config_path = cli_config
                .clone()
                .unwrap_or_else(default_global_config_path);
            let output = match args.command {
                ServiceCommand::Enable => enable_backend_service(&config_path)?,
                ServiceCommand::Disable => disable_backend_service()?,
                ServiceCommand::Status => backend_service_status(&config_path)?,
            };
            println!("{output}");
            return Ok(());
        }
        Command::Watch(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            let repo_root = resolve_repo_root(&cwd)?;
            match &args.command {
                WatchCommand::Enable(args) => {
                    let project = resolve_project_slug(args.project.clone(), &cwd)?;
                    let output = enable_watch_service(&repo_root, &project)?;
                    println!("{output}");
                }
                WatchCommand::Disable(args) => {
                    let project = resolve_project_slug(args.project.clone(), &cwd)?;
                    let output = disable_watch_service(&project)?;
                    println!("{output}");
                }
                WatchCommand::Status(args) => {
                    let project = resolve_project_slug(args.project.clone(), &cwd)?;
                    let output = watch_service_status(&repo_root, &project)?;
                    println!("{output}");
                }
            }
            return Ok(());
        }
        Command::Doctor(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            let repo_root = resolve_repo_root(&cwd)?;
            let project = resolve_project_slug(args.project.clone(), &cwd).unwrap_or_else(|_| {
                repo_root
                    .file_name()
                    .and_then(|v| v.to_str())
                    .unwrap_or("memory")
                    .to_string()
            });
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
        Command::Wizard(_) => unreachable!("wizard is handled before config loading"),
        Command::Init(_) => unreachable!("init is handled before config loading"),
        Command::Service(_) => unreachable!("service is handled before config loading"),
        Command::Watch(_) => unreachable!("watch is handled before config loading"),
        Command::Doctor(_) => unreachable!("doctor is handled before config loading"),
        Command::Commits(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            let repo_root = resolve_repo_root(&cwd)?;
            let api = ApiClient::new(client, config);
            match args.command {
                CommitsCommand::Sync(args) => {
                    let project = resolve_project_slug(args.project, &cwd)?;
                    let commits = commits::collect_git_commits(
                        &repo_root,
                        args.since.as_deref(),
                        args.limit,
                    )?;
                    let response = api
                        .sync_commits(&CommitSyncRequest {
                            project,
                            repo_root: repo_root.display().to_string(),
                            commits,
                        })
                        .await?;
                    if args.json {
                        println!("{}", serde_json::to_string_pretty(&response)?);
                    } else {
                        print_commit_sync_response(&response);
                    }
                }
                CommitsCommand::List(args) => {
                    let project = resolve_project_slug(args.project, &cwd)?;
                    let response = api
                        .project_commits(&project, args.limit.clamp(1, 500), args.offset.max(0))
                        .await?;
                    if args.json {
                        println!("{}", serde_json::to_string_pretty(&response)?);
                    } else {
                        print_project_commits(&response);
                    }
                }
                CommitsCommand::Show(args) => {
                    let project = resolve_project_slug(args.project, &cwd)?;
                    let response = api.project_commit(&project, &args.commit).await?;
                    if args.json {
                        println!("{}", serde_json::to_string_pretty(&response)?);
                    } else {
                        print_commit_detail(&response);
                    }
                }
            }
        }
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
        Command::Scan(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            let repo_root = resolve_repo_root(&cwd)?;
            let project = resolve_project_slug(args.project, &cwd)?;
            let api = ApiClient::new(client, config);
            let report = scan::run_scan(
                &api,
                &repo_root,
                &project,
                args.since.as_deref(),
                args.dry_run,
            )
            .await?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_scan_report(&report);
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
                    let project = resolve_project_slug(Some(args.project.project), &cwd)?;
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
                    run_once(
                        &api.config,
                        &api.client,
                        &project,
                        &repo_root,
                        true,
                        args.curate,
                    )
                    .await?;
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "project": project,
                            "status": "flush_requested",
                            "curate": args.curate
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

fn write_shared_env_file(path: &Path, key: &str, value: &str) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("env file path has no parent"))?;
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    let mut lines = if path.exists() {
        fs::read_to_string(path)
            .with_context(|| format!("read {}", path.display()))?
            .lines()
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let wanted = format!("{key}={value}");
    let mut replaced = false;
    for line in &mut lines {
        if line
            .split_once('=')
            .is_some_and(|(existing, _)| existing.trim() == key)
        {
            *line = wanted.clone();
            replaced = true;
        }
    }
    if !replaced {
        lines.push(wanted);
    }
    let mut content = lines.join("\n");
    if !content.ends_with('\n') {
        content.push('\n');
    }
    fs::write(path, content).with_context(|| format!("write {}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("chmod {}", path.display()))
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

fn shared_env_path_for_config(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("memory-layer.env")
}

fn default_global_config_path() -> PathBuf {
    #[cfg(target_os = "macos")]
    if let Some(path) = macos_app_support_dir() {
        return path.join("memory-layer.toml");
    }

    if let Ok(config_home) = env::var("XDG_CONFIG_HOME") {
        PathBuf::from(config_home)
            .join("memory-layer")
            .join("memory-layer.toml")
    } else if let Ok(home) = env::var("HOME") {
        PathBuf::from(home)
            .join(".config")
            .join("memory-layer")
            .join("memory-layer.toml")
    } else {
        PathBuf::from("/etc/memory-layer/memory-layer.toml")
    }
}

fn default_shared_capnp_unix_socket() -> String {
    #[cfg(target_os = "macos")]
    if let Some(path) = macos_app_support_dir() {
        return path
            .join("run")
            .join("memory-layer.capnp.sock")
            .display()
            .to_string();
    }

    "/tmp/memory-layer.capnp.sock".to_string()
}

fn backend_start_hint(config_path: &Path) -> String {
    if backend_service_available() {
        "mem-cli service enable".to_string()
    } else {
        format!("mem-service {}", config_path.display())
    }
}

fn backend_service_available() -> bool {
    #[cfg(target_os = "macos")]
    {
        mem_service_binary_path().is_ok()
    }

    #[cfg(not(target_os = "macos"))]
    {
        packaged_service_available()
    }
}

#[cfg(not(target_os = "macos"))]
fn packaged_service_available() -> bool {
    Path::new("/lib/systemd/system/memory-layer.service").is_file()
        || Path::new("/etc/systemd/system/memory-layer.service").is_file()
}

#[cfg(not(target_os = "macos"))]
fn run_systemctl_system<const N: usize>(args: [&str; N]) -> Result<()> {
    let output = ProcessCommand::new("systemctl")
        .args(args)
        .output()
        .with_context(|| format!("run systemctl {}", args.join(" ")))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    anyhow::bail!(
        "systemctl {} failed: {}{}{}",
        args.join(" "),
        stderr.trim(),
        if stderr.trim().is_empty() || stdout.trim().is_empty() {
            ""
        } else {
            " | "
        },
        stdout.trim()
    )
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
    let local_service_overrides = read_local_service_overrides(repo_root);

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

        if is_placeholder_database_url(&config.database.url) {
            report.push(doctor_check(
                "database.pgvector_extension",
                DoctorStatus::Skipped,
                "Skipped pgvector checks because the database URL is still a placeholder.",
                None,
                None,
                false,
            ));
        } else {
            match PgPoolOptions::new()
                .max_connections(1)
                .acquire_timeout(Duration::from_secs(3))
                .connect(&config.database.url)
                .await
            {
                Ok(pool) => {
                    report.push(doctor_check(
                        "database.connect",
                        DoctorStatus::Ok,
                        "Database connection succeeded.",
                        Some(mask_database_url(&config.database.url)),
                        None,
                        false,
                    ));

                    match sqlx::query(
                        "SELECT extversion FROM pg_extension WHERE extname = 'vector' LIMIT 1",
                    )
                    .fetch_optional(&pool)
                    .await
                    {
                        Ok(Some(row)) => report.push(doctor_check(
                            "database.pgvector_extension",
                            DoctorStatus::Ok,
                            "pgvector extension is enabled in the target database.",
                            Some(format!(
                                "vector extension version {}",
                                row.try_get::<String, _>("extversion")
                                    .unwrap_or_else(|_| "unknown".to_string())
                            )),
                            None,
                            false,
                        )),
                        Ok(None) => report.push(doctor_check(
                            "database.pgvector_extension",
                            DoctorStatus::Fail,
                            "pgvector extension is not enabled in the target database.",
                            None,
                            Some(
                                "Install pgvector for your PostgreSQL version and run CREATE EXTENSION vector; in the target database."
                                    .to_string(),
                            ),
                            false,
                        )),
                        Err(error) => report.push(doctor_check(
                            "database.pgvector_extension",
                            DoctorStatus::Fail,
                            "Could not verify pgvector extension state.",
                            Some(error.to_string()),
                            Some(
                                "Install pgvector for your PostgreSQL version and run CREATE EXTENSION vector; in the target database."
                                    .to_string(),
                            ),
                            false,
                        )),
                    }
                }
                Err(error) => {
                    report.push(doctor_check(
                        "database.connect",
                        DoctorStatus::Fail,
                        "Could not connect to the configured database directly.",
                        Some(error.to_string()),
                        Some("Fix the database URL or credentials first.".to_string()),
                        false,
                    ));
                    report.push(doctor_check(
                        "database.pgvector_extension",
                        DoctorStatus::Skipped,
                        "Skipped pgvector extension check because the database connection failed.",
                        None,
                        None,
                        false,
                    ));
                }
            }
        }

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

        report.push(doctor_check(
            "config.llm_model",
            if config.llm.model.trim().is_empty() {
                DoctorStatus::Fail
            } else {
                DoctorStatus::Ok
            },
            if config.llm.model.trim().is_empty() {
                "LLM model is not configured."
            } else {
                "LLM model is configured."
            },
            Some(format!(
                "provider={} base_url={}",
                config.llm.provider, config.llm.base_url
            )),
            if config.llm.model.trim().is_empty() {
                Some(format!(
                    "Set [llm].model in {}",
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

        let repo_env_path = discover_repo_env_path();
        let llm_api_key_value = env::var(&config.llm.api_key_env)
            .ok()
            .or_else(|| {
                repo_env_path
                    .as_ref()
                    .and_then(|path| shared_env_lookup(path, &config.llm.api_key_env))
            })
            .or_else(|| {
                global_config_path.as_ref().and_then(|path| {
                    shared_env_lookup(&shared_env_path_for_config(path), &config.llm.api_key_env)
                })
            })
            .unwrap_or_default();
        report.push(doctor_check(
            "config.llm_api_key",
            if llm_api_key_value.trim().is_empty() {
                DoctorStatus::Fail
            } else {
                DoctorStatus::Ok
            },
            if llm_api_key_value.trim().is_empty() {
                "LLM API key environment variable is missing."
            } else {
                "LLM API key environment variable is present."
            },
            Some(config.llm.api_key_env.clone()),
            if llm_api_key_value.trim().is_empty() {
                Some({
                    let mut locations = Vec::new();
                    if let Some(path) = repo_env_path.as_ref() {
                        locations.push(path.display().to_string());
                    }
                    locations.push(
                        global_config_path
                            .as_ref()
                            .map(|path| shared_env_path_for_config(path).display().to_string())
                            .unwrap_or_else(|| {
                                shared_env_path_for_config(&config_path)
                                    .display()
                                    .to_string()
                            }),
                    );
                    format!(
                        "Set {} in {} or export it in your shell",
                        config.llm.api_key_env,
                        locations.join(" or ")
                    )
                })
            } else {
                None
            },
            false,
        ));

        report.push(doctor_check(
            "config.service_endpoints",
            DoctorStatus::Ok,
            if local_service_overrides.is_some() {
                "Repo-local service endpoints are configured."
            } else {
                "Using shared/global service endpoints."
            },
            Some(format!(
                "http={} capnp_tcp={} capnp_unix={}",
                config.service.bind_addr,
                config.service.capnp_tcp_addr,
                config.service.capnp_unix_socket
            )),
            None,
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
                Some(format!(
                    "Edit {} and set [automation].repo_root",
                    config_path.display()
                ))
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
                    Ok(overview) => {
                        report.push(doctor_check(
                            "backend.project_overview",
                            DoctorStatus::Ok,
                            "Project overview endpoint is reachable.",
                            Some(format!(
                                "{} memories / {} raw captures",
                                overview.memory_entries_total, overview.raw_captures_total
                            )),
                            None,
                            false,
                        ));
                        if overview
                            .automation
                            .as_ref()
                            .is_some_and(|automation| automation.enabled)
                        {
                            let active_watchers = overview
                                .watchers
                                .as_ref()
                                .map(|watchers| watchers.active_count);
                            report.push(doctor_check(
                                "backend.watchers",
                                if active_watchers.unwrap_or(0) > 0 {
                                    DoctorStatus::Ok
                                } else {
                                    DoctorStatus::Warn
                                },
                                if active_watchers.unwrap_or(0) > 0 {
                                    "At least one active watcher is visible to the backend."
                                } else {
                                    "Automation is enabled but no active watcher is visible."
                                },
                                active_watchers.map(|count| format!("{count} active watcher(s)")),
                                if active_watchers.unwrap_or(0) > 0 {
                                    None
                                } else {
                                    Some(format!("memctl watch enable --project {}", project))
                                },
                                false,
                            ));
                        }

                        if repo_root.join(".git").exists() {
                            match api.project_commits(project, 1, 0).await {
                                Ok(commits) => report.push(doctor_check(
                                    "history.commit_sync",
                                    if commits.total > 0 {
                                        DoctorStatus::Ok
                                    } else {
                                        DoctorStatus::Warn
                                    },
                                    if commits.total > 0 {
                                        "Commit history has been imported for this project."
                                    } else {
                                        "No commit history has been imported for this project."
                                    },
                                    Some(format!("{} stored commit(s)", commits.total)),
                                    if commits.total > 0 {
                                        None
                                    } else {
                                        Some(format!("memctl commits sync --project {}", project))
                                    },
                                    false,
                                )),
                                Err(error) => report.push(doctor_check(
                                    "history.commit_sync",
                                    DoctorStatus::Warn,
                                    "Could not load project commit history.",
                                    Some(error.to_string()),
                                    Some(format!("memctl commits sync --project {}", project)),
                                    false,
                                )),
                            }
                        }
                    }
                    Err(error) => report.push(doctor_check(
                        "backend.project_overview",
                        DoctorStatus::Warn,
                        "Project overview endpoint did not return data.",
                        Some(error.to_string()),
                        Some(format!("memctl init --project {}", project)),
                        false,
                    )),
                }

                let (http_status, http_details) = tcp_endpoint_status(&config.service.bind_addr);
                report.push(doctor_check(
                    "backend.http_endpoint",
                    if matches!(http_status, DoctorStatus::Fail) {
                        DoctorStatus::Fail
                    } else {
                        DoctorStatus::Ok
                    },
                    "Configured HTTP endpoint is reachable.",
                    Some(http_details),
                    None,
                    false,
                ));

                let (tcp_status, tcp_details) = tcp_endpoint_status(&config.service.capnp_tcp_addr);
                report.push(doctor_check(
                    "backend.capnp_tcp_endpoint",
                    if matches!(tcp_status, DoctorStatus::Fail) {
                        DoctorStatus::Fail
                    } else {
                        DoctorStatus::Ok
                    },
                    "Configured Cap'n Proto TCP endpoint has a listener.",
                    Some(tcp_details),
                    None,
                    false,
                ));

                let (unix_status, unix_details) =
                    unix_socket_status(&config.service.capnp_unix_socket);
                report.push(doctor_check(
                    "backend.capnp_unix_socket",
                    if matches!(unix_status, DoctorStatus::Fail) {
                        DoctorStatus::Fail
                    } else {
                        DoctorStatus::Ok
                    },
                    "Configured Cap'n Proto Unix socket path is active.",
                    Some(unix_details),
                    None,
                    false,
                ));
            }
            Err(error) => {
                report.push(doctor_check(
                    "backend.health",
                    DoctorStatus::Fail,
                    "Backend health endpoint is not reachable.",
                    Some(error.to_string()),
                    Some(backend_start_hint(&config_path)),
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
                report.push(doctor_check(
                    "history.commit_sync",
                    DoctorStatus::Skipped,
                    "Skipped commit history check because the backend is unavailable.",
                    None,
                    None,
                    false,
                ));

                let (http_status, http_details) = tcp_endpoint_status(&config.service.bind_addr);
                report.push(doctor_check(
                    "backend.http_endpoint",
                    http_status,
                    "Configured HTTP endpoint is not serving Memory Layer health.",
                    Some(http_details),
                    Some(format!(
                        "Start the intended backend for {} or change [service].bind_addr",
                        project
                    )),
                    false,
                ));

                let (tcp_status, tcp_details) = tcp_endpoint_status(&config.service.capnp_tcp_addr);
                report.push(doctor_check(
                    "backend.capnp_tcp_endpoint",
                    tcp_status,
                    "Configured Cap'n Proto TCP endpoint is not confirmed healthy.",
                    Some(tcp_details),
                    Some(format!(
                        "Start the intended backend for {} or change [service].capnp_tcp_addr",
                        project
                    )),
                    false,
                ));

                let (unix_status, unix_details) =
                    unix_socket_status(&config.service.capnp_unix_socket);
                report.push(doctor_check(
                    "backend.capnp_unix_socket",
                    unix_status,
                    "Configured Cap'n Proto Unix socket is not confirmed healthy.",
                    Some(unix_details),
                    Some(format!(
                        "Start the intended backend for {} or change [service].capnp_unix_socket",
                        project
                    )),
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
            (
                "config.database_url",
                "Skipped database URL validation because config could not load.",
            ),
            (
                "config.api_token",
                "Skipped API token validation because config could not load.",
            ),
            (
                "automation.runtime_dir",
                "Skipped automation runtime checks because config could not load.",
            ),
            (
                "config.llm_model",
                "Skipped LLM model validation because config could not load.",
            ),
            (
                "config.llm_api_key",
                "Skipped LLM API key validation because config could not load.",
            ),
            (
                "automation.repo_root",
                "Skipped automation repo_root check because config could not load.",
            ),
            (
                "backend.health",
                "Skipped backend health check because config could not load.",
            ),
            (
                "backend.project_overview",
                "Skipped project overview check because config could not load.",
            ),
            (
                "automation.state",
                "Skipped automation state check because config could not load.",
            ),
            (
                "workflow.remember_ready",
                "Skipped remember readiness check because config could not load.",
            ),
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
    let skill_dir = repo_root
        .join(".agents")
        .join("skills")
        .join("memory-layer");

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
    if !skill_dir.exists() {
        let skill_template_dir = discover_skill_template_dir().ok_or_else(|| {
            anyhow::anyhow!("could not locate packaged memory-layer skill template")
        })?;
        copy_skill_template(&skill_template_dir, &skill_dir, false)?;
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

#[derive(Clone, Debug, Default)]
struct LocalServiceOverrides {
    bind_addr: String,
    capnp_tcp_addr: String,
    capnp_unix_socket: String,
}

impl LocalServiceOverrides {
    fn is_enabled(&self) -> bool {
        !self.bind_addr.trim().is_empty()
            || !self.capnp_tcp_addr.trim().is_empty()
            || !self.capnp_unix_socket.trim().is_empty()
    }
}

fn default_local_service_overrides(repo_root: &Path) -> LocalServiceOverrides {
    LocalServiceOverrides {
        bind_addr: "127.0.0.1:4140".to_string(),
        capnp_tcp_addr: "127.0.0.1:4141".to_string(),
        capnp_unix_socket: repo_root
            .join(".mem")
            .join("runtime")
            .join("memory-layer.capnp.sock")
            .display()
            .to_string(),
    }
}

fn read_local_service_overrides(repo_root: &Path) -> Option<LocalServiceOverrides> {
    let config_path = repo_root.join(".mem").join("config.toml");
    let content = fs::read_to_string(config_path).ok()?;
    let mut in_service = false;
    let mut overrides = LocalServiceOverrides::default();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_service = trimmed == "[service]";
            continue;
        }
        if !in_service {
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("bind_addr = ") {
            overrides.bind_addr = value.trim_matches('"').to_string();
        } else if let Some(value) = trimmed.strip_prefix("capnp_tcp_addr = ") {
            overrides.capnp_tcp_addr = value.trim_matches('"').to_string();
        } else if let Some(value) = trimmed.strip_prefix("capnp_unix_socket = ") {
            overrides.capnp_unix_socket = value.trim_matches('"').to_string();
        }
    }

    overrides.is_enabled().then_some(overrides)
}

fn tcp_endpoint_status(addr: &str) -> (DoctorStatus, String) {
    match addr.parse::<SocketAddr>() {
        Ok(socket_addr) => {
            match TcpStream::connect_timeout(&socket_addr, Duration::from_millis(250)) {
                Ok(_) => (DoctorStatus::Warn, format!("listener detected on {addr}")),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    (DoctorStatus::Ok, format!("no listener detected on {addr}"))
                }
                Err(error) => (DoctorStatus::Warn, error.to_string()),
            }
        }
        Err(error) => (
            DoctorStatus::Fail,
            format!("invalid socket address: {error}"),
        ),
    }
}

fn unix_socket_status(path: &str) -> (DoctorStatus, String) {
    let socket_path = Path::new(path);
    if !socket_path.exists() {
        return (DoctorStatus::Ok, "socket path is free".to_string());
    }

    match UnixStream::connect(socket_path) {
        Ok(_) => (
            DoctorStatus::Warn,
            format!("listener detected on {}", socket_path.display()),
        ),
        Err(error) => (
            DoctorStatus::Warn,
            format!("path exists but is not accepting connections: {error}"),
        ),
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
    default_global_config_path().display().to_string()
}

fn initialize_repo(
    repo_root: &Path,
    project: &str,
    force: bool,
    print_only: bool,
) -> Result<String> {
    let mem_dir = repo_root.join(".mem");
    let runtime_dir = mem_dir.join("runtime");
    let config_path = mem_dir.join("config.toml");
    let project_path = mem_dir.join("project.toml");
    let local_gitignore_path = mem_dir.join(".gitignore");
    let root_gitignore_path = repo_root.join(".gitignore");
    let skill_dir = repo_root
        .join(".agents")
        .join("skills")
        .join("memory-layer");
    let skill_template_dir = discover_skill_template_dir()
        .ok_or_else(|| anyhow::anyhow!("could not locate packaged memory-layer skill template"))?;

    if !force {
        for path in [&config_path, &project_path] {
            if path.exists() {
                anyhow::bail!(
                    "{} already exists; rerun with --force to overwrite generated files",
                    path.display()
                );
            }
        }
        if skill_dir.exists() {
            anyhow::bail!(
                "{} already exists; rerun with --force to overwrite generated files",
                skill_dir.display()
            );
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
        fs::write(&local_gitignore_path, mem_gitignore_contents)
            .context("write .mem/.gitignore")?;
        copy_skill_template(&skill_template_dir, &skill_dir, force)?;
        ensure_root_gitignore_entry(&root_gitignore_path, root_gitignore_line)?;
    }

    Ok(render_init_summary(
        repo_root,
        project,
        &config_path,
        &project_path,
        &skill_dir,
        print_only,
    ))
}

fn enable_backend_service(config_path: &Path) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        let plist_path = backend_launch_agent_path()?;
        let _ = bootout_launch_agent(&plist_path, backend_launch_agent_label());
        if plist_path.exists() {
            let _ = fs::remove_file(&plist_path);
        }
        let pid_path = backend_pid_file_path()?;
        let stdout_path = user_memory_layer_log_dir()?.join("mem-service.stdout.log");
        let stderr_path = user_memory_layer_log_dir()?.join("mem-service.stderr.log");
        fs::create_dir_all(
            pid_path
                .parent()
                .ok_or_else(|| anyhow::anyhow!("backend pid path has no parent"))?,
        )
        .with_context(|| format!("create {}", pid_path.display()))?;
        if let Some(pid) = backend_running_pid()? {
            return Ok(format!(
                "Backend process already running.\nPID file: {}\nPID: {}\nConfig: {}",
                pid_path.display(),
                pid,
                config_path.display()
            ));
        }
        let exports = shell_export_prefix()?;
        let program_command = shell_program_invocation(&[
            mem_service_binary_path()?.display().to_string(),
            config_path.display().to_string(),
        ]);
        let shell_command = format!(
            "{exports} nohup {program_command} >>{} 2>>{} </dev/null & echo $! > {}",
            shell_quote_sh(&stdout_path.display().to_string()),
            shell_quote_sh(&stderr_path.display().to_string()),
            shell_quote_sh(&pid_path.display().to_string()),
        );
        let output = ProcessCommand::new("/bin/zsh")
            .args(["-lc", &shell_command])
            .output()
            .context("start backend process")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("start backend process failed: {}", stderr.trim());
        }
        Ok(format!(
            "Started backend process.\nPID file: {}\nConfig: {}\nLogs:\n- {}\n- {}",
            pid_path.display(),
            config_path.display(),
            stdout_path.display(),
            stderr_path.display(),
        ))
    }

    #[cfg(not(target_os = "macos"))]
    {
        run_systemctl_system(["daemon-reload"])?;
        run_systemctl_system(["enable", "--now", "memory-layer.service"])?;
        Ok("Enabled memory-layer.service".to_string())
    }
}

fn disable_backend_service() -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        let plist_path = backend_launch_agent_path()?;
        let _ = bootout_launch_agent(&plist_path, backend_launch_agent_label());
        if plist_path.exists() {
            let _ = fs::remove_file(&plist_path);
        }
        let pid_path = backend_pid_file_path()?;
        if let Some(pid) = backend_running_pid()? {
            let output = ProcessCommand::new("kill")
                .arg(pid.to_string())
                .output()
                .with_context(|| format!("kill backend pid {pid}"))?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("kill {} failed: {}", pid, stderr.trim());
            }
        }
        if pid_path.exists() {
            fs::remove_file(&pid_path).with_context(|| format!("remove {}", pid_path.display()))?;
        }
        Ok(format!(
            "Stopped backend process.\nRemoved pid file: {}",
            pid_path.display()
        ))
    }

    #[cfg(not(target_os = "macos"))]
    {
        run_systemctl_system(["disable", "--now", "memory-layer.service"])?;
        Ok("Disabled memory-layer.service".to_string())
    }
}

fn backend_service_status(config_path: &Path) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        let pid_path = backend_pid_file_path()?;
        let pid = backend_running_pid()?;
        Ok(format!(
            "Backend service:\n- pid file: {}\n- config: {}\n- installed: {}\n- running: {}\n- pid: {}\n\nInspect with:\n- tail -f {}",
            pid_path.display(),
            config_path.display(),
            yes_no(pid_path.exists()),
            yes_no(pid.is_some()),
            pid.map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            user_memory_layer_log_dir()?
                .join("mem-service.stderr.log")
                .display(),
        ))
    }

    #[cfg(not(target_os = "macos"))]
    {
        let is_installed = packaged_service_available();
        let is_enabled = run_systemctl_system(["is-enabled", "memory-layer.service"]).is_ok();
        let is_active = run_systemctl_system(["is-active", "memory-layer.service"]).is_ok();
        Ok(format!(
            "Backend service:\n- unit: memory-layer.service\n- config: {}\n- installed: {}\n- enabled: {}\n- active: {}\n\nInspect with:\n- systemctl status memory-layer.service",
            config_path.display(),
            yes_no(is_installed),
            yes_no(is_enabled),
            yes_no(is_active),
        ))
    }
}

fn enable_watch_service(repo_root: &Path, project: &str) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        let plist_path = watch_launch_agent_path(project)?;
        write_launch_agent(
            &plist_path,
            render_watch_launch_agent(repo_root, project)?,
            &watch_launch_agent_label(project),
        )?;
        bootstrap_launch_agent(&plist_path, &watch_launch_agent_label(project))?;
        Ok(format!(
            "Installed and started watcher LaunchAgent {}.\nPlist: {}\nRepo: {}\nProject: {}\n\nManage it with:\n- mem-cli watch status --project {}\n- mem-cli watch disable --project {}\n- launchctl kickstart -k {}/{}",
            watch_launch_agent_label(project),
            plist_path.display(),
            repo_root.display(),
            project,
            project,
            project,
            launchctl_domain_target()?,
            watch_launch_agent_label(project),
        ))
    }

    #[cfg(not(target_os = "macos"))]
    {
        let unit_name = watch_unit_name(project);
        let unit_dir = user_systemd_unit_dir()?;
        let unit_path = unit_dir.join(&unit_name);
        fs::create_dir_all(&unit_dir).with_context(|| format!("create {}", unit_dir.display()))?;
        fs::write(&unit_path, render_watch_unit(repo_root, project)?)
            .with_context(|| format!("write {}", unit_path.display()))?;
        run_systemctl_user(["daemon-reload"])?;
        run_systemctl_user(["enable", "--now", &unit_name])?;
        Ok(format!(
            "Installed and started user service {}.\nUnit: {}\nRepo: {}\nProject: {}\n\nManage it with:\n- mem-cli watch status --project {}\n- mem-cli watch disable --project {}\n- systemctl --user restart {}",
            unit_name,
            unit_path.display(),
            repo_root.display(),
            project,
            project,
            project,
            unit_name
        ))
    }
}

fn disable_watch_service(project: &str) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        let plist_path = watch_launch_agent_path(project)?;
        let label = watch_launch_agent_label(project);
        let _ = bootout_launch_agent(&plist_path, &label);
        if plist_path.exists() {
            fs::remove_file(&plist_path)
                .with_context(|| format!("remove {}", plist_path.display()))?;
        }
        Ok(format!(
            "Disabled watcher LaunchAgent {}.\nRemoved plist: {}",
            label,
            plist_path.display()
        ))
    }

    #[cfg(not(target_os = "macos"))]
    {
        let unit_name = watch_unit_name(project);
        let unit_path = user_systemd_unit_dir()?.join(&unit_name);
        let _ = run_systemctl_user(["disable", "--now", &unit_name]);
        if unit_path.exists() {
            fs::remove_file(&unit_path)
                .with_context(|| format!("remove {}", unit_path.display()))?;
        }
        run_systemctl_user(["daemon-reload"])?;
        Ok(format!(
            "Disabled user service {}.\nRemoved unit: {}",
            unit_name,
            unit_path.display()
        ))
    }
}

fn watch_service_status(repo_root: &Path, project: &str) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        let plist_path = watch_launch_agent_path(project)?;
        let label = watch_launch_agent_label(project);
        let status = launch_agent_status(&label)?;
        Ok(format!(
            "Watcher service for project {}:\n- label: {}\n- plist: {}\n- repo: {}\n- installed: {}\n- loaded: {}\n- running: {}\n\nInspect with:\n- launchctl print {}/{}",
            project,
            label,
            plist_path.display(),
            repo_root.display(),
            yes_no(plist_path.exists()),
            yes_no(status.loaded),
            yes_no(status.running),
            launchctl_domain_target()?,
            label
        ))
    }

    #[cfg(not(target_os = "macos"))]
    {
        let unit_name = watch_unit_name(project);
        let unit_path = user_systemd_unit_dir()?.join(&unit_name);
        let is_enabled = run_systemctl_user(["is-enabled", &unit_name]).is_ok();
        let is_active = run_systemctl_user(["is-active", &unit_name]).is_ok();
        Ok(format!(
            "Watcher service for project {}:\n- unit: {}\n- repo: {}\n- installed: {}\n- enabled: {}\n- active: {}\n\nInspect with:\n- systemctl --user status {}",
            project,
            unit_path.display(),
            repo_root.display(),
            yes_no(unit_path.exists()),
            yes_no(is_enabled),
            yes_no(is_active),
            unit_name
        ))
    }
}

#[cfg(not(target_os = "macos"))]
fn render_watch_unit(repo_root: &Path, project: &str) -> Result<String> {
    let watch_binary = memory_watch_binary_path()?;
    let env_file = user_memory_layer_env_file()?;
    let working_directory = repo_root
        .canonicalize()
        .with_context(|| format!("canonicalize {}", repo_root.display()))?;
    Ok(format!(
        "[Unit]\nDescription=Memory Layer Watcher ({project})\nAfter=default.target\n\n[Service]\nType=simple\nEnvironmentFile=-{}\nWorkingDirectory={}\nExecStart={} run --project {}\nRestart=on-failure\nRestartSec=2\n\n[Install]\nWantedBy=default.target\n",
        shell_escape_path(&env_file),
        working_directory.display(),
        shell_escape_path(&watch_binary),
        shell_escape_str(project),
    ))
}

#[cfg(not(target_os = "macos"))]
fn user_systemd_unit_dir() -> Result<PathBuf> {
    if let Ok(config_home) = env::var("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(config_home).join("systemd").join("user"));
    }
    let home = env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join("systemd")
        .join("user"))
}

fn user_memory_layer_env_file() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        return Ok(macos_app_support_dir()
            .ok_or_else(|| anyhow::anyhow!("HOME is not set"))?
            .join("memory-layer.env"));
    }

    #[cfg(not(target_os = "macos"))]
    {
        if let Ok(config_home) = env::var("XDG_CONFIG_HOME") {
            return Ok(PathBuf::from(config_home)
                .join("memory-layer")
                .join("memory-layer.env"));
        }
        let home = env::var("HOME").context("HOME is not set")?;
        Ok(PathBuf::from(home)
            .join(".config")
            .join("memory-layer")
            .join("memory-layer.env"))
    }
}

fn mem_service_binary_path() -> Result<PathBuf> {
    let current_exe = env::current_exe().context("locate current executable")?;
    if let Some(bin_dir) = current_exe.parent() {
        let sibling = bin_dir.join("mem-service");
        if sibling.is_file() {
            return Ok(sibling);
        }
    }
    Ok(PathBuf::from("mem-service"))
}

fn memory_watch_binary_path() -> Result<PathBuf> {
    let current_exe = env::current_exe().context("locate current executable")?;
    if let Some(bin_dir) = current_exe.parent() {
        let sibling = bin_dir.join("memory-watch");
        if sibling.is_file() {
            return Ok(sibling);
        }
    }
    Ok(PathBuf::from("memory-watch"))
}

#[cfg(not(target_os = "macos"))]
fn watch_unit_name(project: &str) -> String {
    let sanitized = sanitize_service_fragment(project);
    format!("memory-watch-{}.service", sanitized)
}

fn sanitize_service_fragment(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => ch,
            _ => '-',
        })
        .collect::<String>()
}

#[cfg(target_os = "macos")]
#[derive(Debug, Default)]
struct LaunchAgentStatus {
    loaded: bool,
    running: bool,
}

#[cfg(target_os = "macos")]
fn backend_launch_agent_label() -> &'static str {
    "com.memory-layer.mem-service"
}

#[cfg(target_os = "macos")]
fn watch_launch_agent_label(project: &str) -> String {
    format!(
        "com.memory-layer.memory-watch.{}",
        sanitize_service_fragment(project)
    )
}

#[cfg(target_os = "macos")]
fn backend_launch_agent_path() -> Result<PathBuf> {
    Ok(user_launch_agents_dir()?.join(format!("{}.plist", backend_launch_agent_label())))
}

#[cfg(target_os = "macos")]
fn backend_pid_file_path() -> Result<PathBuf> {
    Ok(macos_app_support_dir()
        .ok_or_else(|| anyhow::anyhow!("HOME is not set"))?
        .join("run")
        .join("mem-service.pid"))
}

#[cfg(target_os = "macos")]
fn watch_launch_agent_path(project: &str) -> Result<PathBuf> {
    Ok(user_launch_agents_dir()?.join(format!("{}.plist", watch_launch_agent_label(project))))
}

#[cfg(target_os = "macos")]
fn user_launch_agents_dir() -> Result<PathBuf> {
    let home = env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join("Library").join("LaunchAgents"))
}

#[cfg(target_os = "macos")]
fn macos_app_support_dir() -> Option<PathBuf> {
    let home = env::var("HOME").ok()?;
    Some(
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("memory-layer"),
    )
}

#[cfg(target_os = "macos")]
fn user_memory_layer_log_dir() -> Result<PathBuf> {
    Ok(macos_app_support_dir()
        .ok_or_else(|| anyhow::anyhow!("HOME is not set"))?
        .join("logs"))
}

#[cfg(target_os = "macos")]
fn launchctl_domain_target() -> Result<String> {
    let output = ProcessCommand::new("id")
        .arg("-u")
        .output()
        .context("run id -u")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("id -u failed: {}", stderr.trim());
    }
    let uid = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(format!("gui/{uid}"))
}

#[cfg(target_os = "macos")]
fn backend_running_pid() -> Result<Option<u32>> {
    let pid_path = backend_pid_file_path()?;
    if !pid_path.exists() {
        return Ok(None);
    }
    let content =
        fs::read_to_string(&pid_path).with_context(|| format!("read {}", pid_path.display()))?;
    let pid = match content.trim().parse::<u32>() {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    let status = ProcessCommand::new("kill")
        .args(["-0", &pid.to_string()])
        .output()
        .with_context(|| format!("check backend pid {pid}"))?;
    if status.status.success() {
        Ok(Some(pid))
    } else {
        Ok(None)
    }
}

#[cfg(target_os = "macos")]
fn write_launch_agent(path: &Path, contents: String, label: &str) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("launch agent path has no parent"))?;
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    fs::write(path, contents).with_context(|| format!("write {}", path.display()))?;
    let _ = bootout_launch_agent(path, label);
    Ok(())
}

#[cfg(target_os = "macos")]
fn bootstrap_launch_agent(path: &Path, label: &str) -> Result<()> {
    run_launchctl([
        "bootstrap",
        &launchctl_domain_target()?,
        &path.display().to_string(),
    ])?;
    run_launchctl([
        "kickstart",
        "-k",
        &format!("{}/{}", launchctl_domain_target()?, label),
    ])?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn bootout_launch_agent(path: &Path, label: &str) -> Result<()> {
    let target = format!("{}/{}", launchctl_domain_target()?, label);
    if run_launchctl(["bootout", &target]).is_ok() {
        return Ok(());
    }
    run_launchctl([
        "bootout",
        &launchctl_domain_target()?,
        &path.display().to_string(),
    ])
}

#[cfg(target_os = "macos")]
fn launch_agent_status(label: &str) -> Result<LaunchAgentStatus> {
    let target = format!("{}/{}", launchctl_domain_target()?, label);
    let output = ProcessCommand::new("launchctl")
        .args(["print", &target])
        .output()
        .with_context(|| format!("run launchctl print {target}"))?;
    if !output.status.success() {
        return Ok(LaunchAgentStatus::default());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(LaunchAgentStatus {
        loaded: true,
        running: stdout.contains("state = running") || stdout.contains("\"PID\" ="),
    })
}

#[cfg(target_os = "macos")]
fn run_launchctl<const N: usize>(args: [&str; N]) -> Result<()> {
    let output = ProcessCommand::new("launchctl")
        .args(args)
        .output()
        .with_context(|| format!("run launchctl {}", args.join(" ")))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    anyhow::bail!(
        "launchctl {} failed: {}{}{}",
        args.join(" "),
        stderr.trim(),
        if stderr.trim().is_empty() || stdout.trim().is_empty() {
            ""
        } else {
            " | "
        },
        stdout.trim()
    )
}

#[cfg(target_os = "macos")]
fn render_backend_launch_agent(config_path: &Path) -> Result<String> {
    let binary = mem_service_binary_path()?;
    let working_directory =
        macos_app_support_dir().ok_or_else(|| anyhow::anyhow!("HOME is not set"))?;
    let stdout_path = user_memory_layer_log_dir()?.join("mem-service.stdout.log");
    let stderr_path = user_memory_layer_log_dir()?.join("mem-service.stderr.log");
    let command = launch_agent_shell_command(&[
        binary.display().to_string(),
        config_path.display().to_string(),
    ])?;
    render_launch_agent_plist(
        backend_launch_agent_label(),
        &working_directory,
        &command,
        &stdout_path,
        &stderr_path,
    )
}

#[cfg(target_os = "macos")]
fn render_watch_launch_agent(repo_root: &Path, project: &str) -> Result<String> {
    let binary = memory_watch_binary_path()?;
    let working_directory = repo_root
        .canonicalize()
        .with_context(|| format!("canonicalize {}", repo_root.display()))?;
    let log_dir = user_memory_layer_log_dir()?;
    let sanitized = sanitize_service_fragment(project);
    let stdout_path = log_dir.join(format!("memory-watch-{sanitized}.stdout.log"));
    let stderr_path = log_dir.join(format!("memory-watch-{sanitized}.stderr.log"));
    let command = launch_agent_shell_command(&[
        binary.display().to_string(),
        "--config".to_string(),
        default_global_config_path().display().to_string(),
        "run".to_string(),
        "--project".to_string(),
        project.to_string(),
    ])?;
    render_launch_agent_plist(
        &watch_launch_agent_label(project),
        &working_directory,
        &command,
        &stdout_path,
        &stderr_path,
    )
}

#[cfg(target_os = "macos")]
fn shell_export_prefix() -> Result<String> {
    let env_vars = launch_agent_environment_variables()?;
    let mut command = String::new();
    for (key, value) in env_vars {
        command.push_str("export ");
        command.push_str(&key);
        command.push('=');
        command.push_str(&shell_quote_sh(&value));
        command.push_str("; ");
    }
    Ok(command)
}

#[cfg(target_os = "macos")]
fn shell_program_invocation(program_arguments: &[String]) -> String {
    let mut command = String::new();
    let mut first = true;
    for arg in program_arguments {
        if !first {
            command.push(' ');
        }
        first = false;
        command.push_str(&shell_quote_sh(arg));
    }
    command
}

#[cfg(target_os = "macos")]
fn shell_command_for_program(program_arguments: &[String], exec_program: bool) -> Result<String> {
    let mut command = shell_export_prefix()?;
    if exec_program {
        command.push_str("exec");
        command.push(' ');
    }
    command.push_str(&shell_program_invocation(program_arguments));
    Ok(command)
}

#[cfg(target_os = "macos")]
fn launch_agent_shell_command(program_arguments: &[String]) -> Result<String> {
    shell_command_for_program(program_arguments, true)
}

#[cfg(target_os = "macos")]
fn launch_agent_environment_variables() -> Result<BTreeMap<String, String>> {
    let mut values = BTreeMap::new();
    values.insert("HOME".to_string(), env::var("HOME").context("HOME is not set")?);
    values.insert(
        "PATH".to_string(),
        env::var("PATH")
            .unwrap_or_else(|_| "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin".to_string()),
    );
    if let Ok(user) = env::var("USER") {
        values.insert("USER".to_string(), user.clone());
        values.insert("LOGNAME".to_string(), user);
    }
    let env_file = user_memory_layer_env_file()?;
    if !env_file.exists() {
        return Ok(values);
    }
    let content =
        fs::read_to_string(&env_file).with_context(|| format!("read {}", env_file.display()))?;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            values.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    Ok(values)
}

#[cfg(target_os = "macos")]
fn render_launch_agent_plist(
    label: &str,
    working_directory: &Path,
    shell_command: &str,
    stdout_path: &Path,
    stderr_path: &Path,
) -> Result<String> {
    let log_dir = stdout_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("stdout log path has no parent"))?;
    fs::create_dir_all(log_dir).with_context(|| format!("create {}", log_dir.display()))?;
    let env_vars = launch_agent_environment_variables()?;
    let program_arguments = [
        "/bin/zsh".to_string(),
        "-lc".to_string(),
        shell_command.to_string(),
    ];
    let args_xml = program_arguments
        .iter()
        .map(|arg| format!("    <string>{}</string>", xml_escape(arg)))
        .collect::<Vec<_>>()
        .join("\n");
    let env_xml = env_vars
        .iter()
        .map(|(key, value)| {
            format!(
                "    <key>{}</key>\n    <string>{}</string>",
                xml_escape(key),
                xml_escape(value)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
{args_xml}
  </array>
  <key>WorkingDirectory</key>
  <string>{working_directory}</string>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>{stdout_path}</string>
  <key>StandardErrorPath</key>
  <string>{stderr_path}</string>
  <key>EnvironmentVariables</key>
  <dict>
{env_xml}
  </dict>
</dict>
</plist>
"#,
        label = xml_escape(label),
        args_xml = args_xml,
        working_directory = xml_escape(&working_directory.display().to_string()),
        stdout_path = xml_escape(&stdout_path.display().to_string()),
        stderr_path = xml_escape(&stderr_path.display().to_string()),
        env_xml = env_xml,
    ))
}

#[cfg(target_os = "macos")]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(target_os = "macos")]
fn shell_quote_sh(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

#[cfg(not(target_os = "macos"))]
fn run_systemctl_user<const N: usize>(args: [&str; N]) -> Result<()> {
    let output = ProcessCommand::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .with_context(|| format!("run systemctl --user {}", args.join(" ")))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    anyhow::bail!(
        "systemctl --user {} failed: {}{}{}",
        args.join(" "),
        stderr.trim(),
        if stderr.trim().is_empty() || stdout.trim().is_empty() {
            ""
        } else {
            " | "
        },
        stdout.trim()
    )
}

#[cfg(not(target_os = "macos"))]
fn shell_escape_path(value: &Path) -> String {
    shell_escape_str(&value.display().to_string())
}

#[cfg(not(target_os = "macos"))]
fn shell_escape_str(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-'))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn discover_skill_template_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(exe) = env::current_exe() {
        if let Some(bin_dir) = exe.parent() {
            if let Some(prefix) = bin_dir.parent() {
                candidates.push(
                    prefix
                        .join("share")
                        .join("memory-layer")
                        .join("skill-template"),
                );
            }
        }
    }
    if let Ok(data_home) = env::var("XDG_DATA_HOME") {
        candidates.push(
            PathBuf::from(data_home)
                .join("memory-layer")
                .join("skill-template"),
        );
    }
    if let Ok(home) = env::var("HOME") {
        candidates.push(
            PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("memory-layer")
                .join("skill-template"),
        );
    }
    candidates.push(PathBuf::from("/usr/share/memory-layer/skill-template"));
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join(".agents")
            .join("skills")
            .join("memory-layer"),
    );

    candidates.into_iter().find(|path| path.is_dir())
}

fn copy_skill_template(src: &Path, dest: &Path, force: bool) -> Result<()> {
    if dest.exists() {
        if force {
            fs::remove_dir_all(dest).with_context(|| format!("remove {}", dest.display()))?;
        } else {
            anyhow::bail!(
                "{} already exists; rerun with --force to overwrite generated files",
                dest.display()
            );
        }
    }
    copy_directory_tree(src, dest)
}

fn copy_directory_tree(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest).with_context(|| format!("create {}", dest.display()))?;
    for entry in fs::read_dir(src).with_context(|| format!("read {}", src.display()))? {
        let entry = entry.with_context(|| format!("read entry in {}", src.display()))?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        let file_type = entry
            .file_type()
            .with_context(|| format!("read type for {}", src_path.display()))?;
        if file_type.is_dir() {
            copy_directory_tree(&src_path, &dest_path)?;
        } else if file_type.is_file() {
            fs::copy(&src_path, &dest_path).with_context(|| {
                format!("copy {} -> {}", src_path.display(), dest_path.display())
            })?;
            let mode = if src_path
                .components()
                .any(|component| component.as_os_str() == "scripts")
            {
                0o755
            } else {
                0o644
            };
            fs::set_permissions(&dest_path, fs::Permissions::from_mode(mode))
                .with_context(|| format!("chmod {}", dest_path.display()))?;
        }
    }
    Ok(())
}

fn render_repo_config(repo_root: &Path) -> String {
    let repo_root = repo_root.display();
    format!(
        r#"# Repo-local overrides for this project.
# Put shared defaults and secrets in the global config:
#   {}
# Shared LLM settings for `memctl scan` should also live there under [llm].

# Uncomment [service] to run a repo-local dev backend alongside the shared one.
# Example dev endpoints:
# [service]
# bind_addr = "127.0.0.1:4140"
# capnp_unix_socket = "{repo_root}/.mem/runtime/memory-layer.capnp.sock"
# capnp_tcp_addr = "127.0.0.1:4141"

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

    if !content
        .lines()
        .any(|existing| existing.trim() == line.trim())
    {
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
    skill_path: &Path,
    print_only: bool,
) -> String {
    let action = if print_only {
        "Would create"
    } else {
        "Created"
    };
    format!(
        "{action} repo-local memory bootstrap for project `{project}` at {}.\n\nFiles:\n- {}\n- {}\n- {}/runtime/\n- {}\n\nNext steps:\n1. Set shared values like `database.url`, `service.api_token`, and `[llm]` config in {}\n2. Use {} only for repo-specific overrides\n3. Start the shared backend if it is not already running:\n   mem-service\n4. Optional: configure repo-local [service] overrides if you want a parallel dev backend for this repo\n5. Optional: run a project scan:\n   mem-cli scan --project {}\n6. Optional: enable the per-repo watcher user service:\n   mem-cli watch enable --project {}\n7. Open the TUI:\n   mem-cli tui --project {}\n8. Use the repo-local skill from {}",
        repo_root.display(),
        config_path.display(),
        project_path.display(),
        config_path.parent().unwrap_or(repo_root).display(),
        skill_path.display(),
        default_global_config_path_label(),
        config_path.display(),
        project,
        project,
        project,
        skill_path.display()
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

    pub(crate) async fn project_commits(
        &self,
        project: &str,
        limit: i64,
        offset: i64,
    ) -> Result<ProjectCommitsResponse> {
        get_json(
            self.client
                .get(service_url(
                    &self.config,
                    &format!("/v1/projects/{project}/commits?limit={limit}&offset={offset}"),
                ))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn project_commit(
        &self,
        project: &str,
        commit: &str,
    ) -> Result<CommitDetailResponse> {
        get_json(
            self.client
                .get(service_url(
                    &self.config,
                    &format!("/v1/projects/{project}/commits/{commit}"),
                ))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn query(&self, request: &QueryRequest) -> Result<QueryResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/query"))
                .json(request)
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

    pub(crate) async fn sync_commits(
        &self,
        request: &CommitSyncRequest,
    ) -> Result<CommitSyncResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/commits/sync"))
                .headers(write_headers(&self.config.service.api_token)?)
                .json(request)
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

    pub(crate) async fn delete_memory(&self, memory_id: Uuid) -> Result<DeleteMemoryResponse> {
        get_json(
            self.client
                .delete(service_url(&self.config, "/v1/memory"))
                .headers(write_headers(&self.config.service.api_token)?)
                .json(&DeleteMemoryRequest { memory_id })
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
    println!(
        "Confidence: {:.2} | Evidence: {}\n",
        payload.confidence,
        if payload.insufficient_evidence {
            "insufficient"
        } else {
            "sufficient"
        }
    );
    println!(
        "Diagnostics: lexical {} ({} ms) | semantic {} ({} ms) | merged {} | returned {} | rerank {} ms | total {} ms\n",
        payload.diagnostics.lexical_candidates,
        payload.diagnostics.lexical_duration_ms,
        payload.diagnostics.semantic_candidates,
        payload.diagnostics.semantic_duration_ms,
        payload.diagnostics.merged_candidates,
        payload.diagnostics.returned_results,
        payload.diagnostics.rerank_duration_ms,
        payload.diagnostics.total_duration_ms,
    );
    for result in payload.results {
        println!(
            "- {} [{} / {}] score={:.2}",
            result.summary, result.memory_type, result.match_kind, result.score
        );
        println!("  {}", result.snippet);
        println!(
            "  debug: chunk {:.2} | entry {:.2} | semantic {:.2} | relation {:.2}",
            result.debug.chunk_fts,
            result.debug.entry_fts,
            result.debug.semantic_similarity,
            result.debug.relation_boost,
        );
        if !result.score_explanation.is_empty() {
            println!("  why: {}", result.score_explanation.join(" | "));
        }
        if !result.tags.is_empty() {
            println!("  tags: {}", result.tags.join(", "));
        }
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

fn print_scan_report(report: &scan::ScanReport) {
    println!("Scan summary:\n{}\n", report.summary);
    println!(
        "Project: {} | Files: {} | Commits: {} | Candidates: {} | Written: {}",
        report.project,
        report.files_considered,
        report.commits_considered,
        report.candidate_count,
        if report.written { "yes" } else { "no" }
    );
    println!("Report: {}", report.report_path);
    if let Some(capture_id) = &report.capture_id {
        println!("Capture: {capture_id}");
    }
    if let Some(run_id) = &report.curate_run_id {
        println!("Curate run: {run_id}");
    }
}

fn print_commit_sync_response(response: &CommitSyncResponse) {
    println!(
        "Commit sync complete: {} imported, {} updated, {} received.",
        response.imported_count, response.updated_count, response.total_received
    );
    if let Some(newest) = &response.newest_commit {
        println!("Newest commit: {newest}");
    }
    if let Some(oldest) = &response.oldest_commit {
        println!("Oldest commit: {oldest}");
    }
}

fn print_project_commits(response: &ProjectCommitsResponse) {
    println!(
        "Project {} commit history (showing {} / {}):",
        response.project,
        response.items.len(),
        response.total
    );
    for commit in &response.items {
        println!(
            "- {} {} ({})",
            commit.short_hash,
            commit.subject,
            commit.committed_at.format("%Y-%m-%d %H:%M UTC")
        );
        if let Some(author) = &commit.author_name {
            println!("  author: {author}");
        }
        if !commit.changed_paths.is_empty() {
            println!("  files: {}", commit.changed_paths.join(", "));
        }
    }
}

fn print_commit_detail(response: &CommitDetailResponse) {
    let commit = &response.commit;
    println!("Project: {}", response.project);
    println!("Commit: {} ({})", commit.hash, commit.short_hash);
    println!("When: {}", commit.committed_at.format("%Y-%m-%d %H:%M UTC"));
    if let Some(author) = &commit.author_name {
        if let Some(email) = &commit.author_email {
            println!("Author: {author} <{email}>");
        } else {
            println!("Author: {author}");
        }
    }
    println!("Subject: {}", commit.subject);
    if !commit.body.trim().is_empty() {
        println!("\nBody:\n{}", commit.body);
    }
    if !commit.parent_hashes.is_empty() {
        println!("\nParents: {}", commit.parent_hashes.join(", "));
    }
    if !commit.changed_paths.is_empty() {
        println!("\nChanged paths:");
        for path in &commit.changed_paths {
            println!("- {path}");
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
        structured_candidates: Vec::new(),
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
        RememberArgs, backend_service_available, build_remember_request, initialize_repo,
        is_placeholder_database_url, mask_database_url, repair_repo_bootstrap,
        resolve_project_slug, resolve_repo_root, root_gitignore_contains_mem,
        sanitize_service_fragment,
    };

    #[cfg(target_os = "macos")]
    use super::{
        backend_launch_agent_label, default_global_config_path, render_backend_launch_agent,
        render_watch_launch_agent, watch_launch_agent_label,
    };

    #[cfg(not(target_os = "macos"))]
    use super::{render_watch_unit, watch_unit_name};

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
        assert!(summary.contains(".agents/skills/memory-layer"));
        assert!(summary.contains("mem-cli watch enable --project memory"));
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
            repo_root
                .join(".agents/skills/memory-layer/SKILL.md")
                .is_file()
        );
        assert!(
            repo_root
                .join(".agents/skills/memory-layer/scripts/remember-task.sh")
                .is_file()
        );
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
        assert!(
            repo_root
                .join(".agents/skills/memory-layer/SKILL.md")
                .is_file()
        );
        assert!(root_gitignore_contains_mem(&repo_root).unwrap());

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

    #[cfg(target_os = "macos")]
    #[test]
    fn launch_agent_labels_are_project_scoped() {
        assert_eq!(backend_launch_agent_label(), "com.memory-layer.mem-service");
        assert_eq!(
            watch_launch_agent_label("customer portal"),
            "com.memory-layer.memory-watch.customer-portal"
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

        assert!(backend_service_available());
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
