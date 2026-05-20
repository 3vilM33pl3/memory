use anyhow::{Context, Result};
use mem_api::{AppConfig, QueryFilters, QueryRequest, QueryResponse};
use reqwest::Client;

use crate::commands::{
    api::{get_json, print_query_response},
    output::{parse_memory_type, service_url},
    runtime::QueryArgs,
};

pub(super) async fn handle(args: QueryArgs, client: Client, config: AppConfig) -> Result<()> {
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
        include_stale: args.include_stale,
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
