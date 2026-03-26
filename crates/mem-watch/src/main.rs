use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use mem_api::AppConfig;
use mem_watch::{
    build_watcher_heartbeat_request, build_watcher_unregister_request, detect_hostname, flush_path,
    fetch_service_instance_id, heartbeat_watcher, load_state, run_once, to_status,
    unregister_watcher,
};
use reqwest::Client;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HeartbeatState {
    Unknown,
    Healthy,
    Failing,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct BackendInstanceState {
    current: Option<String>,
}

#[derive(Debug, Parser)]
#[command(name = "memory-watch", version)]
struct Cli {
    #[arg(long, env = "MEMORY_LAYER_CONFIG")]
    config: Option<PathBuf>,
    #[arg(
        long = "writer-id",
        visible_alias = "agent-id",
        env = "MEMORY_LAYER_WRITER_ID"
    )]
    writer_id: Option<String>,
    #[command(subcommand)]
    command: WatchCommand,
}

#[derive(Debug, Subcommand)]
enum WatchCommand {
    Run(RunArgs),
    Status(ProjectArgs),
    Flush(FlushArgs),
}

#[derive(Debug, Args)]
struct RunArgs {
    #[arg(long)]
    project: Option<String>,
    #[arg(long)]
    repo_root: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ProjectArgs {
    #[arg(long)]
    project: Option<String>,
    #[arg(long)]
    repo_root: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct FlushArgs {
    #[arg(long)]
    project: Option<String>,
    #[arg(long)]
    repo_root: Option<PathBuf>,
    #[arg(long)]
    curate: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = AppConfig::load_from_path(cli.config).context("load config")?;
    let writer_id = resolve_writer_id(&config, cli.writer_id)?;
    let writer_name = config.writer.name.clone();

    match cli.command {
        WatchCommand::Run(args) => run_loop(config, args, writer_id, writer_name).await,
        WatchCommand::Status(args) => status(config, args).await,
        WatchCommand::Flush(args) => flush(config, args, writer_id, writer_name).await,
    }
}

async fn run_loop(
    config: AppConfig,
    args: RunArgs,
    writer_id: String,
    writer_name: Option<String>,
) -> Result<()> {
    let repo_root = resolve_repo_root(&config, args.repo_root)?;
    let project = resolve_project(args.project, &repo_root)?;
    let client = Client::new();
    let state = load_state(&project, &repo_root, &config.automation).await?;
    let watcher_id = Uuid::new_v4().to_string();
    let hostname = detect_hostname();
    let host_service_id = config.cluster.service_id.clone();
    let managed_by_service = watcher_is_service_managed();
    let pid = std::process::id();
    let started_at = chrono::Utc::now();

    let heartbeat_request = build_watcher_heartbeat_request(
        &state,
        &watcher_id,
        &hostname,
        &host_service_id,
        managed_by_service,
        pid,
        started_at,
    );
    let mut heartbeat_state = HeartbeatState::Unknown;
    let mut backend_instance = BackendInstanceState {
        current: fetch_service_instance_id(&client, &config).await.ok().flatten(),
    };
    heartbeat_state = log_heartbeat_transition(
        heartbeat_state,
        heartbeat_watcher(&client, &config, &heartbeat_request).await,
    );

    let mut poll = tokio::time::interval(config.automation.poll_interval);
    let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(30));

    loop {
        tokio::select! {
            _ = poll.tick() => {
                run_once(
                    &config,
                    &client,
                    &project,
                    &repo_root,
                    false,
                    false,
                    &writer_id,
                    writer_name.as_deref(),
                ).await?;
            }
            _ = heartbeat.tick() => {
                let state = load_state(&project, &repo_root, &config.automation).await?;
                let request = build_watcher_heartbeat_request(
                    &state,
                    &watcher_id,
                    &hostname,
                    &host_service_id,
                    managed_by_service,
                    pid,
                    started_at,
                );
                heartbeat_state = log_heartbeat_transition(
                    heartbeat_state,
                    heartbeat_watcher(&client, &config, &request).await,
                );
                update_backend_instance_state(
                    &mut backend_instance,
                    fetch_service_instance_id(&client, &config).await?,
                )?;
            }
            _ = shutdown_signal() => {
                let request = build_watcher_unregister_request(&project, &watcher_id);
                if let Err(error) = unregister_watcher(&client, &config, &request).await {
                    eprintln!("watcher unregister failed: {error}");
                }
                break;
            }
        }
    }
    Ok(())
}

fn update_backend_instance_state(
    state: &mut BackendInstanceState,
    current: Option<String>,
) -> Result<()> {
    match (&state.current, current) {
        (Some(previous), Some(current)) if previous != &current => {
            anyhow::bail!(
                "backend service restarted (instance changed from {previous} to {current}); exiting watcher for clean restart"
            );
        }
        (None, Some(current)) => {
            state.current = Some(current);
        }
        _ => {}
    }
    Ok(())
}

fn log_heartbeat_transition(
    previous: HeartbeatState,
    result: Result<mem_api::WatcherPresenceSummary>,
) -> HeartbeatState {
    match result {
        Ok(summary) => {
            if previous == HeartbeatState::Failing {
                println!(
                    "watcher heartbeat recovered: {} active watcher(s), last heartbeat {}",
                    summary.active_count,
                    summary
                        .last_heartbeat_at
                        .map(|value| value.to_rfc3339())
                        .unwrap_or_else(|| "n/a".to_string())
                );
            }
            HeartbeatState::Healthy
        }
        Err(error) => {
            if previous != HeartbeatState::Failing {
                eprintln!("watcher heartbeat failed: {error}");
            }
            HeartbeatState::Failing
        }
    }
}

async fn status(config: AppConfig, args: ProjectArgs) -> Result<()> {
    let repo_root = resolve_repo_root(&config, args.repo_root)?;
    let project = resolve_project(args.project, &repo_root)?;
    let state = load_state(&project, &repo_root, &config.automation).await?;
    println!("{}", serde_json::to_string_pretty(&to_status(&state))?);
    Ok(())
}

async fn flush(
    config: AppConfig,
    args: FlushArgs,
    writer_id: String,
    writer_name: Option<String>,
) -> Result<()> {
    let repo_root = resolve_repo_root(&config, args.repo_root)?;
    let project = resolve_project(args.project, &repo_root)?;
    let client = Client::new();
    tokio::fs::write(flush_path(&repo_root), b"flush\n")
        .await
        .ok();
    run_once(
        &config,
        &client,
        &project,
        &repo_root,
        true,
        args.curate,
        &writer_id,
        writer_name.as_deref(),
    )
    .await
}

fn resolve_repo_root(config: &AppConfig, repo_root: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(repo_root) = repo_root {
        return Ok(repo_root);
    }
    if let Some(repo_root) = &config.automation.repo_root {
        return Ok(PathBuf::from(repo_root));
    }
    std::env::current_dir().context("read current directory")
}

fn resolve_project(project: Option<String>, repo_root: &std::path::Path) -> Result<String> {
    if let Some(project) = project {
        return Ok(project);
    }
    let Some(name) = repo_root.file_name().and_then(|value| value.to_str()) else {
        anyhow::bail!("could not determine project slug from repo root");
    };
    Ok(name.to_string())
}

fn resolve_writer_id(config: &AppConfig, cli_writer_id: Option<String>) -> Result<String> {
    if let Some(writer_id) = cli_writer_id {
        let trimmed = writer_id.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    if let Ok(writer_id) = std::env::var("MEMORY_LAYER_WRITER_ID") {
        let trimmed = writer_id.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    if let Ok(writer_id) = std::env::var("MEMORY_LAYER_AGENT_ID") {
        let trimmed = writer_id.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    let trimmed = config.writer.id.trim();
    if !trimmed.is_empty() {
        return Ok(trimmed.to_string());
    }
    anyhow::bail!(
        "missing writer id; set --writer-id, MEMORY_LAYER_WRITER_ID, MEMORY_LAYER_AGENT_ID, or [writer].id in config"
    );
}

fn watcher_is_service_managed() -> bool {
    std::env::var("MEMORY_LAYER_WATCH_SERVICE_MANAGED")
        .ok()
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut terminate = signal(SignalKind::terminate()).expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BackendInstanceState, HeartbeatState, log_heartbeat_transition,
        update_backend_instance_state,
    };
    use mem_api::WatcherPresenceSummary;

    #[test]
    fn heartbeat_failure_enters_failing_state() {
        let next = log_heartbeat_transition(
            HeartbeatState::Unknown,
            Err(anyhow::anyhow!("connect failed")),
        );
        assert_eq!(next, HeartbeatState::Failing);
    }

    #[test]
    fn heartbeat_success_after_failure_recovers() {
        let summary = WatcherPresenceSummary {
            active_count: 1,
            unhealthy_count: 0,
            stale_after_seconds: 90,
            last_heartbeat_at: None,
            watchers: Vec::new(),
        };
        let next = log_heartbeat_transition(HeartbeatState::Failing, Ok(summary));
        assert_eq!(next, HeartbeatState::Healthy);
    }

    #[test]
    fn backend_instance_change_requests_watcher_restart() {
        let mut state = BackendInstanceState {
            current: Some("old-instance".to_string()),
        };
        let error = update_backend_instance_state(&mut state, Some("new-instance".to_string()))
            .expect_err("instance change should force restart");
        assert!(error
            .to_string()
            .contains("backend service restarted"));
    }

    #[test]
    fn backend_instance_is_seeded_when_first_seen() {
        let mut state = BackendInstanceState::default();
        update_backend_instance_state(&mut state, Some("instance-a".to_string())).unwrap();
        assert_eq!(state.current.as_deref(), Some("instance-a"));
    }
}
