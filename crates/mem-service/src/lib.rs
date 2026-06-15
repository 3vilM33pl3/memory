mod auth;
mod error;
mod handlers;
mod mcp_http;
mod prelude;
pub mod repository;
mod routes;
mod runtime;
mod runtime_status;
mod state;

pub use repository::{
    fetch_project_commit, fetch_project_commits, fetch_project_memories, fetch_project_overview,
    parse_status_filter, preview_project_commit_sync, row_to_commit_record, sync_project_commits,
};
pub use runtime::run_service;

pub(crate) use auth::*;
pub(crate) use error::*;
pub(crate) use handlers::{activity::*, curation::*, query::*};
pub(crate) use repository::events::*;
pub(crate) use repository::handlers::{
    bundle::*, embeddings::*, loops::*, memory::*, project::*, provenance::*, system::*,
};
pub(crate) use repository::stream::*;
pub(crate) use runtime::*;
pub(crate) use runtime_status::*;
pub(crate) use state::*;

#[cfg(test)]
mod tests;
