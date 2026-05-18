use anyhow::{Context, Result};
use mem_api::AppConfig;
use std::env;

use crate::commands::{
    memory_ops::resolve_project_slug,
    runtime::{McpArgs, McpCommand},
};

pub(crate) async fn handle(args: McpArgs, config: AppConfig) -> Result<()> {
    match args.command {
        McpCommand::Run(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            mem_mcp::run_stdio(config, args.project, &cwd).await?;
        }
        McpCommand::Status(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            let project = match args.project {
                Some(project) => Some(project),
                None => resolve_project_slug(None, &cwd).ok(),
            };
            let report = mem_mcp::status_report(config, project).await;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("{}", mem_mcp::format_status_text(&report));
            }
        }
    };
    Ok(())
}
