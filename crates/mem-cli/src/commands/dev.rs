// SPDX-License-Identifier: AGPL-3.0-or-later

use anyhow::{Context, Result};
use std::env;

use crate::commands::{
    init_support::initialize_dev_overlay,
    runtime::DevInitArgs,
    skill_support::resolve_repo_root,
};

pub(super) async fn handle(args: &DevInitArgs) -> Result<()> {
    let cwd = env::current_dir().context("read current directory")?;
    let repo_root = resolve_repo_root(&cwd)?;
    let output = initialize_dev_overlay(&repo_root, args)?;
    println!("{output}");
    Ok(())
}
