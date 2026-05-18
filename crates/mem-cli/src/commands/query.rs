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

pub(crate) async fn handle(args: QueryArgs, client: Client, config: AppConfig) -> Result<()> {
    let request = QueryRequest {
        project: args.project,
        query: args.question,
        filters: QueryFilters {
            types: args
                .types
                .into_iter()
                .map(parse_memory_type)
                .collect::<Result<Vec<_>>>()?,
            tags: args.tags,
        },
        top_k: args.limit,
        min_confidence: args.min_confidence,
        history: args.history,
        retrieval_mode: None,
        answer_mode: None,
    };
    let payload: QueryResponse = get_json(
        client
            .post(service_url(&config, "/v1/query"))
            .json(&request)
            .send()
            .await
            .context("query request failed")?,
    )
    .await?;
    if args.json {
        println!("{}", serde_json::to_string(&payload)?);
    } else {
        print_query_response(payload);
    }

    Ok(())
}
