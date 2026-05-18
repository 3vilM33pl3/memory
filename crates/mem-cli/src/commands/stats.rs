use anyhow::Result;
use mem_api::AppConfig;
use reqwest::Client;

use crate::commands::runtime::*;

pub(crate) async fn handle(client: Client, config: AppConfig) -> Result<()> {
    let response = client.get(service_url(&config, "/v1/stats")).send().await?;
    print_json_response(response).await
}
