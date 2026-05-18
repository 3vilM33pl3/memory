use anyhow::{Context, Result};
use mem_api::AppConfig;
use reqwest::Client;
use std::env;

use crate::commands::{
    api::ApiClient, memory_ops::resolve_project_slug, output::print_activities_response,
    runtime::ActivitiesArgs,
};

pub(super) async fn handle(args: ActivitiesArgs, client: Client, config: AppConfig) -> Result<()> {
    let cwd = env::current_dir().context("read current directory")?;
    let project = resolve_project_slug(args.project, &cwd)?;
    let api = ApiClient::new(client, config);
    let payload = api
        .project_activities(&project, args.limit.clamp(1, 500), args.kind.as_deref())
        .await?;
    if args.text {
        print_activities_response(&payload);
    } else {
        println!("{}", serde_json::to_string_pretty(&payload)?);
    }

    Ok(())
}
