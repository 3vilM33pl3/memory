//! ACT-R procedural utility learning: the delta rule
//! `U_n = U_{n-1} + alpha * (R_n - U_{n-1})` (Rescorla–Wagner / temporal
//! difference; Fu & Anderson 2006). Pure and deterministic — no database, no
//! clock, and deliberately no ACT-R utility noise term, which would violate
//! the workspace's deterministic-by-default principle. Utility is advisory
//! only: it informs ordering and recommendations, never permission gates.

/// Learning parameters for the delta rule.
#[derive(Debug, Clone, Copy)]
pub struct UtilityParams {
    /// Learning rate in `[0, 1]`.
    pub alpha: f64,
    /// U_0 used as `prev` for a producer's first update.
    pub initial_utility: f64,
    /// Clamp floor, keeps utilities bounded and auditable.
    pub min_utility: f64,
    /// Clamp ceiling.
    pub max_utility: f64,
}

impl Default for UtilityParams {
    fn default() -> Self {
        Self {
            alpha: 0.2,
            initial_utility: 0.0,
            min_utility: -5.0,
            max_utility: 10.0,
        }
    }
}

/// The ACT-R delta rule. `prev` is U_{n-1}, `reward` is R_n.
pub fn apply_utility_update(prev: f64, reward: f64, params: &UtilityParams) -> f64 {
    let alpha = params.alpha.clamp(0.0, 1.0);
    (prev + alpha * (reward - prev)).clamp(params.min_utility, params.max_utility)
}

/// Reward magnitudes per event, sourced from config.
#[derive(Debug, Clone, Copy)]
pub struct ProceduralRewards {
    pub approved: f64,
    pub edited_approved: f64,
    pub rejected: f64,
    pub run_error: f64,
    pub cited: f64,
}

impl Default for ProceduralRewards {
    fn default() -> Self {
        Self {
            approved: 1.0,
            edited_approved: 0.4,
            rejected: -1.0,
            run_error: -0.2,
            cited: 0.5,
        }
    }
}

/// A named reward event, so call sites express intent and the audit reason
/// string is derived in exactly one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewardEvent {
    /// A proposal from this producer was approved as-is.
    ProposalApproved,
    /// A proposal was approved only after human editing (partial credit).
    ProposalEditedApproved,
    /// A proposal was rejected.
    ProposalRejected,
    /// A run of this producer errored.
    LoopRunError,
    /// A memory this producer created was later cited in an answer.
    MemoryCited,
}

impl RewardEvent {
    pub fn reward(self, rewards: &ProceduralRewards) -> f64 {
        match self {
            Self::ProposalApproved => rewards.approved,
            Self::ProposalEditedApproved => rewards.edited_approved,
            Self::ProposalRejected => rewards.rejected,
            Self::LoopRunError => rewards.run_error,
            Self::MemoryCited => rewards.cited,
        }
    }

    /// Stable string stored in `procedural_utility_audit.reason`.
    pub fn audit_reason(self) -> &'static str {
        match self {
            Self::ProposalApproved => "proposal_approved",
            Self::ProposalEditedApproved => "proposal_edited_approved",
            Self::ProposalRejected => "proposal_rejected",
            Self::LoopRunError => "loop_run_error",
            Self::MemoryCited => "memory_cited",
        }
    }
}

impl From<&mem_api::ProceduralConfig> for UtilityParams {
    fn from(config: &mem_api::ProceduralConfig) -> Self {
        Self {
            alpha: config.alpha,
            initial_utility: config.initial_utility,
            min_utility: config.min_utility,
            max_utility: config.max_utility,
        }
    }
}

impl From<&mem_api::ProceduralConfig> for ProceduralRewards {
    fn from(config: &mem_api::ProceduralConfig) -> Self {
        Self {
            approved: config.reward_approved,
            edited_approved: config.reward_edited_approved,
            rejected: config.reward_rejected,
            run_error: config.reward_run_error,
            cited: config.reward_cited,
        }
    }
}

impl From<&mem_api::ProceduralConfig> for RecommendationThresholds {
    fn from(config: &mem_api::ProceduralConfig) -> Self {
        Self {
            min_samples: config.min_samples,
            snooze_threshold: config.snooze_threshold,
            keep_threshold: config.keep_threshold,
        }
    }
}

/// A producer's learned utility as read back from storage.
#[derive(Debug, Clone)]
pub struct UtilitySnapshot {
    pub producer_id: String,
    pub utility: f64,
    pub update_count: i64,
}

/// Thresholds for the advisory recommendation text.
#[derive(Debug, Clone, Copy)]
pub struct RecommendationThresholds {
    /// Minimum updates before any recommendation (guards reward sparsity).
    pub min_samples: i64,
    /// At or below this utility, suggest snoozing.
    pub snooze_threshold: f64,
    /// At or above this utility, affirm the producer.
    pub keep_threshold: f64,
}

impl Default for RecommendationThresholds {
    fn default() -> Self {
        Self {
            min_samples: 5,
            snooze_threshold: -0.5,
            keep_threshold: 0.5,
        }
    }
}

/// Advisory recommendation string for a producer, or `None` when the sample
/// is too small or the utility is unremarkable. Never mutates anything — the
/// human acts on it (or not).
pub fn utility_recommendation(
    snapshot: &UtilitySnapshot,
    thresholds: &RecommendationThresholds,
) -> Option<String> {
    if snapshot.update_count < thresholds.min_samples {
        return None;
    }
    if snapshot.utility <= thresholds.snooze_threshold {
        return Some(format!(
            "Outcomes from this loop are consistently negative (utility {:.2} over {} decisions); consider snoozing it.",
            snapshot.utility, snapshot.update_count
        ));
    }
    if snapshot.utility >= thresholds.keep_threshold {
        return Some(format!(
            "High-value loop (utility {:.2} over {} decisions); its proposals are consistently accepted.",
            snapshot.utility, snapshot.update_count
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approve_raises_and_reject_lowers() {
        let params = UtilityParams::default();
        let rewards = ProceduralRewards::default();
        let up = apply_utility_update(0.0, RewardEvent::ProposalApproved.reward(&rewards), &params);
        let down =
            apply_utility_update(0.0, RewardEvent::ProposalRejected.reward(&rewards), &params);
        assert!(up > 0.0);
        assert!(down < 0.0);
        // Edited approval earns strictly less than a clean approval.
        let edited = apply_utility_update(
            0.0,
            RewardEvent::ProposalEditedApproved.reward(&rewards),
            &params,
        );
        assert!(edited > 0.0 && edited < up);
    }

    #[test]
    fn converges_to_constant_reward() {
        let params = UtilityParams::default();
        let mut utility = params.initial_utility;
        for _ in 0..100 {
            utility = apply_utility_update(utility, 1.0, &params);
        }
        assert!((utility - 1.0).abs() < 1e-6);
    }

    #[test]
    fn clamps_to_bounds() {
        let params = UtilityParams {
            alpha: 1.0,
            initial_utility: 0.0,
            min_utility: -1.0,
            max_utility: 1.0,
        };
        assert_eq!(apply_utility_update(0.0, 50.0, &params), 1.0);
        assert_eq!(apply_utility_update(0.0, -50.0, &params), -1.0);
    }

    #[test]
    fn alpha_zero_is_identity_and_alpha_one_jumps() {
        let base = UtilityParams::default();
        let frozen = UtilityParams { alpha: 0.0, ..base };
        assert_eq!(apply_utility_update(0.3, 5.0, &frozen), 0.3);
        let eager = UtilityParams { alpha: 1.0, ..base };
        assert_eq!(apply_utility_update(0.3, 5.0, &eager), 5.0);
    }

    #[test]
    fn recommendation_respects_min_samples_and_thresholds() {
        let thresholds = RecommendationThresholds::default();
        let sparse = UtilitySnapshot {
            producer_id: "loop".into(),
            utility: -3.0,
            update_count: 2,
        };
        assert!(utility_recommendation(&sparse, &thresholds).is_none());

        let bad = UtilitySnapshot { update_count: 8, ..sparse.clone() };
        assert!(
            utility_recommendation(&bad, &thresholds)
                .is_some_and(|text| text.contains("snoozing"))
        );

        let good = UtilitySnapshot {
            producer_id: "loop".into(),
            utility: 0.9,
            update_count: 8,
        };
        assert!(
            utility_recommendation(&good, &thresholds)
                .is_some_and(|text| text.contains("High-value"))
        );

        let middling = UtilitySnapshot { utility: 0.1, ..good };
        assert!(utility_recommendation(&middling, &thresholds).is_none());
    }
}
