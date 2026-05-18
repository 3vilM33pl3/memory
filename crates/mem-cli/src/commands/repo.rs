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

pub(crate) async fn handle(args: RepoArgs, config: AppConfig) -> Result<()> {
    let cwd = env::current_dir().context("read current directory")?;
    let repo_root = resolve_repo_root(&cwd)?;
    match args.command {
        RepoCommand::Index(args) => {
            let project = resolve_project_slug(args.project, &cwd)?;
            let report = scan_runtime::run_index(
                &repo_root,
                &project,
                args.since.as_deref(),
                &config,
                args.dry_run,
            )?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_index_report(&report);
            }
        }
        RepoCommand::Status(args) => {
            let project = resolve_project_slug(args.project, &cwd)?;
            let status = scan_runtime::read_index_status(&repo_root, &project)?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&status)?);
            } else {
                print_index_status(&status, &project);
            }
        }
    }

    Ok(())
}
