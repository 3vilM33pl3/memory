// SPDX-License-Identifier: AGPL-3.0-or-later

use anyhow::{Context, Result};
use mem_config::AppConfig;
use reqwest::Client;
use std::env;

use crate::{
    commands::{
        api::ApiClient,
        memory_ops::resolve_project_slug,
        output::{print_graph_extract_report, print_graph_status},
        runtime::{GraphArgs, GraphCommand},
        skill_support::resolve_repo_root,
    },
    scan as scan_runtime,
};

pub(super) async fn handle(args: GraphArgs, client: Client, config: AppConfig) -> Result<()> {
    let cwd = env::current_dir().context("read current directory")?;
    let repo_root = resolve_repo_root(&cwd)?;
    match args.command {
        GraphCommand::Extract(args) => {
            let project = resolve_project_slug(args.project, &cwd)?;
            let index = scan_runtime::load_graph_index(
                &repo_root,
                &project,
                args.since.as_deref(),
                &config,
                args.rebuild_index,
                args.dry_run,
            )?;
            let request = mem_graph::GraphExtractionRequest {
                project: index.project,
                repo_root: index.repo_root,
                git_head: index.head,
                since: index.since,
                force: args.force,
                dry_run: args.dry_run,
                index_reused: index.index_reused,
                analysis: index.analysis,
            };
            let report = if args.dry_run {
                mem_graph::build_extraction_preview(&request)
            } else {
                // The service owns the database; it also records the
                // extraction activity event.
                let api = ApiClient::new(client.clone(), config.clone());
                api.graph_extract(&request.project.clone(), &request)
                    .await?
            };
            if args.text {
                print_graph_extract_report(&report, &index.index_path);
            } else {
                println!("{}", serde_json::to_string_pretty(&report)?);
            }
        }
        GraphCommand::Status(args) => {
            let project = resolve_project_slug(args.project, &cwd)?;
            let api = ApiClient::new(client.clone(), config.clone());
            let status = api.graph_status(&project).await?;
            if args.text {
                print_graph_status(&status);
            } else {
                println!("{}", serde_json::to_string_pretty(&status)?);
            }
        }
    }

    Ok(())
}
