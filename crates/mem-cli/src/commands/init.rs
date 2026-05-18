use anyhow::{Context, Result};
use std::env;

use crate::commands::runtime::*;

pub(crate) async fn handle(args: &InitArgs) -> Result<()> {
    let cwd = env::current_dir().context("read current directory")?;
    let project = resolve_project_slug(args.project.clone(), &cwd)?;
    let repo_root = resolve_repo_root(&cwd)?;
    let output = initialize_repo(&repo_root, &project, args.force, args.dry_run)?;
    println!("{output}");
    Ok(())
}
