use anyhow::{Context, Result};
use mem_api::{AppConfig, CommitSyncRequest};
use reqwest::Client;
use std::env;

use crate::{
    commands::{
        api::ApiClient,
        memory_ops::resolve_project_slug,
        output::{print_commit_detail, print_commit_sync_response, print_project_commits},
        runtime::{CommitsArgs, CommitsCommand},
        skill_support::resolve_repo_root,
    },
    commits as git_commits,
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
