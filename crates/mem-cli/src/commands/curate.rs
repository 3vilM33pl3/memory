use anyhow::{Context, Result};
use mem_api::{AppConfig, CurateRequest};
use reqwest::Client;
use std::env;

use crate::commands::{
    api::print_json_response,
    init_support::repo_replacement_policy,
    output::{service_url, write_headers},
    runtime::CurateArgs,
    skill_support::resolve_repo_root,
};

pub(crate) async fn handle(args: CurateArgs, client: Client, config: AppConfig) -> Result<()> {
    let cwd = env::current_dir().context("read current directory")?;
    let repo_root = resolve_repo_root(&cwd)?;
    let replacement_policy = repo_replacement_policy(&repo_root);
    let response = client
        .post(service_url(&config, "/v1/curate"))
        .headers(write_headers(&config)?)
        .json(&CurateRequest {
            project: args.project,
            batch_size: args.batch_size,
            raw_capture_id: None,
            replacement_policy: Some(replacement_policy),
            dry_run: args.dry_run,
        })
        .send()
        .await?;
    print_json_response(response).await?;

    Ok(())
}
