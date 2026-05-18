mod auth;
mod error;
mod events;
mod handlers;
mod mcp_http;
mod prelude;
pub mod repository;
mod routes;
mod runtime;
mod runtime_status;
mod state;
mod stream;

pub use repository::*;
pub use runtime::run_service;

pub(crate) use auth::*;
pub(crate) use error::*;
pub(crate) use events::*;
pub(crate) use handlers::{
    activity::*, bundle::*, curation::*, embeddings::*, memory::*, project::*, provenance::*,
    query::*, system::*,
};
pub(crate) use runtime::*;
pub(crate) use runtime_status::*;
pub(crate) use state::*;
pub(crate) use stream::*;

#[cfg(test)]
mod tests;
