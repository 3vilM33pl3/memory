#![allow(unused_imports)]

use anyhow::{Context, Result};
use clap::CommandFactory;
use clap_complete::generate;
use mem_api::*;
use mem_service as service_runtime;
use mem_watch::{WatcherRunArgs, flush_path, load_state, run_once, run_watcher_daemon, to_status};
use reqwest::Client;
use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use crate::commands::runtime::*;
use crate::writer_identity::{resolve_writer_identity, resolve_writer_identity_for_tool};
use crate::{
    commits as git_commits, resume as checkpoint_store, scan as scan_runtime, tui as tui_runtime,
    wizard as wizard_runtime,
};

pub(crate) async fn handle(args: ResumeArgs, client: Client, config: AppConfig) -> Result<()> {
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
