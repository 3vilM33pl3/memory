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
    args: WatcherArgs,
    config: AppConfig,
    cli_config_path: Option<PathBuf>,
    cli_writer_id: Option<String>,
) -> Result<()> {
    match args.command {
        WatcherCommand::Run(args) => {
            let writer = resolve_writer_identity_for_tool(
                &config,
                cli_writer_id.as_deref(),
                "memory-watcher",
            )?;
            run_watcher_daemon(
                config,
                WatcherRunArgs {
                    project: args.project,
                    repo_root: args.repo_root,
                    agent_cli: args.agent_cli,
                    agent_session_id: args.agent_session_id,
                    agent_pid: args.agent_pid,
                    agent_started_at: args.agent_started_at,
                },
                writer.id,
                writer.name,
            )
            .await?;
        }
        WatcherCommand::Manager(args) => match args.command {
            WatcherManagerCommand::Run => run_watcher_manager(config, cli_config_path).await?,
            WatcherManagerCommand::Enable(_)
            | WatcherManagerCommand::Disable(_)
            | WatcherManagerCommand::Status => {
                unreachable!("watcher manager lifecycle commands are handled before config loading")
            }
        },
        WatcherCommand::Enable(_) | WatcherCommand::Disable(_) | WatcherCommand::Status(_) => {
            unreachable!("watcher lifecycle commands are handled before config loading")
        }
    };
    Ok(())
}
