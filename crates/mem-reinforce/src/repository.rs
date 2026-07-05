//! Database access for reinforcement scoring. All sqlx queries in this
//! crate live here, per workspace convention.

#![allow(dead_code)]

// Populated in the access-recording phase: score upserts, access events,
// canonical edge fetch, validation-candidate scan, retention pruning.
