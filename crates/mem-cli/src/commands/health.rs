use anyhow::Result;
use mem_api::AppConfig;
use reqwest::Client;

use crate::commands::{api::print_json_response, output::service_url};

pub(crate) async fn handle(client: Client, config: AppConfig) -> Result<()> {
    let response = client.get(service_url(&config, "/healthz")).send().await?;
    print_json_response(response).await
}
