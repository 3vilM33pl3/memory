//! Memory reinforcement: access-driven activation scoring with spreading
//! activation, time decay, volatility tracking, and threshold-triggered
//! validation of memories against project evidence.
//!
//! Scoring math is pure (`scoring`, `propagation`, `selection`); all
//! database access lives in `repository`; the validation pipeline is in
//! `validate` behind the [`validate::VerdictProvider`] trait so this crate
//! never talks to an LLM directly.

pub mod procedural;
pub mod propagation;
pub mod recorder;
pub mod repository;
pub mod scoring;
pub mod selection;
pub mod validate;

pub use procedural::{
    ProceduralRewards, RecommendationThresholds, RewardEvent, UtilityParams, UtilitySnapshot,
    apply_utility_update, utility_recommendation,
};
pub use propagation::{CanonicalEdge, PropagatedIncrement, propagation_increments};
pub use recorder::{AccessBatch, ThresholdCrossing, record_access_batch};
pub use scoring::{
    AccessKind, ScoreParams, activation_rank_boost, apply_boost, decayed, update_volatility,
};
pub use selection::{ThresholdInput, ValidationCandidate, validation_due};
pub use validate::{
    RawVerdict, ReviewResolution, ValidationAction, ValidationContext, ValidationOutcome,
    ValidationPolicy, ValidationTrigger, VerdictProvider, resolve_review, run_validation,
};
