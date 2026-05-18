use anyhow::{Context, Result};
use mem_api::{AppConfig, Profile};
use mem_watch::{WatcherRunArgs, run_watcher_daemon};
use std::{env, path::PathBuf};

use crate::{
    commands::{
        memory_ops::resolve_project_slug,
        runtime::{WatcherArgs, WatcherCommand, WatcherManagerCommand, default_global_config_path},
        skill_support::resolve_repo_root,
        watch_support::{
            disable_watch_manager_service, disable_watch_service, enable_watch_manager_service,
            enable_watch_service, preview_disable_watch_manager_service,
            preview_disable_watch_service, preview_enable_watch_manager_service,
            preview_enable_watch_service, run_watcher_manager, watch_manager_service_status,
            watch_service_status, watcher_command_requires_config_load,
        },
    },
    writer_identity::resolve_writer_identity_for_tool,
};

pub(crate) async fn handle_pre_config(
    args: &WatcherArgs,
    cli_config: Option<PathBuf>,
) -> Result<bool> {
    let cwd = env::current_dir().context("read current directory")?;
    let repo_root = resolve_repo_root(&cwd)?;
    match &args.command {
        WatcherCommand::Run(_) => {}
        WatcherCommand::Manager(args) => match &args.command {
            WatcherManagerCommand::Run => {}
            WatcherManagerCommand::Enable(args) => {
                let output = if args.dry_run {
                    preview_enable_watch_manager_service()?
                } else {
                    enable_watch_manager_service(
                        &cli_config
                            .clone()
                            .unwrap_or_else(default_global_config_path),
                    )?
                };
                println!("{output}");
                return Ok(false);
            }
            WatcherManagerCommand::Disable(args) => {
                let output = if args.dry_run {
                    preview_disable_watch_manager_service()?
                } else {
                    disable_watch_manager_service(Profile::detect())?
                };
                println!("{output}");
                return Ok(false);
            }
            WatcherManagerCommand::Status => {
                println!("{}", watch_manager_service_status(Profile::detect())?);
                return Ok(false);
            }
        },
        WatcherCommand::Enable(args) => {
            let project = resolve_project_slug(args.project.clone(), &cwd)?;
            let output = if args.dry_run {
                preview_enable_watch_service(&repo_root, &project)?
            } else {
                enable_watch_service(&repo_root, &project)?
            };
            println!("{output}");
        }
        WatcherCommand::Disable(args) => {
            let project = resolve_project_slug(args.project.clone(), &cwd)?;
            let output = if args.dry_run {
                preview_disable_watch_service(&project)?
            } else {
                disable_watch_service(&project)?
            };
            println!("{output}");
        }
        WatcherCommand::Status(args) => {
            let project = resolve_project_slug(args.project.clone(), &cwd)?;
            let output = watch_service_status(&repo_root, &project)?;
            println!("{output}");
        }
    }
    Ok(watcher_command_requires_config_load(&args.command))
}

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
