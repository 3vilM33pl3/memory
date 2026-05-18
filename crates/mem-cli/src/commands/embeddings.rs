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

pub(crate) async fn handle(args: EmbeddingsArgs, client: Client, config: AppConfig) -> Result<()> {
    match args.command {
        EmbeddingsCommand::List => {
            let api = ApiClient::new(client.clone(), config.clone());
            let payload = api.list_embedding_backends(None).await?;
            print_embedding_backends(&payload);
        }
        EmbeddingsCommand::Activate(args) => {
            let api = ApiClient::new(client.clone(), config.clone());
            let payload = api.activate_embedding_backend(&args.name).await?;
            print_embedding_backends(&payload);
        }
        EmbeddingsCommand::Reindex(args) => {
            let response = client
                .post(service_url(&config, "/v1/reindex"))
                .headers(write_headers(&config)?)
                .json(&ReindexRequest {
                    project: args.project,
                    dry_run: args.dry_run,
                    backend: args.backend,
                })
                .send()
                .await?;
            print_json_response(response).await?;
        }
        EmbeddingsCommand::Reembed(args) => {
            let response = client
                .post(service_url(&config, "/v1/reembed"))
                .headers(write_headers(&config)?)
                .json(&ReembedRequest {
                    project: args.project,
                    dry_run: args.dry_run,
                    backend: args.backend,
                })
                .send()
                .await?;
            print_json_response(response).await?;
        }
        EmbeddingsCommand::Prune(args) => {
            let api = ApiClient::new(client, config);
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &api.prune_embeddings(&args.project, args.dry_run).await?
                )?
            );
        }
    };
    Ok(())
}
