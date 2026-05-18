use anyhow::Result;
use mem_api::{AppConfig, ArchiveRequest};
use reqwest::Client;

use crate::commands::{
    api::print_json_response,
    output::{service_url, write_headers},
    runtime::ArchiveArgs,
};

pub(super) async fn handle(args: ArchiveArgs, client: Client, config: AppConfig) -> Result<()> {
    let response = client
        .post(service_url(&config, "/v1/archive"))
        .headers(write_headers(&config)?)
        .json(&ArchiveRequest {
            project: args.project,
            max_confidence: args.max_confidence,
            max_importance: args.max_importance,
            dry_run: args.dry_run,
        })
        .send()
        .await?;
    print_json_response(response).await?;

    Ok(())
}
