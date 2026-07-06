//! Deterministic memory-cluster discovery for the consolidation feature.
//!
//! Three retrieval-quality signals (relation edges, embedding similarity, and
//! co-access) are fused into one weighted graph over canonical memory ids, a
//! single community-detection pass groups related memories, and a value gate
//! decides which clusters are worth consolidating into a higher-level
//! "insight" memory. This crate is pure algorithm — no database, no LLM, no
//! wall-clock or randomness — so results are reproducible and unit-testable.

mod detect;
mod fuse;
mod graph;
mod value;

pub use detect::{Community, DetectParams, detect_communities};
pub use fuse::{FuseWeights, fuse_edges};
pub use graph::{FusedGraph, WeightedEdge};
pub use value::{
    ClusterMetrics, GateOutcome, MemberStat, TriggerReason, ValueGateConfig, evaluate_cluster,
};
