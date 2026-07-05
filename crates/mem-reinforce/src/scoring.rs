//! Pure activation-scoring math. No database access and no clock reads:
//! every function takes `now` explicitly so behaviour is fully deterministic
//! under test.
//!
//! The model is ACT-R base-level activation in Petrov's O(1) incremental
//! form: a single running activation value decays exponentially between
//! accesses and receives an additive boost on each access, instead of
//! replaying the full access history.

use chrono::{DateTime, Duration, Utc};

/// Tunable scoring parameters, mirrored from `ReinforcementConfig`.
#[derive(Debug, Clone)]
pub struct ScoreParams {
    /// Activation halves after this much time without access.
    pub half_life: Duration,
    /// Boost for a memory returned in query results.
    pub direct_boost: f64,
    /// Boost for a memory cited in a synthesized answer (replaces, not
    /// stacks with, the direct boost for the same access).
    pub citation_boost: f64,
    /// Boost for a direct single-memory read (get/resume).
    pub direct_read_boost: f64,
    /// Per-hop decay factor for spreading activation.
    pub hop_decay: f64,
    /// Maximum graph distance activation spreads to.
    pub max_hops: u8,
    /// Divide propagated increments by the fan-out of the node they spread
    /// from (ACT-R fan effect) so hub nodes do not inflate their neighbours.
    pub fan_normalization: bool,
    /// Propagated increments below this are dropped entirely.
    pub min_propagated_increment: f64,
    /// Hard ceiling on activation.
    pub max_activation: f64,
}

impl Default for ScoreParams {
    fn default() -> Self {
        Self {
            half_life: Duration::days(30),
            direct_boost: 1.0,
            citation_boost: 1.5,
            direct_read_boost: 0.25,
            hop_decay: 0.5,
            max_hops: 2,
            fan_normalization: true,
            min_propagated_increment: 0.05,
            max_activation: 20.0,
        }
    }
}

impl From<&mem_api::ReinforcementConfig> for ScoreParams {
    fn from(config: &mem_api::ReinforcementConfig) -> Self {
        Self {
            half_life: Duration::from_std(config.half_life).unwrap_or_else(|_| Duration::days(30)),
            direct_boost: config.direct_access_boost,
            citation_boost: config.citation_boost,
            direct_read_boost: config.direct_read_boost,
            hop_decay: config.hop_decay,
            max_hops: config.max_hops,
            fan_normalization: config.fan_normalization,
            min_propagated_increment: config.min_propagated_increment,
            max_activation: config.max_activation,
        }
    }
}

/// Kinds of access that feed the score, with their configured boosts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessKind {
    Retrieval,
    Citation,
    DirectRead,
}

impl AccessKind {
    pub fn boost(self, params: &ScoreParams) -> f64 {
        match self {
            AccessKind::Retrieval => params.direct_boost,
            AccessKind::Citation => params.citation_boost,
            AccessKind::DirectRead => params.direct_read_boost,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            AccessKind::Retrieval => "retrieval",
            AccessKind::Citation => "citation",
            AccessKind::DirectRead => "direct_read",
        }
    }
}

/// Exponential decay of a running activation value:
/// `a * 0.5^(elapsed / half_life)`.
pub fn decayed(
    activation: f64,
    last_decay_at: DateTime<Utc>,
    now: DateTime<Utc>,
    half_life: Duration,
) -> f64 {
    let elapsed = (now - last_decay_at).num_milliseconds();
    if elapsed <= 0 || activation <= 0.0 {
        return activation.max(0.0);
    }
    let half_life_ms = half_life.num_milliseconds();
    if half_life_ms <= 0 {
        return 0.0;
    }
    activation * 0.5_f64.powf(elapsed as f64 / half_life_ms as f64)
}

/// Decay-then-boost update, clamped to `max_activation`.
pub fn apply_boost(
    activation: f64,
    last_decay_at: DateTime<Utc>,
    now: DateTime<Utc>,
    boost: f64,
    params: &ScoreParams,
) -> f64 {
    (decayed(activation, last_decay_at, now, params.half_life) + boost)
        .clamp(0.0, params.max_activation)
}

/// Ranking contribution of an activation value: `weight * ln(1 + a)`, capped.
/// Logarithmic so the score -> rank -> access feedback loop saturates.
pub fn activation_rank_boost(activation: f64, weight: f64, cap: f64) -> f64 {
    (weight * (1.0 + activation.max(0.0)).ln()).min(cap)
}

/// Exponentially weighted moving average of provenance-file change events,
/// expressed as events per day (update-risk TTL model).
pub fn update_volatility(old: f32, change_events: u32, elapsed_days: f64, alpha: f64) -> f32 {
    if elapsed_days <= 0.0 {
        return old;
    }
    let rate = f64::from(change_events) / elapsed_days;
    let alpha = alpha.clamp(0.0, 1.0);
    (alpha * rate + (1.0 - alpha) * f64::from(old)) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn decay_halves_at_exactly_one_half_life() {
        let now = t0() + Duration::days(30);
        let result = decayed(8.0, t0(), now, Duration::days(30));
        assert!((result - 4.0).abs() < 1e-9, "got {result}");
    }

    #[test]
    fn decay_is_identity_for_zero_elapsed_or_negative_elapsed() {
        assert_eq!(decayed(3.0, t0(), t0(), Duration::days(30)), 3.0);
        let earlier = t0() - Duration::days(1);
        assert_eq!(decayed(3.0, t0(), earlier, Duration::days(30)), 3.0);
    }

    #[test]
    fn decay_never_returns_negative() {
        assert_eq!(
            decayed(-1.0, t0(), t0() + Duration::days(1), Duration::days(30)),
            0.0
        );
    }

    #[test]
    fn apply_boost_decays_before_boosting_and_clamps() {
        let params = ScoreParams::default();
        let now = t0() + Duration::days(30);
        // 8.0 decays to 4.0, then +1.0 boost.
        let result = apply_boost(8.0, t0(), now, 1.0, &params);
        assert!((result - 5.0).abs() < 1e-9, "got {result}");

        let clamped = apply_boost(19.9, t0(), t0(), 5.0, &params);
        assert_eq!(clamped, params.max_activation);
    }

    #[test]
    fn access_kind_boosts_follow_params() {
        let params = ScoreParams::default();
        assert_eq!(AccessKind::Retrieval.boost(&params), 1.0);
        assert_eq!(AccessKind::Citation.boost(&params), 1.5);
        assert_eq!(AccessKind::DirectRead.boost(&params), 0.25);
    }

    #[test]
    fn rank_boost_is_logarithmic_and_capped() {
        let unboosted = activation_rank_boost(0.0, 0.3, 1.2);
        assert!(unboosted.abs() < 1e-12);
        let mid = activation_rank_boost(5.0, 0.3, 1.2);
        assert!((mid - 0.3 * 6.0_f64.ln()).abs() < 1e-9);
        // ln(1+20)*0.3 ≈ 0.913 < cap; force cap with a big weight
        assert_eq!(activation_rank_boost(20.0, 1.0, 1.2), 1.2);
        // negative activation treated as zero
        assert!(activation_rank_boost(-3.0, 0.3, 1.2).abs() < 1e-12);
    }

    #[test]
    fn volatility_ewma_converges_toward_observed_rate() {
        let mut v = 0.0_f32;
        for _ in 0..40 {
            v = update_volatility(v, 2, 1.0, 0.3);
        }
        assert!((f64::from(v) - 2.0).abs() < 1e-3, "got {v}");
    }

    #[test]
    fn volatility_ignores_nonpositive_elapsed() {
        assert_eq!(update_volatility(1.5, 10, 0.0, 0.3), 1.5);
    }
}
