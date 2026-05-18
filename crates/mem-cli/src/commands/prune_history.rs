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
    args: PruneHistoryArgs,
    client: Client,
    config: AppConfig,
) -> Result<()> {
    let request = mem_api::PruneHistoryRequest {
        project: args.project,
        tombstone_after: args.tombstone_after,
        superseded_after: args.superseded_after,
        dry_run: args.dry_run,
    };
    let payload: mem_api::PruneHistoryResponse = get_json(
        client
            .post(service_url(&config, "/v1/prune-history"))
            .headers(write_headers(&config)?)
            .json(&request)
            .send()
            .await
            .context("prune-history request failed")?,
    )
    .await?;
    if args.json {
        println!("{}", serde_json::to_string(&payload)?);
    } else {
        let verb = if payload.dry_run {
            "Would prune"
        } else {
            "Pruned"
        };
        let scope = payload
            .project
            .as_deref()
            .map(|p| format!(" for project \"{p}\""))
            .unwrap_or_default();
        println!(
            "{verb} {} canonical tombstone(s) and {} superseded version(s){scope}.",
            payload.canonicals_tombstoned_deleted, payload.superseded_versions_pruned
        );
    }

    Ok(())
}
