use anyhow::{Context, Result};
use std::{env, path::PathBuf};

use crate::commands::runtime::*;

pub(crate) async fn handle(args: &DoctorArgs, cli_config: Option<PathBuf>) -> Result<()> {
    let cwd = env::current_dir().context("read current directory")?;
    let repo_root = resolve_repo_root(&cwd)?;
    let project = resolve_project_slug(args.project.clone(), &cwd).unwrap_or_else(|_| {
        repo_root
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("memory")
            .to_string()
    });
    let report = run_doctor(cli_config, &repo_root, &project, args.fix).await?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_doctor_report(&report);
    }
    Ok(())
}
