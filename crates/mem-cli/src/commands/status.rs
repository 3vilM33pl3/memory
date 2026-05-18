use anyhow::{Context, Result};
use mem_api::AppConfig;
use reqwest::Client;
use std::{env, path::PathBuf};

use crate::commands::{
    memory_ops::resolve_project_slug,
    runtime::StatusArgs,
    skill_support::resolve_repo_root,
    status_support::{build_cli_status_report, print_cli_status_report},
};

pub(crate) async fn handle(
    args: StatusArgs,
    cli_config_path: Option<PathBuf>,
    client: Client,
    config: AppConfig,
) -> Result<()> {
    let cwd = env::current_dir().context("read current directory")?;
    let repo_root = resolve_repo_root(&cwd)?;
    let project = resolve_project_slug(args.project, &cwd)?;
    let report = build_cli_status_report(
        cli_config_path,
        &client,
        config.clone(),
        &repo_root,
        project,
    )
    .await?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_cli_status_report(&report);
    }

    Ok(())
}
