use std::fs;

use anyhow::{Context, Result};
use mem_api::ProjectMemoryExportOptions;

use crate::{
    ApiClient, BundleArgs, BundleCommand, print_bundle_import_preview, print_bundle_import_response,
};

pub(crate) async fn handle(args: BundleArgs, api: &ApiClient) -> Result<()> {
    match args.command {
        BundleCommand::Export(args) => {
            let options = ProjectMemoryExportOptions {
                include_archived: args.include_archived,
                include_tags: true,
                include_relations: true,
                include_source_file_paths: args.include_source_file_paths,
                include_git_commits: args.include_git_commits,
                include_source_excerpts: args.include_source_excerpts,
            };
            let preview = api.export_bundle_preview(&args.project, &options).await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "project": args.project,
                    "output": args.out.display().to_string(),
                    "preview": preview,
                    "dry_run": args.dry_run,
                }))?
            );
            if !args.dry_run {
                let bytes = api.export_bundle(&args.project, &options).await?;
                fs::write(&args.out, bytes)
                    .with_context(|| format!("write {}", args.out.display()))?;
            }
        }
        BundleCommand::Import(args) => {
            let bytes = fs::read(&args.bundle)
                .with_context(|| format!("read {}", args.bundle.display()))?;
            if args.dry_run {
                let preview = api.import_bundle_preview(&args.project, bytes).await?;
                if args.json {
                    println!("{}", serde_json::to_string_pretty(&preview)?);
                } else {
                    print_bundle_import_preview(&preview);
                }
            } else {
                let response = api.import_bundle(&args.project, bytes).await?;
                if args.json {
                    println!("{}", serde_json::to_string_pretty(&response)?);
                } else {
                    print_bundle_import_response(&response);
                }
            }
        }
    }
    Ok(())
}
