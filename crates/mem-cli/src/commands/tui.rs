use anyhow::{Context, Result};
use mem_api::AppConfig;
use reqwest::Client;
use std::env;

use crate::{
    commands::{
        api::ApiClient, memory_ops::resolve_project_slug, runtime::TuiArgs,
        skill_support::resolve_repo_root,
    },
    tui as tui_runtime,
};

pub(super) async fn handle(args: TuiArgs, client: Client, config: AppConfig) -> Result<()> {
    let cwd = env::current_dir().context("read current directory")?;
    let repo_root = resolve_repo_root(&cwd)?;
    let project = resolve_project_slug(args.project, &cwd)?;
    let api = ApiClient::new(client, config);
    tui_runtime::run(api, project, repo_root).await?;

    Ok(())
}
