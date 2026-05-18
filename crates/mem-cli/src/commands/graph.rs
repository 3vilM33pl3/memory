use anyhow::{Context, Result};
use mem_api::AppConfig;
use reqwest::Client;
use std::env;

use crate::{
    commands::{
        api::ApiClient,
        memory_ops::resolve_project_slug,
        output::{
            build_graph_activity_request, connect_graph_database, print_graph_extract_report,
            print_graph_status,
        },
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
                let pool = connect_graph_database(&config).await?;
                mem_graph::run_migrations(&pool).await?;
                mem_graph::PostgresGraphRepository::new(pool)
                    .extract(request)
                    .await?
            };
            if !report.dry_run {
                let api = ApiClient::new(client.clone(), config.clone());
                let activity_request = build_graph_activity_request(&report);
                if let Err(error) = api.log_graph_activity(&activity_request).await {
                    eprintln!(
                        "warning: failed to log graph extraction activity for `{}`: {error}",
                        report.project
                    );
                }
            }
            if args.text {
                print_graph_extract_report(&report, &index.index_path);
            } else {
                println!("{}", serde_json::to_string_pretty(&report)?);
            }
        }
        GraphCommand::Status(args) => {
            let project = resolve_project_slug(args.project, &cwd)?;
            let pool = connect_graph_database(&config).await?;
            mem_graph::run_migrations(&pool).await?;
            let status = mem_graph::PostgresGraphRepository::new(pool)
                .latest_status(&project)
                .await?;
            if args.text {
                print_graph_status(&status, &project);
            } else {
                println!("{}", serde_json::to_string_pretty(&status)?);
            }
        }
    }

    Ok(())
}
