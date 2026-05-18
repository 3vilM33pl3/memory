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

pub use repository::*;
pub use runtime::run_service;

pub(crate) use auth::*;
pub(crate) use error::*;
pub(crate) use handlers::{activity::*, curation::*, query::*};
pub(crate) use repository::events::*;
pub(crate) use repository::handlers::{
    bundle::*, embeddings::*, memory::*, project::*, provenance::*, system::*,
};
pub(crate) use repository::stream::*;
pub(crate) use runtime::*;
pub(crate) use runtime_status::*;
pub(crate) use state::*;

#[cfg(test)]
mod tests;
