use anyhow::{Context, Result};
use mem_api::AppConfig;
use mem_watch::{flush_path, load_state, run_once, to_status};
use reqwest::Client;
use std::{env, path::PathBuf};

use crate::{
    commands::{
        api::ApiClient,
        memory_ops::{preview_automation_flush, resolve_project_slug},
        runtime::{AutomationArgs, AutomationCommand},
    },
    writer_identity::resolve_writer_identity,
};

pub(super) async fn handle(
    args: AutomationArgs,
    client: Client,
    config: AppConfig,
    cli_writer_id: Option<String>,
) -> Result<()> {
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
            let writer = resolve_writer_identity(&config, cli_writer_id.as_deref())?;
            if args.dry_run {
                let preview = preview_automation_flush(
                    &api.config,
                    &api.client,
                    &project,
                    &repo_root,
                    args.curate,
                    &writer.id,
                    writer.name.as_deref(),
                )
                .await?;
                println!("{}", serde_json::to_string_pretty(&preview)?);
                return Ok(());
            }
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
                &writer.id,
                writer.name.as_deref(),
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

    Ok(())
}
