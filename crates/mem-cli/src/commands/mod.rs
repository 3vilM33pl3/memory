mod activities;
pub(crate) mod api;
mod archive;
mod automation;
mod bundle;
mod capture;
mod checkpoint;
mod commits;
mod completion;
mod curate;
mod dev;
mod doctor;
mod embeddings;
mod eval;
pub(crate) mod eval_support;
mod graph;
mod health;
mod history;
mod init;
pub(crate) mod init_support;
mod mcp;
pub(crate) mod memory_ops;
pub(crate) mod output;
mod proposals;
mod prune_history;
mod query;
mod remember;
mod repo;
mod resume;
pub(crate) mod runtime;
mod scan;
mod service;
pub(crate) mod service_support;
pub(crate) mod skill_support;
mod stats;
mod status;
pub(crate) mod status_support;
mod tui;
mod up_to_speed;
mod upgrade;
mod verify_provenance;
pub(crate) mod watch_support;
mod watcher;
mod wizard;

pub(crate) async fn run() -> anyhow::Result<()> {
    runtime::run().await
}
