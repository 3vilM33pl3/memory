use anyhow::{Context, Result};
use std::env;

use crate::commands::{
    runtime::UpgradeArgs,
    skill_support::{print_skill_upgrade_report, resolve_repo_root, upgrade_project_skills},
};

pub(super) async fn handle(args: &UpgradeArgs) -> Result<()> {
    let cwd = env::current_dir().context("read current directory")?;
    let repo_root = resolve_repo_root(&cwd)?;
    let report = upgrade_project_skills(&repo_root, args.force, args.dry_run)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_skill_upgrade_report(&report);
    }
    Ok(())
}
