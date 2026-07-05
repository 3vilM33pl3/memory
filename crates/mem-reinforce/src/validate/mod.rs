//! Threshold-triggered validation pipeline: deterministic evidence
//! gathering, a pluggable verdict provider (LLM in the service; an
//! agent-CLI/worktree runner can implement the same trait later), and a
//! strict apply policy. Validation reads never count as memory accesses.

pub mod apply;
pub mod evidence;
pub mod verdict;

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use crate::repository::{
    EvidenceRow, ValidationRunCompletion, complete_validation_run, fail_validation_run,
    insert_score_audit, insert_validation_evidence, insert_validation_run, mark_correction_pending,
    mark_needs_review, mark_validated, set_validation_cooldown,
};
use crate::selection::ValidationCandidate;
pub use apply::{Decision, ValidationAction, ValidationPolicy, decide};
pub use evidence::{ValidationContext, gather_context};
pub use verdict::{RawVerdict, ValidatedVerdict, Verdict, parse_verdict_content, validate_verdict};

/// What initiated a validation run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationTrigger {
    Threshold,
    Curator,
    Manual,
    Scheduled,
}

impl ValidationTrigger {
    pub fn as_str(self) -> &'static str {
        match self {
            ValidationTrigger::Threshold => "threshold",
            ValidationTrigger::Curator => "curator",
            ValidationTrigger::Manual => "manual",
            ValidationTrigger::Scheduled => "scheduled",
        }
    }
}

/// Produces a verdict for a gathered context. Implementations must not
/// mutate any state; the pipeline owns all persistence.
#[async_trait]
pub trait VerdictProvider: Send + Sync {
    async fn assess(&self, context: &ValidationContext) -> Result<RawVerdict>;

    /// Model identifier recorded on the validation run, if any.
    fn model_name(&self) -> Option<String> {
        None
    }
}

/// Result of one validation run.
#[derive(Debug, Clone)]
pub struct ValidationOutcome {
    pub run_id: Uuid,
    pub verdict: Verdict,
    pub confidence: f32,
    pub action: ValidationAction,
    pub dry_run: bool,
    /// New memory version id when rewording was auto-applied.
    pub new_memory_id: Option<Uuid>,
}

/// Cooldown after a failed run: short, so transient provider errors are
/// retried well before the regular cooldown would allow.
const FAILURE_COOLDOWN_HOURS: i64 = 24;

/// Runs the full pipeline for one candidate. Every path leaves an auditable
/// trace: a run row (completed or failed), evidence rows for completed
/// runs, a score-audit entry, and a cooldown so no memory can loop.
pub async fn run_validation(
    pool: &PgPool,
    candidate: &ValidationCandidate,
    provider: &dyn VerdictProvider,
    policy: &ValidationPolicy,
    trigger: ValidationTrigger,
) -> Result<ValidationOutcome> {
    let run_id = insert_validation_run(
        pool,
        candidate.canonical_id,
        candidate.memory_id,
        candidate.project_id,
        trigger.as_str(),
        policy.dry_run,
    )
    .await?;

    let outcome = execute_validation(pool, candidate, provider, policy, run_id).await;
    if let Err(error) = &outcome {
        fail_validation_run(pool, run_id, &format!("{error:#}")).await?;
        set_validation_cooldown(
            pool,
            candidate.canonical_id,
            Utc::now() + chrono::Duration::hours(FAILURE_COOLDOWN_HOURS),
        )
        .await?;
    }
    outcome
}

async fn execute_validation(
    pool: &PgPool,
    candidate: &ValidationCandidate,
    provider: &dyn VerdictProvider,
    policy: &ValidationPolicy,
    run_id: Uuid,
) -> Result<ValidationOutcome> {
    let started = std::time::Instant::now();
    let context = gather_context(pool, candidate.memory_id).await?;
    let raw = provider
        .assess(&context)
        .await
        .context("verdict provider failed")?;
    let verdict = validate_verdict(raw, &context).context("verdict failed validation")?;
    let decision = decide(&verdict, policy);

    let evidence_rows: Vec<EvidenceRow> = verdict
        .evidence
        .iter()
        .map(|item| EvidenceRow {
            kind: item.kind.as_str().to_string(),
            evidence_ref: item.evidence_ref.clone(),
            stance: item.stance.as_str().to_string(),
            excerpt: item.excerpt.clone(),
        })
        .collect();
    insert_validation_evidence(pool, run_id, &evidence_rows).await?;

    let proposed_candidate_json = if decision.store_proposal {
        Some(serde_json::json!({
            "proposed_summary": verdict.proposed_summary,
            "proposed_text": verdict.proposed_text,
            "previous_memory_id": candidate.memory_id,
        }))
    } else {
        None
    };

    // Apply side effects, unless this is a dry run: dry runs record the
    // run + evidence and change nothing else — except the cooldown, which
    // is pure scheduling state; without it a dry-run deployment would
    // re-validate the same hottest memory every scheduler cycle.
    let cooldown_until = Utc::now() + policy.cooldown;
    let mut new_memory_id = None;
    let mut review_status: Option<&'static str> = None;
    if policy.dry_run {
        set_validation_cooldown(pool, candidate.canonical_id, cooldown_until).await?;
    } else {
        match decision.action {
            ValidationAction::Revalidated => {
                mark_validated(
                    pool,
                    candidate.canonical_id,
                    verdict.confidence,
                    run_id,
                    cooldown_until,
                )
                .await?;
            }
            ValidationAction::Reworded => {
                let summary = verdict
                    .proposed_summary
                    .clone()
                    .unwrap_or_else(|| context.memory.summary.clone());
                let text = verdict
                    .proposed_text
                    .clone()
                    .unwrap_or_else(|| context.memory.canonical_text.clone());
                let applied = mem_curate::apply_validation_revision(
                    pool,
                    candidate.memory_id,
                    &summary,
                    &text,
                )
                .await
                .context("apply validated rewording")?;
                new_memory_id = Some(applied);
                mark_validated(
                    pool,
                    candidate.canonical_id,
                    verdict.confidence,
                    run_id,
                    cooldown_until,
                )
                .await?;
            }
            ValidationAction::CorrectionPending => {
                review_status = Some("pending");
                mark_correction_pending(
                    pool,
                    candidate.canonical_id,
                    run_id,
                    cooldown_until,
                    decision.invalidated,
                )
                .await?;
            }
            ValidationAction::FlaggedNeedsReview => {
                let reason = decision
                    .needs_review_reason
                    .clone()
                    .unwrap_or_else(|| "weak or contradictory evidence".to_string());
                mark_needs_review(
                    pool,
                    candidate.canonical_id,
                    &reason,
                    run_id,
                    cooldown_until,
                )
                .await?;
                insert_score_audit(
                    pool,
                    candidate.canonical_id,
                    candidate.project_id,
                    "needs_review_set",
                    None,
                    None,
                    serde_json::json!({ "reason": reason, "run_id": run_id }),
                )
                .await?;
            }
        }
    }

    complete_validation_run(
        pool,
        run_id,
        &ValidationRunCompletion {
            verdict: verdict.verdict.as_str(),
            confidence: verdict.confidence,
            action: decision.action.as_str(policy.dry_run).to_string(),
            reasons: serde_json::json!(verdict.reasons),
            proposed_candidate_json,
            review_status,
            model: provider.model_name(),
            details: serde_json::json!({
                "trigger_activation": candidate.activation,
                "volatility": candidate.volatility,
                "evidence_count": evidence_rows.len(),
                "git_log_lines": context.git_log.len(),
                "duration_ms": started.elapsed().as_millis() as u64,
                "new_memory_id": new_memory_id,
            }),
        },
    )
    .await?;

    insert_score_audit(
        pool,
        candidate.canonical_id,
        candidate.project_id,
        "validation_completed",
        Some(candidate.activation),
        Some(candidate.activation),
        serde_json::json!({
            "run_id": run_id,
            "verdict": verdict.verdict.as_str(),
            "confidence": verdict.confidence,
            "action": decision.action.as_str(policy.dry_run),
            "dry_run": policy.dry_run,
        }),
    )
    .await?;

    Ok(ValidationOutcome {
        run_id,
        verdict: verdict.verdict,
        confidence: verdict.confidence,
        action: decision.action,
        dry_run: policy.dry_run,
        new_memory_id,
    })
}

#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    use super::evidence::ValidationContext;
    use crate::repository::MemorySnapshot;
    use chrono::Utc;
    use uuid::Uuid;

    /// Bare context for unit tests: no DB, the given paths allowed as
    /// evidence references.
    pub fn minimal_context(allowed_paths: &[&str]) -> ValidationContext {
        let mut context = ValidationContext {
            memory: MemorySnapshot {
                memory_id: Uuid::new_v4(),
                canonical_id: Uuid::new_v4(),
                project_id: Uuid::new_v4(),
                project_slug: "test".to_string(),
                repo_root: String::new(),
                summary: "Test memory".to_string(),
                canonical_text: "Test memory canonical text.".to_string(),
                memory_type: "implementation".to_string(),
                importance: 3,
                confidence: 0.9,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
            tags: Vec::new(),
            sources: Vec::new(),
            related: Vec::new(),
            prior_runs: Vec::new(),
            git_log: Vec::new(),
            allowed_refs: Default::default(),
        };
        for path in allowed_paths {
            context.insert_allowed_reference(path);
        }
        context
    }
}
