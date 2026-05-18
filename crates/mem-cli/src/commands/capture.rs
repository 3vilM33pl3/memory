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
    args: CaptureArgs,
    client: Client,
    config: AppConfig,
    cli_writer_id: Option<String>,
) -> Result<()> {
    match args.command {
        CaptureCommand::Task(args) => {
            let mut request: CaptureTaskRequest =
                serde_json::from_str(&fs::read_to_string(args.file).context("read payload file")?)?;
            if request.writer_id.trim().is_empty() {
                let writer = resolve_writer_identity(&config, cli_writer_id.as_deref())?;
                request.writer_id = writer.id;
                request.writer_name = request.writer_name.or(writer.name);
            }
            request.dry_run = args.dry_run;
            let response = client
                .post(service_url(&config, "/v1/capture/task"))
                .headers(write_headers(&config)?)
                .json(&request)
                .send()
                .await?;
            print_json_response(response).await?;
        }
    };
    Ok(())
}
