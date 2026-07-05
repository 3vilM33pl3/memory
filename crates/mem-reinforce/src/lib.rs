//! Memory reinforcement: access-driven activation scoring with spreading
//! activation, time decay, volatility tracking, and threshold-triggered
//! validation of memories against project evidence.
//!
//! Scoring math is pure (`scoring`, `propagation`, `selection`); all
//! database access lives in `repository`; the validation pipeline is in
//! `validate` behind the [`validate::VerdictProvider`] trait so this crate
//! never talks to an LLM directly.

pub mod propagation;
pub mod repository;
pub mod scoring;
pub mod selection;

pub use propagation::{CanonicalEdge, PropagatedIncrement, propagation_increments};
pub use scoring::{
    AccessKind, ScoreParams, activation_rank_boost, apply_boost, decayed, update_volatility,
};
pub use selection::{ThresholdInput, ValidationCandidate, validation_due};
