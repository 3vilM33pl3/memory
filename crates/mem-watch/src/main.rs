use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use mem_api::AppConfig;
use mem_watch::{
    build_watcher_heartbeat_request, build_watcher_unregister_request, detect_hostname, flush_path,
    heartbeat_watcher, load_state, run_once, to_status, unregister_watcher,
};
use reqwest::Client;
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(name = "memory-watch", version)]
struct Cli {
    #[arg(long, env = "MEMORY_LAYER_CONFIG")]
    config: Option<PathBuf>,
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

    match cli.command {
        WatchCommand::Run(args) => run_loop(config, args).await,
        WatchCommand::Status(args) => status(config, args).await,
        WatchCommand::Flush(args) => flush(config, args).await,
    }
}

async fn run_loop(config: AppConfig, args: RunArgs) -> Result<()> {
    let repo_root = resolve_repo_root(&config, args.repo_root)?;
    let project = resolve_project(args.project, &repo_root)?;
    let client = Client::new();
    let state = load_state(&project, &repo_root, &config.automation).await?;
    let watcher_id = Uuid::new_v4().to_string();
    let hostname = detect_hostname();
    let pid = std::process::id();
    let started_at = chrono::Utc::now();

    let heartbeat_request =
        build_watcher_heartbeat_request(&state, &watcher_id, &hostname, pid, started_at);
    if let Err(error) = heartbeat_watcher(&client, &config, &heartbeat_request).await {
        eprintln!("watcher heartbeat failed: {error}");
    }

    let mut poll = tokio::time::interval(config.automation.poll_interval);
    let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(30));

    loop {
        tokio::select! {
            _ = poll.tick() => {
                run_once(&config, &client, &project, &repo_root, false, false).await?;
            }
            _ = heartbeat.tick() => {
                let state = load_state(&project, &repo_root, &config.automation).await?;
                let request = build_watcher_heartbeat_request(
                    &state,
                    &watcher_id,
                    &hostname,
                    pid,
                    started_at,
                );
                if let Err(error) = heartbeat_watcher(&client, &config, &request).await {
                    eprintln!("watcher heartbeat failed: {error}");
                }
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

async fn status(config: AppConfig, args: ProjectArgs) -> Result<()> {
    let repo_root = resolve_repo_root(&config, args.repo_root)?;
    let project = resolve_project(args.project, &repo_root)?;
    let state = load_state(&project, &repo_root, &config.automation).await?;
    println!("{}", serde_json::to_string_pretty(&to_status(&state))?);
    Ok(())
}

async fn flush(config: AppConfig, args: FlushArgs) -> Result<()> {
    let repo_root = resolve_repo_root(&config, args.repo_root)?;
    let project = resolve_project(args.project, &repo_root)?;
    let client = Client::new();
    tokio::fs::write(flush_path(&repo_root), b"flush\n")
        .await
        .ok();
    run_once(&config, &client, &project, &repo_root, true, args.curate).await
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
