use anyhow::{Context, Result};
use mem_api::{AppConfig, UpToSpeedRequest};
use reqwest::Client;
use std::env;

use crate::commands::{
    api::ApiClient, memory_ops::resolve_project_slug, output::print_up_to_speed_response,
    runtime::UpToSpeedArgs,
};

pub(super) async fn handle(args: UpToSpeedArgs, client: Client, config: AppConfig) -> Result<()> {
    let cwd = env::current_dir().context("read current directory")?;
    let project = resolve_project_slug(args.project, &cwd)?;
    let api = ApiClient::new(client, config);
    let payload = api
        .up_to_speed(&UpToSpeedRequest {
            project,
            include_llm_summary: args.llm,
            limit: args.limit.clamp(1, 50),
        })
        .await?;
    if args.text {
        print_up_to_speed_response(&payload);
    } else {
        println!("{}", serde_json::to_string_pretty(&payload)?);
    }

    Ok(())
}
