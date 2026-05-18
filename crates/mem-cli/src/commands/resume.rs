use anyhow::{Context, Result};
use mem_api::{AppConfig, ResumeRequest};
use reqwest::Client;
use std::env;

use crate::{
    commands::{
        api::ApiClient, memory_ops::resolve_project_slug, output::print_resume_response,
        runtime::ResumeArgs, skill_support::resolve_repo_root,
    },
    resume as checkpoint_store,
};

pub(super) async fn handle(args: ResumeArgs, client: Client, config: AppConfig) -> Result<()> {
    let cwd = env::current_dir().context("read current directory")?;
    let repo_root = resolve_repo_root(&cwd)?;
    let project = resolve_project_slug(args.project, &cwd)?;
    let checkpoint = checkpoint_store::load_checkpoint(&project, &repo_root)?;
    let api = ApiClient::new(client, config);
    let payload = api
        .resume(&ResumeRequest {
            project: project.clone(),
            checkpoint,
            repo_root: Some(repo_root.display().to_string()),
            since: None,
            include_llm_summary: args.include_llm_summary,
            limit: 12,
        })
        .await?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        print_resume_response(&payload);
    }

    Ok(())
}
