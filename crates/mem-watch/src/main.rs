use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use mem_api::AppConfig;
use mem_watch::{flush_path, load_state, run_once, to_status};
use reqwest::Client;

#[derive(Debug, Parser)]
#[command(name = "memory-watch")]
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
    Flush(ProjectArgs),
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

    loop {
        run_once(&config, &client, &project, &repo_root, false).await?;
        tokio::time::sleep(config.automation.poll_interval).await;
    }
}

async fn status(config: AppConfig, args: ProjectArgs) -> Result<()> {
    let repo_root = resolve_repo_root(&config, args.repo_root)?;
    let project = resolve_project(args.project, &repo_root)?;
    let state = load_state(&project, &repo_root, &config.automation).await?;
    println!("{}", serde_json::to_string_pretty(&to_status(&state))?);
    Ok(())
}

async fn flush(config: AppConfig, args: ProjectArgs) -> Result<()> {
    let repo_root = resolve_repo_root(&config, args.repo_root)?;
    let project = resolve_project(args.project, &repo_root)?;
    let client = Client::new();
    tokio::fs::write(flush_path(&repo_root), b"flush\n")
        .await
        .ok();
    run_once(&config, &client, &project, &repo_root, true).await
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
