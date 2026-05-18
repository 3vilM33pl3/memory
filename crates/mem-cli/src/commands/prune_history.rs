use anyhow::{Context, Result};
use mem_api::AppConfig;
use reqwest::Client;

use crate::commands::{
    api::get_json,
    output::{service_url, write_headers},
    runtime::PruneHistoryArgs,
};

pub(crate) async fn handle(
    args: PruneHistoryArgs,
    client: Client,
    config: AppConfig,
) -> Result<()> {
    let request = mem_api::PruneHistoryRequest {
        project: args.project,
        tombstone_after: args.tombstone_after,
        superseded_after: args.superseded_after,
        dry_run: args.dry_run,
    };
    let payload: mem_api::PruneHistoryResponse = get_json(
        client
            .post(service_url(&config, "/v1/prune-history"))
            .headers(write_headers(&config)?)
            .json(&request)
            .send()
            .await
            .context("prune-history request failed")?,
    )
    .await?;
    if args.json {
        println!("{}", serde_json::to_string(&payload)?);
    } else {
        let verb = if payload.dry_run {
            "Would prune"
        } else {
            "Pruned"
        };
        let scope = payload
            .project
            .as_deref()
            .map(|p| format!(" for project \"{p}\""))
            .unwrap_or_default();
        println!(
            "{verb} {} canonical tombstone(s) and {} superseded version(s){scope}.",
            payload.canonicals_tombstoned_deleted, payload.superseded_versions_pruned
        );
    }

    Ok(())
}
