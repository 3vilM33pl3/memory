use anyhow::{Context, Result};
use std::env;

use crate::commands::runtime::*;

pub(crate) async fn handle(args: &DevArgs) -> Result<()> {
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
