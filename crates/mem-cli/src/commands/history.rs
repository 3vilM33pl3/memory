use anyhow::{Context, Result};
use mem_api::AppConfig;
use reqwest::Client;

use crate::commands::{
    api::{get_json, print_memory_history},
    output::service_url,
    runtime::HistoryArgs,
};

pub(crate) async fn handle(args: HistoryArgs, client: Client, config: AppConfig) -> Result<()> {
    let payload: mem_api::MemoryHistoryResponse = get_json(
        client
            .get(service_url(
                &config,
                &format!("/v1/memory/{}/history", args.memory_id),
            ))
            .send()
            .await
            .context("history request failed")?,
    )
    .await?;
    if args.json {
        println!("{}", serde_json::to_string(&payload)?);
    } else {
        print_memory_history(&payload);
    }

    Ok(())
}
