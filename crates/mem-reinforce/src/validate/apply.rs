//! Pure apply-policy state machine (stage 3 of validation): maps a
//! validated verdict and the configured policy to exactly one action.
//! The cardinal rule: weak or contradictory evidence NEVER modifies memory
//! content — it flags the memory for human review instead.

use super::verdict::{ValidatedVerdict, Verdict};

/// What validation decided to do with a memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationAction {
    /// Memory confirmed accurate and clearly worded.
    Revalidated,
    /// High-confidence wording improvement applied as a new version.
    Reworded,
    /// A proposed rewording or correction awaits human review.
    CorrectionPending,
    /// Weak, ambiguous, or contradictory evidence: human attention needed.
    FlaggedNeedsReview,
}

impl ValidationAction {
    /// Persisted action string; dry runs record what *would* happen.
    pub fn as_str(self, dry_run: bool) -> &'static str {
        match (self, dry_run) {
            (ValidationAction::Revalidated, false) => "revalidated",
            (ValidationAction::Revalidated, true) => "would_revalidate",
            (ValidationAction::Reworded, false) => "reworded",
            (ValidationAction::Reworded, true) => "would_reword",
            (ValidationAction::CorrectionPending, false) => "correction_pending",
            (ValidationAction::CorrectionPending, true) => "would_queue_correction",
            (ValidationAction::FlaggedNeedsReview, false) => "flagged_needs_review",
            (ValidationAction::FlaggedNeedsReview, true) => "would_flag_needs_review",
        }
    }
}

/// Policy knobs, mirrored from `ReinforcementConfig`.
#[derive(Debug, Clone)]
pub struct ValidationPolicy {
    pub dry_run: bool,
    pub auto_apply_rewording: bool,
    pub auto_apply_min_confidence: f32,
    pub needs_review_min_confidence: f32,
    pub cooldown: chrono::Duration,
}

impl From<&mem_api::ReinforcementConfig> for ValidationPolicy {
    fn from(config: &mem_api::ReinforcementConfig) -> Self {
        Self {
            dry_run: config.validation_dry_run,
            auto_apply_rewording: config.auto_apply_rewording,
            auto_apply_min_confidence: config.auto_apply_min_confidence,
            needs_review_min_confidence: config.needs_review_min_confidence,
            cooldown: chrono::Duration::from_std(config.validation_cooldown)
                .unwrap_or_else(|_| chrono::Duration::days(7)),
        }
    }
}

/// The full decision derived from a verdict.
#[derive(Debug, Clone)]
pub struct Decision {
    pub action: ValidationAction,
    /// Reason attached when flagging for review.
    pub needs_review_reason: Option<String>,
    /// Whether `last_invalidated_at` should be stamped (verdict found the
    /// content no longer accurate).
    pub invalidated: bool,
    /// Whether a proposed candidate (rewording or correction) should be
    /// stored on the run for review or auto-application.
    pub store_proposal: bool,
}

pub fn decide(verdict: &ValidatedVerdict, policy: &ValidationPolicy) -> Decision {
    let has_proposal = verdict.proposed_summary.is_some() || verdict.proposed_text.is_some();

    // Weak evidence beats everything: never act on a low-confidence or
    // inherently uncertain verdict.
    if verdict.confidence < policy.needs_review_min_confidence
        || matches!(verdict.verdict, Verdict::Ambiguous | Verdict::Unsupported)
    {
        return Decision {
            action: ValidationAction::FlaggedNeedsReview,
            needs_review_reason: Some(format!(
                "verdict {} at confidence {:.2}",
                verdict.verdict.as_str(),
                verdict.confidence
            )),
            invalidated: false,
            store_proposal: has_proposal,
        };
    }

    match verdict.verdict {
        Verdict::Valid => {
            if verdict.clarity_ok || !has_proposal {
                Decision {
                    action: ValidationAction::Revalidated,
                    needs_review_reason: None,
                    invalidated: false,
                    store_proposal: false,
                }
            } else if policy.auto_apply_rewording
                && !policy.dry_run
                && verdict.confidence >= policy.auto_apply_min_confidence
            {
                Decision {
                    action: ValidationAction::Reworded,
                    needs_review_reason: None,
                    invalidated: false,
                    store_proposal: true,
                }
            } else {
                Decision {
                    action: ValidationAction::CorrectionPending,
                    needs_review_reason: None,
                    invalidated: false,
                    store_proposal: true,
                }
            }
        }
        Verdict::Outdated | Verdict::PartiallyValid => {
            if has_proposal {
                // Corrections are always human-gated, regardless of
                // confidence: the memory stays active until review.
                Decision {
                    action: ValidationAction::CorrectionPending,
                    needs_review_reason: None,
                    invalidated: true,
                    store_proposal: true,
                }
            } else {
                Decision {
                    action: ValidationAction::FlaggedNeedsReview,
                    needs_review_reason: Some(format!(
                        "verdict {} without a proposed correction",
                        verdict.verdict.as_str()
                    )),
                    invalidated: true,
                    store_proposal: false,
                }
            }
        }
        Verdict::Ambiguous | Verdict::Unsupported => unreachable!("handled above"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate::verdict::ValidatedVerdict;

    fn verdict(kind: Verdict, confidence: f32) -> ValidatedVerdict {
        ValidatedVerdict {
            verdict: kind,
            confidence,
            reasons: Vec::new(),
            evidence: Vec::new(),
            proposed_summary: None,
            proposed_text: None,
            clarity_ok: true,
        }
    }

    fn policy() -> ValidationPolicy {
        ValidationPolicy {
            dry_run: false,
            auto_apply_rewording: true,
            auto_apply_min_confidence: 0.85,
            needs_review_min_confidence: 0.5,
            cooldown: chrono::Duration::days(7),
        }
    }

    #[test]
    fn valid_and_clear_revalidates() {
        let decision = decide(&verdict(Verdict::Valid, 0.9), &policy());
        assert_eq!(decision.action, ValidationAction::Revalidated);
        assert!(!decision.invalidated);
    }

    #[test]
    fn low_confidence_always_flags() {
        for kind in [
            Verdict::Valid,
            Verdict::Outdated,
            Verdict::PartiallyValid,
            Verdict::Ambiguous,
            Verdict::Unsupported,
        ] {
            let decision = decide(&verdict(kind, 0.3), &policy());
            assert_eq!(
                decision.action,
                ValidationAction::FlaggedNeedsReview,
                "verdict {kind:?} at low confidence must flag"
            );
        }
    }

    #[test]
    fn ambiguous_and_unsupported_flag_even_at_high_confidence() {
        for kind in [Verdict::Ambiguous, Verdict::Unsupported] {
            let decision = decide(&verdict(kind, 0.95), &policy());
            assert_eq!(decision.action, ValidationAction::FlaggedNeedsReview);
        }
    }

    #[test]
    fn unclear_wording_rewrites_only_with_high_confidence_and_flag_enabled() {
        let mut unclear = verdict(Verdict::Valid, 0.9);
        unclear.clarity_ok = false;
        unclear.proposed_summary = Some("Clearer summary".to_string());

        let decision = decide(&unclear, &policy());
        assert_eq!(decision.action, ValidationAction::Reworded);

        let below_bar = ValidatedVerdict {
            confidence: 0.7,
            ..unclear.clone()
        };
        assert_eq!(
            decide(&below_bar, &policy()).action,
            ValidationAction::CorrectionPending
        );

        let auto_off = ValidationPolicy {
            auto_apply_rewording: false,
            ..policy()
        };
        assert_eq!(
            decide(&unclear, &auto_off).action,
            ValidationAction::CorrectionPending
        );

        let dry = ValidationPolicy {
            dry_run: true,
            ..policy()
        };
        assert_eq!(
            decide(&unclear, &dry).action,
            ValidationAction::CorrectionPending,
            "dry-run never auto-applies"
        );
    }

    #[test]
    fn unclear_wording_without_proposal_still_revalidates() {
        let mut unclear = verdict(Verdict::Valid, 0.9);
        unclear.clarity_ok = false;
        assert_eq!(
            decide(&unclear, &policy()).action,
            ValidationAction::Revalidated
        );
    }

    #[test]
    fn outdated_with_correction_queues_review_and_invalidates() {
        let mut outdated = verdict(Verdict::Outdated, 0.9);
        outdated.proposed_text = Some("Corrected fact".to_string());
        let decision = decide(&outdated, &policy());
        assert_eq!(decision.action, ValidationAction::CorrectionPending);
        assert!(decision.invalidated);
        assert!(decision.store_proposal);
    }

    #[test]
    fn outdated_without_correction_flags_and_invalidates() {
        let decision = decide(&verdict(Verdict::Outdated, 0.9), &policy());
        assert_eq!(decision.action, ValidationAction::FlaggedNeedsReview);
        assert!(decision.invalidated);
    }

    #[test]
    fn action_strings_reflect_dry_run() {
        assert_eq!(ValidationAction::Reworded.as_str(false), "reworded");
        assert_eq!(ValidationAction::Reworded.as_str(true), "would_reword");
        assert_eq!(
            ValidationAction::FlaggedNeedsReview.as_str(true),
            "would_flag_needs_review"
        );
    }
}
