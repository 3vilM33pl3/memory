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
    args: VerifyProvenanceArgs,
    client: Client,
    config: AppConfig,
) -> Result<()> {
    let cwd = env::current_dir().context("read current directory")?;
    let api = ApiClient::new(client.clone(), config.clone());
    let project = resolve_project_slug(args.project, &cwd)?;
    let repo_root = args
        .repo_root
        .as_ref()
        .map(|path| path.to_string_lossy().to_string());
    let response = api
        .verify_provenance(&ProvenanceVerificationRequest {
            project,
            repo_root,
            dry_run: args.dry_run,
        })
        .await?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&response)?);
    } else {
        print_provenance_verification_response(&response);
    }

    Ok(())
}
