// SPDX-License-Identifier: AGPL-3.0-or-later

use anyhow::{Context, Result};
use std::env;

use crate::commands::{
    memory_ops::resolve_project_slug, runtime::WizardArgs, skill_support::resolve_repo_root,
};
use crate::wizard as wizard_runtime;

pub(super) async fn handle(args: &WizardArgs) -> Result<()> {
    run_wizard(args.project.clone(), args.global, args.dry_run).await
}

async fn run_wizard(project: Option<String>, global: bool, dry_run: bool) -> Result<()> {
    let cwd = env::current_dir().context("read current directory")?;
    let repo_root = resolve_repo_root(&cwd)?;
    let project = if repo_root == cwd || repo_root.join(".git").exists() {
        resolve_project_slug(project, &repo_root).ok()
    } else {
        project
    };
    wizard_runtime::run(&cwd, &repo_root, project, global, dry_run).await
}
