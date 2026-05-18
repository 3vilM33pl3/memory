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

pub(crate) async fn handle(args: CommitsArgs, client: Client, config: AppConfig) -> Result<()> {
    let cwd = env::current_dir().context("read current directory")?;
    let repo_root = resolve_repo_root(&cwd)?;
    let api = ApiClient::new(client, config);
    match args.command {
        CommitsCommand::Sync(args) => {
            let project = resolve_project_slug(args.project, &cwd)?;
            let commits =
                git_commits::collect_git_commits(&repo_root, args.since.as_deref(), args.limit)?;
            let response = api
                .sync_commits(&CommitSyncRequest {
                    project,
                    repo_root: repo_root.display().to_string(),
                    commits,
                    dry_run: args.dry_run,
                })
                .await?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&response)?);
            } else {
                print_commit_sync_response(&response);
            }
        }
        CommitsCommand::List(args) => {
            let project = resolve_project_slug(args.project, &cwd)?;
            let response = api
                .project_commits(&project, args.limit.clamp(1, 500), args.offset.max(0))
                .await?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&response)?);
            } else {
                print_project_commits(&response);
            }
        }
        CommitsCommand::Show(args) => {
            let project = resolve_project_slug(args.project, &cwd)?;
            let response = api.project_commit(&project, &args.commit).await?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&response)?);
            } else {
                print_commit_detail(&response);
            }
        }
    }

    Ok(())
}
