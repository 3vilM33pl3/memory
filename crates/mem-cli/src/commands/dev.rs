use anyhow::{Context, Result};
use std::env;

use crate::commands::{
    init_support::initialize_dev_overlay,
    runtime::{DevArgs, DevCommand},
    skill_support::resolve_repo_root,
};

pub(super) async fn handle(args: &DevArgs) -> Result<()> {
    match &args.command {
        DevCommand::Init(init_args) => {
            let cwd = env::current_dir().context("read current directory")?;
            let repo_root = resolve_repo_root(&cwd)?;
            let output = initialize_dev_overlay(&repo_root, init_args)?;
            println!("{output}");
        }
    }
    Ok(())
}
