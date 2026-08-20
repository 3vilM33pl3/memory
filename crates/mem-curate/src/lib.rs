// SPDX-License-Identifier: AGPL-3.0-or-later

// Tests may unwrap; production code must not (workspace lints deny it).
#![cfg_attr(test, allow(clippy::unwrap_used))]

mod repository;

pub use repository::{
    SemanticDuplicate, apply_validation_revision, approve_replacement_proposal, curate,
    list_replacement_proposals, preview_capture, preview_curate, refresh_memory_relations,
    refresh_semantic_relations, reject_replacement_proposal, store_capture,
};
