// SPDX-License-Identifier: AGPL-3.0-or-later

//! Transition facade: re-exports the record and config crates so existing
//! `mem_api::` imports keep compiling while dependents migrate. Deleted at
//! the end of the boundary wave - do not add new items here.

pub use mem_config::*;
pub use mem_record::*;
