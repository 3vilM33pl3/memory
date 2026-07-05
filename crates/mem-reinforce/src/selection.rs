//! Due-for-validation predicate. The SQL scan lives in `repository`; this
//! module holds the pure decision logic and the candidate type shared by the
//! curator workflow and the background scheduler.

use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

/// A memory whose score state makes it a potential validation candidate.
#[derive(Debug, Clone)]
pub struct ValidationCandidate {
    pub canonical_id: Uuid,
    pub memory_id: Uuid,
    pub project_id: Uuid,
    pub activation: f64,
    pub volatility: f32,
    pub validated_at: Option<DateTime<Utc>>,
    pub needs_review: bool,
    pub cooldown_until: Option<DateTime<Utc>>,
}

/// Inputs for the threshold predicate, decoupled from the DB row shape so it
/// is trivially unit-testable.
#[derive(Debug, Clone)]
pub struct ThresholdInput {
    pub activation: f64,
    pub threshold: f64,
    pub needs_review: bool,
    pub cooldown_until: Option<DateTime<Utc>>,
    pub validated_at: Option<DateTime<Utc>>,
    pub volatility: f32,
}

/// A memory is due for validation when its (already decay-corrected)
/// activation has crossed the threshold, it is not awaiting human review,
/// any post-validation cooldown has elapsed, and it has either never been
/// validated or its volatility-adjusted revalidation interval has passed:
/// `validated_at + min_revalidation / (1 + volatility * volatility_factor)`.
/// Higher volatility shortens the interval, so memories about frequently
/// changing artefacts are re-checked more often.
pub fn validation_due(
    input: &ThresholdInput,
    min_revalidation: Duration,
    volatility_factor: f64,
    now: DateTime<Utc>,
) -> bool {
    if input.needs_review {
        return false;
    }
    if input.activation < input.threshold {
        return false;
    }
    if let Some(cooldown) = input.cooldown_until
        && cooldown > now
    {
        return false;
    }
    match input.validated_at {
        None => true,
        Some(validated_at) => {
            let scale = 1.0 + f64::from(input.volatility.max(0.0)) * volatility_factor.max(0.0);
            let interval_ms = (min_revalidation.num_milliseconds() as f64 / scale) as i64;
            validated_at + Duration::milliseconds(interval_ms) <= now
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn base() -> ThresholdInput {
        ThresholdInput {
            activation: 10.0,
            threshold: 8.0,
            needs_review: false,
            cooldown_until: None,
            validated_at: None,
            volatility: 0.0,
        }
    }

    #[test]
    fn due_when_over_threshold_and_never_validated() {
        assert!(validation_due(&base(), Duration::days(14), 4.0, t0()));
    }

    #[test]
    fn not_due_below_threshold() {
        let input = ThresholdInput {
            activation: 7.9,
            ..base()
        };
        assert!(!validation_due(&input, Duration::days(14), 4.0, t0()));
    }

    #[test]
    fn not_due_when_needs_review() {
        let input = ThresholdInput {
            needs_review: true,
            ..base()
        };
        assert!(!validation_due(&input, Duration::days(14), 4.0, t0()));
    }

    #[test]
    fn not_due_during_cooldown_due_after() {
        let input = ThresholdInput {
            cooldown_until: Some(t0() + Duration::days(1)),
            ..base()
        };
        assert!(!validation_due(&input, Duration::days(14), 4.0, t0()));
        assert!(validation_due(
            &input,
            Duration::days(14),
            4.0,
            t0() + Duration::days(2)
        ));
    }

    #[test]
    fn revalidation_waits_for_min_interval() {
        let input = ThresholdInput {
            validated_at: Some(t0()),
            ..base()
        };
        assert!(!validation_due(
            &input,
            Duration::days(14),
            4.0,
            t0() + Duration::days(13)
        ));
        assert!(validation_due(
            &input,
            Duration::days(14),
            4.0,
            t0() + Duration::days(14)
        ));
    }

    #[test]
    fn volatility_shortens_revalidation_interval() {
        // volatility 1.0, factor 4.0 → interval / 5 = 2.8 days.
        let input = ThresholdInput {
            validated_at: Some(t0()),
            volatility: 1.0,
            ..base()
        };
        assert!(!validation_due(
            &input,
            Duration::days(14),
            4.0,
            t0() + Duration::days(2)
        ));
        assert!(validation_due(
            &input,
            Duration::days(14),
            4.0,
            t0() + Duration::days(3)
        ));
    }

    #[test]
    fn negative_volatility_is_treated_as_zero() {
        let input = ThresholdInput {
            validated_at: Some(t0()),
            volatility: -5.0,
            ..base()
        };
        assert!(!validation_due(
            &input,
            Duration::days(14),
            4.0,
            t0() + Duration::days(13)
        ));
    }
}
