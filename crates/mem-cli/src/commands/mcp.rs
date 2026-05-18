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

pub(crate) async fn handle(args: McpArgs, config: AppConfig) -> Result<()> {
    match args.command {
        McpCommand::Run(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            mem_mcp::run_stdio(config, args.project, &cwd).await?;
        }
        McpCommand::Status(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            let project = match args.project {
                Some(project) => Some(project),
                None => resolve_project_slug(None, &cwd).ok(),
            };
            let report = mem_mcp::status_report(config, project).await;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("{}", mem_mcp::format_status_text(&report));
            }
        }
    };
    Ok(())
}
