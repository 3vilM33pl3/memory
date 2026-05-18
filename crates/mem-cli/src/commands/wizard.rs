use anyhow::{Context, Result};
use std::env;

use crate::commands::{
    memory_ops::resolve_project_slug, runtime::WizardArgs, skill_support::resolve_repo_root,
};
use crate::wizard as wizard_runtime;

pub(crate) async fn handle(args: &WizardArgs) -> Result<()> {
    let cwd = env::current_dir().context("read current directory")?;
    let repo_root = resolve_repo_root(&cwd)?;
    let project = if repo_root == cwd || repo_root.join(".git").exists() {
        resolve_project_slug(args.project.clone(), &repo_root).ok()
    } else {
        args.project.clone()
    };
    wizard_runtime::run(&cwd, &repo_root, project, args.global, args.dry_run).await
}
