use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use mem_api::{
    AppConfig, ArchiveRequest, CaptureTaskRequest, CurateRequest, QueryFilters, QueryRequest,
    ReindexRequest,
};
use reqwest::{Client, header::HeaderMap};

#[derive(Debug, Parser)]
#[command(name = "memctl")]
struct Cli {
    #[arg(long, env = "MEMORY_LAYER_CONFIG")]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Query(QueryArgs),
    CaptureTask(CaptureTaskArgs),
    Curate(CurateArgs),
    Reindex(ProjectArgs),
    Health,
    Stats,
    Archive(ArchiveArgs),
}

#[derive(Debug, Args)]
struct QueryArgs {
    #[arg(long)]
    project: String,
    #[arg(long)]
    question: String,
    #[arg(long = "type")]
    types: Vec<String>,
    #[arg(long = "tag")]
    tags: Vec<String>,
    #[arg(long, default_value_t = 8)]
    limit: i64,
    #[arg(long)]
    min_confidence: Option<f32>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct CaptureTaskArgs {
    #[arg(long)]
    file: PathBuf,
}

#[derive(Debug, Args)]
struct CurateArgs {
    #[arg(long)]
    project: String,
    #[arg(long)]
    batch_size: Option<i64>,
}

#[derive(Debug, Args)]
struct ProjectArgs {
    #[arg(long)]
    project: String,
}

#[derive(Debug, Args)]
struct ArchiveArgs {
    #[arg(long)]
    project: String,
    #[arg(long, default_value_t = 0.3)]
    max_confidence: f32,
    #[arg(long, default_value_t = 1)]
    max_importance: i32,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = AppConfig::load_from_path(cli.config).context("load config")?;
    let client = Client::builder()
        .timeout(config.service.request_timeout)
        .build()
        .context("build http client")?;

    match cli.command {
        Command::Query(args) => {
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
            };
            let response = client
                .post(format!("http://{}/v1/query", config.service.bind_addr))
                .json(&request)
                .send()
                .await
                .context("query request failed")?;
            let status = response.status();
            let body = response.text().await?;
            if !status.is_success() {
                anyhow::bail!("query failed: {status} {body}");
            }
            if args.json {
                println!("{body}");
            } else {
                let payload: mem_api::QueryResponse = serde_json::from_str(&body)?;
                println!("Answer:\n{}\n", payload.answer);
                println!("Confidence: {:.2}\n", payload.confidence);
                for result in payload.results {
                    println!(
                        "- {} [{}] score={:.2}",
                        result.summary, result.memory_type, result.score
                    );
                    println!("  {}", result.snippet);
                    for source in result.sources {
                        let path = source.file_path.unwrap_or_else(|| "<no-file>".to_string());
                        println!(
                            "  source: {} {}",
                            path,
                            source.source_kind.source_kind_string()
                        );
                    }
                }
            }
        }
        Command::CaptureTask(args) => {
            let request: CaptureTaskRequest =
                serde_json::from_str(&fs::read_to_string(args.file).context("read payload file")?)?;
            let response = client
                .post(format!(
                    "http://{}/v1/capture/task",
                    config.service.bind_addr
                ))
                .headers(write_headers(&config.service.api_token)?)
                .json(&request)
                .send()
                .await?;
            print_json_response(response).await?;
        }
        Command::Curate(args) => {
            let response = client
                .post(format!("http://{}/v1/curate", config.service.bind_addr))
                .headers(write_headers(&config.service.api_token)?)
                .json(&CurateRequest {
                    project: args.project,
                    batch_size: args.batch_size,
                })
                .send()
                .await?;
            print_json_response(response).await?;
        }
        Command::Reindex(args) => {
            let response = client
                .post(format!("http://{}/v1/reindex", config.service.bind_addr))
                .headers(write_headers(&config.service.api_token)?)
                .json(&ReindexRequest {
                    project: args.project,
                })
                .send()
                .await?;
            print_json_response(response).await?;
        }
        Command::Health => {
            let response = client
                .get(format!("http://{}/healthz", config.service.bind_addr))
                .send()
                .await?;
            print_json_response(response).await?;
        }
        Command::Stats => {
            let response = client
                .get(format!("http://{}/v1/stats", config.service.bind_addr))
                .send()
                .await?;
            print_json_response(response).await?;
        }
        Command::Archive(args) => {
            let response = client
                .post(format!("http://{}/v1/archive", config.service.bind_addr))
                .headers(write_headers(&config.service.api_token)?)
                .json(&ArchiveRequest {
                    project: args.project,
                    max_confidence: args.max_confidence,
                    max_importance: args.max_importance,
                })
                .send()
                .await?;
            print_json_response(response).await?;
        }
    }

    Ok(())
}

async fn print_json_response(response: reqwest::Response) -> Result<()> {
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        anyhow::bail!("{status} {body}");
    }
    println!("{body}");
    Ok(())
}

fn parse_memory_type(input: String) -> Result<mem_api::MemoryType> {
    match input.as_str() {
        "architecture" => Ok(mem_api::MemoryType::Architecture),
        "convention" => Ok(mem_api::MemoryType::Convention),
        "decision" => Ok(mem_api::MemoryType::Decision),
        "incident" => Ok(mem_api::MemoryType::Incident),
        "debugging" => Ok(mem_api::MemoryType::Debugging),
        "environment" => Ok(mem_api::MemoryType::Environment),
        "domain_fact" => Ok(mem_api::MemoryType::DomainFact),
        _ => anyhow::bail!("unknown memory type: {input}"),
    }
}

fn write_headers(token: &str) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert("x-api-token", token.parse()?);
    Ok(headers)
}

trait SourceKindString {
    fn source_kind_string(&self) -> &'static str;
}

impl SourceKindString for mem_api::SourceKind {
    fn source_kind_string(&self) -> &'static str {
        match self {
            mem_api::SourceKind::TaskPrompt => "task_prompt",
            mem_api::SourceKind::File => "file",
            mem_api::SourceKind::GitCommit => "git_commit",
            mem_api::SourceKind::CommandOutput => "command_output",
            mem_api::SourceKind::Test => "test",
            mem_api::SourceKind::Note => "note",
        }
    }
}
