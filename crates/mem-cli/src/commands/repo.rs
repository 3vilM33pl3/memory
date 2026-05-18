use anyhow::{Context, Result};
use mem_api::AppConfig;
use std::env;

use crate::{
    commands::{
        memory_ops::resolve_project_slug,
        output::{print_index_report, print_index_status},
        runtime::{RepoArgs, RepoCommand},
        skill_support::resolve_repo_root,
    },
    scan as scan_runtime,
};

pub(super) async fn handle(args: RepoArgs, config: AppConfig) -> Result<()> {
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
