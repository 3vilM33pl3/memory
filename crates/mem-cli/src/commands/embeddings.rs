use anyhow::Result;
use mem_api::{AppConfig, ReembedRequest, ReindexRequest};
use reqwest::Client;

use crate::commands::{
    api::{ApiClient, print_embedding_backends, print_json_response},
    output::{service_url, write_headers},
    runtime::{EmbeddingsArgs, EmbeddingsCommand},
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
