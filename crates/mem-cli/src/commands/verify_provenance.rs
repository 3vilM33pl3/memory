// SPDX-License-Identifier: AGPL-3.0-or-later

use anyhow::{Context, Result};
use mem_config::AppConfig;
use mem_record::ProvenanceVerificationRequest;
use reqwest::Client;
use std::env;

use crate::commands::{
    api::ApiClient, memory_ops::resolve_project_slug,
    output::print_provenance_verification_response, runtime::VerifyProvenanceArgs,
};

pub(super) async fn handle(
    args: VerifyProvenanceArgs,
    client: Client,
    config: AppConfig,
) -> Result<()> {
    let cwd = env::current_dir().context("read current directory")?;
    let api = ApiClient::new(client.clone(), config.clone());
    let project = resolve_project_slug(args.project, &cwd)?;
    let repo_root = args
        .repo_root
        .as_ref()
        .map(|path| path.to_string_lossy().to_string());
    let response = api
        .verify_provenance(&ProvenanceVerificationRequest {
            project,
            repo_root,
            dry_run: args.dry_run,
        })
        .await?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&response)?);
    } else {
        print_provenance_verification_response(&response);
    }

    Ok(())
}
