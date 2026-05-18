use anyhow::{Context, Result};
use mem_api::AppConfig;
use reqwest::Client;
use std::env;

use crate::commands::{api::ApiClient, eval_support::handle_eval_command, runtime::EvalArgs};

pub(crate) async fn handle(args: EvalArgs, client: Client, config: AppConfig) -> Result<()> {
    let cwd = env::current_dir().context("read current directory")?;
    let api = ApiClient::new(client, config);
    handle_eval_command(args, &cwd, &api).await?;

    Ok(())
}
