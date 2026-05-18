use anyhow::{Context, Result};
use mem_api::{AppConfig, CaptureTaskRequest};
use reqwest::Client;
use std::fs;

use crate::{
    commands::{
        api::print_json_response,
        output::{service_url, write_headers},
        runtime::{CaptureArgs, CaptureCommand},
    },
    writer_identity::resolve_writer_identity,
};

pub(super) async fn handle(
    args: CaptureArgs,
    client: Client,
    config: AppConfig,
    cli_writer_id: Option<String>,
) -> Result<()> {
    match args.command {
        CaptureCommand::Task(args) => {
            let mut request: CaptureTaskRequest =
                serde_json::from_str(&fs::read_to_string(args.file).context("read payload file")?)?;
            if request.writer_id.trim().is_empty() {
                let writer = resolve_writer_identity(&config, cli_writer_id.as_deref())?;
                request.writer_id = writer.id;
                request.writer_name = request.writer_name.or(writer.name);
            }
            request.dry_run = args.dry_run;
            let response = client
                .post(service_url(&config, "/v1/capture/task"))
                .headers(write_headers(&config)?)
                .json(&request)
                .send()
                .await?;
            print_json_response(response).await?;
        }
    };
    Ok(())
}
