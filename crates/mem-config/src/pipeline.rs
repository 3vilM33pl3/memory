// SPDX-License-Identifier: AGPL-3.0-or-later

#[allow(unused_imports)]
use std::{
    collections::HashMap,
    env, fmt,
    path::{Path, PathBuf},
    time::Duration,
};

#[allow(unused_imports)]
use chrono::{DateTime, Utc};
#[allow(unused_imports)]
use config::{Config, ConfigError, Environment, File, FileFormat};
#[allow(unused_imports)]
use mem_platform::discover_existing_global_config_path;
#[allow(unused_imports)]
use mem_record::*;
#[allow(unused_imports)]
use serde::{Deserialize, Serialize};
#[allow(unused_imports)]
use thiserror::Error;
#[allow(unused_imports)]
use uuid::Uuid;

#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub mode: AutomationMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<String>,
    #[serde(default = "default_poll_interval")]
    #[serde(with = "humantime_serde")]
    pub poll_interval: Duration,
    #[serde(default = "default_file_events")]
    pub file_events: bool,
    #[serde(default = "default_capture_idle_threshold", alias = "idle_threshold")]
    #[serde(with = "humantime_serde")]
    pub capture_idle_threshold: Duration,
    #[serde(default = "default_min_changed_files")]
    pub min_changed_files: usize,
    #[serde(default)]
    pub require_passing_test: bool,
    #[serde(default = "default_curate_after_captures")]
    pub curate_after_captures: usize,
    #[serde(default = "default_curate_on_explicit_flush")]
    pub curate_on_explicit_flush: bool,
    #[serde(default)]
    pub ignored_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audit_log_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_file_path: Option<String>,
}

/// Service-side curation knobs. Distinct from the per-repo
/// `curation.replacement_policy` agent setting: these control the
/// embedding-based dedup pass that runs after chunk embeddings are built.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurationConfig {
    #[serde(default = "default_true")]
    pub semantic_dedup_enabled: bool,
    /// Minimum max-chunk cosine similarity for two memories to be linked as
    /// semantic duplicates and queued for merge review.
    #[serde(default = "default_semantic_duplicate_threshold")]
    pub semantic_duplicate_threshold: f64,
}

impl Default for CurationConfig {
    fn default() -> Self {
        Self {
            semantic_dedup_enabled: true,
            semantic_duplicate_threshold: default_semantic_duplicate_threshold(),
        }
    }
}

pub fn default_semantic_duplicate_threshold() -> f64 {
    0.90
}

/// Controls memory consolidation: discovering clusters of related memories and
/// synthesizing higher-level `insight` memories. Off by default (LLM cost);
/// enable with dry-run first, matching the reinforcement validation posture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub dry_run: bool,
    /// Auto-run the consolidation loop when the usage accumulator crosses the
    /// salience threshold (still bounded by `enabled`, `dry_run`, `daily_cap`).
    #[serde(default = "default_true")]
    pub auto_trigger: bool,
    // Fusion / clustering knobs.
    #[serde(default = "default_consolidation_sim_floor")]
    pub sim_floor: f64,
    #[serde(default = "default_consolidation_knn_k")]
    pub knn_k: i64,
    #[serde(default = "default_consolidation_weight")]
    pub relation_weight: f64,
    #[serde(default = "default_consolidation_weight")]
    pub similarity_weight: f64,
    #[serde(default = "default_consolidation_weight")]
    pub coaccess_weight: f64,
    #[serde(default = "default_consolidation_coaccess_norm")]
    pub coaccess_norm: f64,
    #[serde(default = "default_consolidation_coaccess_window_days")]
    pub coaccess_window_days: i64,
    #[serde(default = "default_consolidation_min_coaccess")]
    pub min_coaccess_count: i64,
    // Value-gate knobs.
    #[serde(default = "default_consolidation_min_size")]
    pub min_size: usize,
    #[serde(default = "default_consolidation_max_size")]
    pub max_size: usize,
    #[serde(default = "default_consolidation_min_cohesion")]
    pub min_cohesion: f64,
    #[serde(default = "default_consolidation_min_salience")]
    pub min_salience: f64,
    #[serde(default = "default_consolidation_cold_activation_max")]
    pub cold_activation_max: f64,
    /// Fraction of a cluster already summarized by an existing insight above
    /// which the cluster is skipped as non-novel.
    #[serde(default = "default_consolidation_novelty_overlap_max")]
    pub novelty_overlap_max: f64,
    // Synthesis knobs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default = "default_consolidation_max_output_tokens")]
    pub max_output_tokens_cap: u32,
    #[serde(default = "default_consolidation_daily_cap")]
    pub daily_cap: u32,
}

impl Default for ConsolidationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            dry_run: true,
            auto_trigger: true,
            sim_floor: default_consolidation_sim_floor(),
            knn_k: default_consolidation_knn_k(),
            relation_weight: default_consolidation_weight(),
            similarity_weight: default_consolidation_weight(),
            coaccess_weight: default_consolidation_weight(),
            coaccess_norm: default_consolidation_coaccess_norm(),
            coaccess_window_days: default_consolidation_coaccess_window_days(),
            min_coaccess_count: default_consolidation_min_coaccess(),
            min_size: default_consolidation_min_size(),
            max_size: default_consolidation_max_size(),
            min_cohesion: default_consolidation_min_cohesion(),
            min_salience: default_consolidation_min_salience(),
            cold_activation_max: default_consolidation_cold_activation_max(),
            novelty_overlap_max: default_consolidation_novelty_overlap_max(),
            model: None,
            max_output_tokens_cap: default_consolidation_max_output_tokens(),
            daily_cap: default_consolidation_daily_cap(),
        }
    }
}

pub fn default_consolidation_sim_floor() -> f64 {
    0.82
}

pub fn default_consolidation_knn_k() -> i64 {
    8
}

pub fn default_consolidation_weight() -> f64 {
    1.0
}

pub fn default_consolidation_coaccess_norm() -> f64 {
    4.0
}

pub fn default_consolidation_coaccess_window_days() -> i64 {
    30
}

pub fn default_consolidation_min_coaccess() -> i64 {
    2
}

pub fn default_consolidation_min_size() -> usize {
    3
}

pub fn default_consolidation_max_size() -> usize {
    25
}

pub fn default_consolidation_min_cohesion() -> f64 {
    0.35
}

pub fn default_consolidation_min_salience() -> f64 {
    2.0
}

pub fn default_consolidation_cold_activation_max() -> f64 {
    1.0
}

pub fn default_consolidation_novelty_overlap_max() -> f64 {
    0.5
}

pub fn default_consolidation_max_output_tokens() -> u32 {
    1400
}

pub fn default_consolidation_daily_cap() -> u32 {
    20
}

/// ACT-R procedural utility learning: per-loop learned value from proposal
/// decisions and citations. Deterministic and advisory — it informs listing
/// order and recommendations, never permission gates — so it defaults on,
/// like activation scoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProceduralConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Delta-rule learning rate, 0..1.
    #[serde(default = "default_procedural_alpha")]
    pub alpha: f64,
    #[serde(default)]
    pub initial_utility: f64,
    #[serde(default = "default_procedural_min_utility")]
    pub min_utility: f64,
    #[serde(default = "default_procedural_max_utility")]
    pub max_utility: f64,
    // Reward magnitudes.
    #[serde(default = "default_reward_approved")]
    pub reward_approved: f64,
    #[serde(default = "default_reward_edited_approved")]
    pub reward_edited_approved: f64,
    #[serde(default = "default_reward_rejected")]
    pub reward_rejected: f64,
    #[serde(default = "default_reward_run_error")]
    pub reward_run_error: f64,
    #[serde(default = "default_reward_cited")]
    pub reward_cited: f64,
    // Recommendation thresholds.
    #[serde(default = "default_procedural_min_samples")]
    pub min_samples: i64,
    #[serde(default = "default_procedural_snooze_threshold")]
    pub snooze_threshold: f64,
    #[serde(default = "default_procedural_keep_threshold")]
    pub keep_threshold: f64,
    /// When on, auto-trigger paths skip firing loops whose learned utility is
    /// below `utility_floor` (with at least `min_samples` decisions). Manual
    /// runs and permission gates are never affected.
    #[serde(default)]
    pub utility_floor_enabled: bool,
    #[serde(default)]
    pub utility_floor: f64,
}

impl Default for ProceduralConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            alpha: default_procedural_alpha(),
            initial_utility: 0.0,
            min_utility: default_procedural_min_utility(),
            max_utility: default_procedural_max_utility(),
            reward_approved: default_reward_approved(),
            reward_edited_approved: default_reward_edited_approved(),
            reward_rejected: default_reward_rejected(),
            reward_run_error: default_reward_run_error(),
            reward_cited: default_reward_cited(),
            min_samples: default_procedural_min_samples(),
            snooze_threshold: default_procedural_snooze_threshold(),
            keep_threshold: default_procedural_keep_threshold(),
            utility_floor_enabled: false,
            utility_floor: 0.0,
        }
    }
}

pub fn default_procedural_alpha() -> f64 {
    0.2
}

pub fn default_procedural_min_utility() -> f64 {
    -5.0
}

pub fn default_procedural_max_utility() -> f64 {
    10.0
}

pub fn default_reward_approved() -> f64 {
    1.0
}

pub fn default_reward_edited_approved() -> f64 {
    0.4
}

pub fn default_reward_rejected() -> f64 {
    -1.0
}

pub fn default_reward_run_error() -> f64 {
    -0.2
}

pub fn default_reward_cited() -> f64 {
    0.5
}

pub fn default_procedural_min_samples() -> i64 {
    5
}

pub fn default_procedural_snooze_threshold() -> f64 {
    -0.5
}

pub fn default_procedural_keep_threshold() -> f64 {
    0.5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceConfig {
    #[serde(default = "default_missing_file_decay")]
    pub missing_file_decay: f64,
    #[serde(default = "default_missing_symbol_decay")]
    pub missing_symbol_decay: f64,
    #[serde(default = "default_stale_decay")]
    pub stale_decay: f64,
    #[serde(default = "default_true")]
    pub reverify_enabled: bool,
    #[serde(default = "default_reverify_interval")]
    #[serde(with = "humantime_serde")]
    pub reverify_interval: Duration,
}

impl Default for ProvenanceConfig {
    fn default() -> Self {
        Self {
            missing_file_decay: default_missing_file_decay(),
            missing_symbol_decay: default_missing_symbol_decay(),
            stale_decay: default_stale_decay(),
            reverify_enabled: true,
            reverify_interval: default_reverify_interval(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RetentionConfig {
    /// Delete a canonical memory entirely (all its versions) when its
    /// latest version is a tombstone older than this duration. Default
    /// None means "never prune tombstones".
    #[serde(default, with = "humantime_serde::option")]
    pub tombstone_after: Option<Duration>,
    /// Delete superseded (non-latest, non-tombstone) versions whose
    /// `updated_at` is older than this duration. The latest version of
    /// each canonical memory is always kept. Default None means "never
    /// prune superseded versions".
    #[serde(default, with = "humantime_serde::option")]
    pub superseded_after: Option<Duration>,
}

/// Access-driven memory reinforcement: activation scoring, spreading
/// activation over memory relations, volatility tracking, and
/// threshold-triggered validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReinforcementConfig {
    /// Master switch for access scoring, decay, and ranking integration.
    /// Cheap and deterministic; involves no LLM calls.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Enables the threshold-triggered LLM validation pipeline (scheduler
    /// and curator trigger). Off by default because it costs LLM calls.
    #[serde(default)]
    pub validation_enabled: bool,
    /// When validation is enabled, only report what would change instead
    /// of applying rewording or queuing corrections.
    #[serde(default = "default_true")]
    pub validation_dry_run: bool,
    /// Activation boost when a memory is returned in query results.
    #[serde(default = "default_direct_access_boost")]
    pub direct_access_boost: f64,
    /// Activation boost when a memory is cited in a synthesized answer
    /// (replaces the retrieval boost for that access).
    #[serde(default = "default_citation_boost")]
    pub citation_boost: f64,
    /// Activation boost for a direct single-memory read (get/resume).
    #[serde(default = "default_direct_read_boost")]
    pub direct_read_boost: f64,
    /// Activation halves after this long without access.
    #[serde(default = "default_reinforcement_half_life")]
    #[serde(with = "humantime_serde")]
    pub half_life: Duration,
    /// Per-hop decay factor for spreading activation to linked memories.
    #[serde(default = "default_hop_decay")]
    pub hop_decay: f64,
    /// Maximum graph distance activation spreads to.
    #[serde(default = "default_max_hops")]
    pub max_hops: u8,
    /// Divide propagated increments by the fan-out of the node they
    /// spread from, so hub memories do not inflate all their neighbours.
    #[serde(default = "default_true")]
    pub fan_normalization: bool,
    /// Propagated increments below this are dropped.
    #[serde(default = "default_min_propagated_increment")]
    pub min_propagated_increment: f64,
    /// Hard ceiling on activation.
    #[serde(default = "default_max_activation")]
    pub max_activation: f64,
    /// Activation at which a memory becomes due for validation.
    #[serde(default = "default_validation_threshold")]
    pub validation_threshold: f64,
    /// Minimum wait after any validation run before the same memory can
    /// be validated again (hysteresis).
    #[serde(default = "default_validation_cooldown")]
    #[serde(with = "humantime_serde")]
    pub validation_cooldown: Duration,
    /// Base revalidation interval for already-validated memories; divided
    /// by `1 + volatility * volatility_revalidation_factor`.
    #[serde(default = "default_min_revalidation_interval")]
    #[serde(with = "humantime_serde")]
    pub min_revalidation_interval: Duration,
    /// How strongly volatility shortens the revalidation interval.
    #[serde(default = "default_volatility_revalidation_factor")]
    pub volatility_revalidation_factor: f64,
    /// Smoothing factor for the volatility EWMA (0..1).
    #[serde(default = "default_volatility_ewma_alpha")]
    pub volatility_ewma_alpha: f64,
    /// Background reinforcement scheduler tick interval.
    #[serde(default = "default_reinforcement_scheduler_interval")]
    #[serde(with = "humantime_serde")]
    pub scheduler_interval: Duration,
    /// Maximum validations started per scheduler cycle per project.
    #[serde(default = "default_validation_batch_size")]
    pub validation_batch_size: u32,
    /// Global cap on non-dry-run validation runs per rolling day.
    #[serde(default = "default_daily_validation_cap")]
    pub daily_validation_cap: u32,
    /// Allow automatic application of high-confidence rewording of
    /// still-valid memories. Corrections are always human-gated.
    #[serde(default)]
    pub auto_apply_rewording: bool,
    /// Minimum verdict confidence for automatic rewording.
    #[serde(default = "default_auto_apply_min_confidence")]
    pub auto_apply_min_confidence: f32,
    /// Verdict confidence below this flags the memory for review instead
    /// of acting on the verdict.
    #[serde(default = "default_needs_review_min_confidence")]
    pub needs_review_min_confidence: f32,
    /// Weight of `ln(1 + activation)` in search ranking.
    #[serde(default = "default_activation_rank_weight")]
    pub activation_rank_weight: f64,
    /// Cap on the activation ranking boost.
    #[serde(default = "default_activation_rank_cap")]
    pub activation_rank_cap: f64,
    /// Multiplier applied to the final score of memories flagged for
    /// review (penalty, not exclusion).
    #[serde(default = "default_needs_review_rank_penalty")]
    pub needs_review_rank_penalty: f64,
    /// How long individual access events are kept before pruning.
    #[serde(default = "default_access_event_retention")]
    #[serde(with = "humantime_serde")]
    pub access_event_retention: Duration,
    /// Capacity of the bounded access-recording channel; overflow drops
    /// events (scoring is advisory, load-shedding is deliberate).
    #[serde(default = "default_access_channel_capacity")]
    pub access_channel_capacity: usize,
}

impl Default for ReinforcementConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            validation_enabled: false,
            validation_dry_run: true,
            direct_access_boost: default_direct_access_boost(),
            citation_boost: default_citation_boost(),
            direct_read_boost: default_direct_read_boost(),
            half_life: default_reinforcement_half_life(),
            hop_decay: default_hop_decay(),
            max_hops: default_max_hops(),
            fan_normalization: true,
            min_propagated_increment: default_min_propagated_increment(),
            max_activation: default_max_activation(),
            validation_threshold: default_validation_threshold(),
            validation_cooldown: default_validation_cooldown(),
            min_revalidation_interval: default_min_revalidation_interval(),
            volatility_revalidation_factor: default_volatility_revalidation_factor(),
            volatility_ewma_alpha: default_volatility_ewma_alpha(),
            scheduler_interval: default_reinforcement_scheduler_interval(),
            validation_batch_size: default_validation_batch_size(),
            daily_validation_cap: default_daily_validation_cap(),
            auto_apply_rewording: false,
            auto_apply_min_confidence: default_auto_apply_min_confidence(),
            needs_review_min_confidence: default_needs_review_min_confidence(),
            activation_rank_weight: default_activation_rank_weight(),
            activation_rank_cap: default_activation_rank_cap(),
            needs_review_rank_penalty: default_needs_review_rank_penalty(),
            access_event_retention: default_access_event_retention(),
            access_channel_capacity: default_access_channel_capacity(),
        }
    }
}

impl Default for AutomationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: AutomationMode::Suggest,
            repo_root: None,
            poll_interval: default_poll_interval(),
            file_events: default_file_events(),
            capture_idle_threshold: default_capture_idle_threshold(),
            min_changed_files: default_min_changed_files(),
            require_passing_test: false,
            curate_after_captures: default_curate_after_captures(),
            curate_on_explicit_flush: default_curate_on_explicit_flush(),
            ignored_paths: Vec::new(),
            audit_log_path: None,
            state_file_path: None,
        }
    }
}

pub fn default_file_events() -> bool {
    true
}

pub fn default_missing_file_decay() -> f64 {
    0.5
}

pub fn default_missing_symbol_decay() -> f64 {
    0.7
}

pub fn default_stale_decay() -> f64 {
    0.85
}

pub fn default_reverify_interval() -> Duration {
    Duration::from_secs(24 * 60 * 60)
}

pub fn default_direct_access_boost() -> f64 {
    1.0
}

pub fn default_citation_boost() -> f64 {
    1.5
}

pub fn default_direct_read_boost() -> f64 {
    0.25
}

pub fn default_reinforcement_half_life() -> Duration {
    Duration::from_secs(30 * 24 * 60 * 60)
}

pub fn default_hop_decay() -> f64 {
    0.5
}

pub fn default_max_hops() -> u8 {
    2
}

pub fn default_min_propagated_increment() -> f64 {
    0.05
}

pub fn default_max_activation() -> f64 {
    20.0
}

pub fn default_validation_threshold() -> f64 {
    8.0
}

pub fn default_validation_cooldown() -> Duration {
    Duration::from_secs(7 * 24 * 60 * 60)
}

pub fn default_min_revalidation_interval() -> Duration {
    Duration::from_secs(14 * 24 * 60 * 60)
}

pub fn default_volatility_revalidation_factor() -> f64 {
    4.0
}

pub fn default_volatility_ewma_alpha() -> f64 {
    0.3
}

pub fn default_reinforcement_scheduler_interval() -> Duration {
    Duration::from_secs(15 * 60)
}

pub fn default_validation_batch_size() -> u32 {
    3
}

pub fn default_daily_validation_cap() -> u32 {
    20
}

pub fn default_auto_apply_min_confidence() -> f32 {
    0.85
}

pub fn default_needs_review_min_confidence() -> f32 {
    0.5
}

pub fn default_activation_rank_weight() -> f64 {
    0.3
}

pub fn default_activation_rank_cap() -> f64 {
    1.2
}

pub fn default_needs_review_rank_penalty() -> f64 {
    0.6
}

pub fn default_access_event_retention() -> Duration {
    Duration::from_secs(30 * 24 * 60 * 60)
}

pub fn default_access_channel_capacity() -> usize {
    1024
}

pub fn default_poll_interval() -> Duration {
    Duration::from_secs(60)
}

pub fn default_capture_idle_threshold() -> Duration {
    Duration::from_secs(600)
}

pub fn default_min_changed_files() -> usize {
    2
}

pub fn default_curate_after_captures() -> usize {
    3
}

pub fn default_curate_on_explicit_flush() -> bool {
    true
}
