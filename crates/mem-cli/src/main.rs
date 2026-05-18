mod commands;
mod commits;
mod plan_execution;
mod resume;
mod scan;
mod tui;
mod wizard;
mod writer_identity;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    commands::run().await
}
