use anyhow::{Context, Result};
use mem_api::{AppConfig, CaptureTaskRequest, CurateRequest};
use reqwest::Client;

use crate::{
    commands::{
        output::{service_url, write_headers},
        runtime::DemoArgs,
    },
    writer_identity::resolve_writer_identity,
};

/// A self-referential showcase corpus (memories about Memory Layer itself),
/// embedded so `memory demo` works from a fresh install with no files on disk.
const DEMO_CORPUS: &str = include_str!("../assets/demo-corpus.json");

pub(super) async fn handle(
    args: DemoArgs,
    client: Client,
    config: AppConfig,
    cli_writer_id: Option<String>,
) -> Result<()> {
    let mut request: CaptureTaskRequest =
        serde_json::from_str(DEMO_CORPUS).context("parse embedded demo corpus")?;
    request.project = args.project.clone();
    if request.writer_id.trim().is_empty() {
        let writer = resolve_writer_identity(&config, cli_writer_id.as_deref())?;
        request.writer_id = writer.id;
        request.writer_name = request.writer_name.or(writer.name);
    }
    let candidate_count = request.structured_candidates.len();

    let capture = client
        .post(service_url(&config, "/v1/capture/task"))
        .headers(write_headers(&config)?)
        .json(&request)
        .send()
        .await
        .map_err(unreachable_service_hint)?;
    if !capture.status().is_success() {
        let status = capture.status();
        let body = capture.text().await.unwrap_or_default();
        anyhow::bail!("capture failed ({status}): {body}");
    }

    let curate = client
        .post(service_url(&config, "/v1/curate"))
        .headers(write_headers(&config)?)
        .json(&CurateRequest {
            project: args.project.clone(),
            batch_size: None,
            raw_capture_id: None,
            replacement_policy: None,
            dry_run: false,
        })
        .send()
        .await
        .map_err(unreachable_service_hint)?;
    if !curate.status().is_success() {
        let status = curate.status();
        let body = curate.text().await.unwrap_or_default();
        anyhow::bail!("curation failed ({status}): {body}");
    }

    let project = &args.project;
    println!("Loaded {candidate_count} demo memories into project '{project}'.\n");
    println!("Try one of these:");
    println!("  memory query --project {project} --question \"How does reinforcement work?\"");
    println!(
        "  memory query --project {project} --question \"What does the service need to run?\""
    );
    println!("  memory tui   # browse and search everything, including the memory graph");
    Ok(())
}

/// Map a transport-level failure (service not running) to an actionable hint.
fn unreachable_service_hint(err: reqwest::Error) -> anyhow::Error {
    anyhow::anyhow!(
        "could not reach the Memory Layer service: {err}\n\
         Start it first — `docker compose up` (bundled stack) or `memory service run` — then rerun `memory demo`."
    )
}
