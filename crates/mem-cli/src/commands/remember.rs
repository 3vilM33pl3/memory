#![allow(unused_imports)]

use anyhow::{Context, Result};
use clap::CommandFactory;
use clap_complete::generate;
use mem_api::*;
use mem_service as service_runtime;
use mem_watch::{WatcherRunArgs, flush_path, load_state, run_once, run_watcher_daemon, to_status};
use reqwest::Client;
use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use crate::commands::runtime::*;
use crate::writer_identity::{resolve_writer_identity, resolve_writer_identity_for_tool};
use crate::{
    commits as git_commits, resume as checkpoint_store, scan as scan_runtime, tui as tui_runtime,
    wizard as wizard_runtime,
};

pub(crate) async fn handle(
    args: RememberArgs,
    client: Client,
    config: AppConfig,
    cli_writer_id: Option<String>,
) -> Result<()> {
    let cwd = env::current_dir().context("read current directory")?;
    let repo_root = resolve_repo_root(&cwd)?;
    let project = resolve_project_slug(args.project.clone(), &cwd)?;
    let writer = resolve_writer_identity(&config, cli_writer_id.as_deref())?;
    let dry_run = args.dry_run;
    let mut request = build_remember_request(args, &project, &writer.id, writer.name.as_deref())?;
    request.dry_run = dry_run;
    let api = ApiClient::new(client, config);
    let capture = api.capture_task(&request).await?;
    let curate = if dry_run {
        api.curate(&project, repo_replacement_policy(&repo_root), true)
            .await?
    } else {
        api.curate_capture(
            &project,
            capture.raw_capture_id,
            repo_replacement_policy(&repo_root),
            false,
        )
        .await?
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "capture": capture,
            "curate": curate,
            "dry_run": dry_run,
        }))?
    );

    Ok(())
}
