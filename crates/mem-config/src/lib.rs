// SPDX-License-Identifier: AGPL-3.0-or-later

// Tests may unwrap; production code must not (workspace lints deny it).
#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod app;
pub mod auth;
pub mod discovery;
pub mod embeddings;
#[cfg(test)]
mod legacy_tests;
pub mod llm;
pub mod pipeline;
pub mod profile;
pub mod repo;

pub use app::*;
pub use auth::*;
pub use discovery::*;
pub use embeddings::*;
pub use llm::*;
pub use pipeline::*;
pub use profile::*;
pub use repo::*;
