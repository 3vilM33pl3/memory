use anyhow::{Context, Result};
use mem_api::AppConfig;
use reqwest::Client;
use std::env;

use crate::{
    commands::{
        api::ApiClient, memory_ops::resolve_project_slug, output::print_scan_report,
        runtime::ScanArgs, skill_support::resolve_repo_root,
    },
    scan as scan_runtime,
    writer_identity::resolve_writer_identity,
};

pub(super) async fn handle(
    args: ScanArgs,
    client: Client,
    config: AppConfig,
    cli_writer_id: Option<String>,
) -> Result<()> {
    let cwd = env::current_dir().context("read current directory")?;
    let repo_root = resolve_repo_root(&cwd)?;
    let project = resolve_project_slug(args.project, &cwd)?;
    let writer = resolve_writer_identity(&config, cli_writer_id.as_deref())?;
    let api = ApiClient::new(client, config);
    let report = scan_runtime::run_scan(
        &api,
        &repo_root,
        &project,
        args.since.as_deref(),
        args.rebuild_index,
        args.dry_run,
        &writer.id,
        writer.name.as_deref(),
    )
    .await?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_scan_report(&report);
    }

    Ok(())
}
