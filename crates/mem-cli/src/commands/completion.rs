use anyhow::{Context, Result};
use clap::CommandFactory;
use clap_complete::generate;
use std::io::{self, Write};

use crate::commands::runtime::{Cli, CompletionArgs};

pub(crate) async fn handle(args: &CompletionArgs) -> Result<()> {
    let mut command = Cli::command();
    let mut output = Vec::new();
    generate(args.shell, &mut command, "memory", &mut output);
    if let Err(error) = io::stdout().write_all(&output)
        && error.kind() != io::ErrorKind::BrokenPipe
    {
        return Err(error).context("write completion script");
    }
    Ok(())
}
