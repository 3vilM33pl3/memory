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

pub(crate) async fn handle(args: CurateArgs, client: Client, config: AppConfig) -> Result<()> {
    let cwd = env::current_dir().context("read current directory")?;
    let repo_root = resolve_repo_root(&cwd)?;
    let replacement_policy = repo_replacement_policy(&repo_root);
    let response = client
        .post(service_url(&config, "/v1/curate"))
        .headers(write_headers(&config)?)
        .json(&CurateRequest {
            project: args.project,
            batch_size: args.batch_size,
            raw_capture_id: None,
            replacement_policy: Some(replacement_policy),
            dry_run: args.dry_run,
        })
        .send()
        .await?;
    print_json_response(response).await?;

    Ok(())
}
