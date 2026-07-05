//! Background reinforcement scheduler: retention pruning, score compaction,
//! and the threshold-triggered validation sweep. Modeled on
//! `run_provenance_reverify_scheduler`. Runs only on the primary node.

use crate::prelude::*;
use mem_api::ValidationDueInfo;
use mem_reinforce::repository::{
    SelectionParams, compact_scores, count_validation_runs_since, fetch_due_candidates,
    insert_score_audit, last_compaction_at, prune_access_events,
};
use mem_reinforce::selection::ValidationCandidate;

use crate::state::AppState;

const COMPACTION_INTERVAL_DAYS: i64 = 7;
/// Volatility EWMA moves larger than this get an audit row.
const VOLATILITY_AUDIT_DELTA: f32 = 0.5;

pub(crate) async fn run_reinforcement_scheduler(state: AppState) -> Result<()> {
    tokio::time::sleep(StdDuration::from_secs(10)).await;
    let interval = state
        .config
        .reinforcement
        .scheduler_interval
        .max(StdDuration::from_secs(60));
    let notify = state
        .reinforcement
        .as_ref()
        .map(|runtime| runtime.notify.clone());
    loop {
        if state.is_primary()
            && let Err(error) = reinforcement_sweep_once(&state).await
        {
            tracing::warn!(error = %error, "reinforcement sweep failed");
            if let Some(runtime) = &state.reinforcement {
                let mut status = runtime
                    .status
                    .lock()
                    .expect("reinforcement status mutex poisoned");
                status.status = "error".to_string();
                status.error = Some(error.to_string());
            }
        }
        match &notify {
            Some(notify) => {
                tokio::select! {
                    _ = tokio::time::sleep(interval) => {}
                    _ = notify.notified() => {}
                }
            }
            None => tokio::time::sleep(interval).await,
        }
    }
}

pub(crate) async fn reinforcement_sweep_once(state: &AppState) -> Result<()> {
    let Ok(pool) = state.pool() else {
        return Ok(());
    };
    let config = &state.config.reinforcement;
    let Some(runtime) = &state.reinforcement else {
        return Ok(());
    };

    {
        let mut status = runtime
            .status
            .lock()
            .expect("reinforcement status mutex poisoned");
        status.status = "running".to_string();
        status.last_started_at = Some(chrono::Utc::now());
        status.last_finished_at = None;
        status.error = None;
    }

    // 1. Retention: prune old access events.
    let retention_cutoff = chrono::Utc::now()
        - chrono::Duration::from_std(config.access_event_retention)
            .unwrap_or_else(|_| chrono::Duration::days(30));
    let pruned = prune_access_events(&pool, retention_cutoff).await?;

    // 2. Weekly compaction of cold and orphaned score rows.
    let mut compacted = 0;
    let due_for_compaction = match last_compaction_at(&pool).await? {
        None => true,
        Some(at) => chrono::Utc::now() - at > chrono::Duration::days(COMPACTION_INTERVAL_DAYS),
    };
    if due_for_compaction {
        let summary = compact_scores(&pool, config.half_life.as_secs_f64().max(1.0)).await?;
        compacted = summary.cold_rows_deleted + summary.orphan_rows_deleted;
    }

    // 3. Validation sweep, gated on the opt-in flag and the daily budget.
    let mut due_count = 0;
    let mut validations_run = 0;
    let mut budget_remaining = None;
    if config.validation_enabled {
        let day_ago = chrono::Utc::now() - chrono::Duration::days(1);
        let used = count_validation_runs_since(&pool, day_ago).await?;
        let remaining = (i64::from(config.daily_validation_cap) - used).max(0);
        budget_remaining = Some(remaining);
        let batch = i64::from(config.validation_batch_size).min(remaining.max(0));
        if batch > 0 {
            let candidates =
                fetch_due_candidates(&pool, None, &SelectionParams::from(config), batch).await?;
            due_count = candidates.len();
            for candidate in candidates {
                match validate_candidate(state, &pool, &candidate).await {
                    Ok(()) => validations_run += 1,
                    Err(error) => {
                        tracing::warn!(
                            canonical_id = %candidate.canonical_id,
                            error = %error,
                            "reinforcement validation failed"
                        );
                    }
                }
            }
        }
    }

    {
        let mut status = runtime
            .status
            .lock()
            .expect("reinforcement status mutex poisoned");
        status.status = "idle".to_string();
        status.last_finished_at = Some(chrono::Utc::now());
        status.pruned_access_events = pruned;
        status.compacted_rows = compacted;
        status.due_candidates = due_count;
        status.validations_run = validations_run;
        status.daily_budget_remaining = budget_remaining;
    }
    Ok(())
}

/// Runs the validation pipeline for one due candidate with the LLM-backed
/// verdict provider.
async fn validate_candidate(
    state: &AppState,
    pool: &PgPool,
    candidate: &ValidationCandidate,
) -> Result<()> {
    let policy = mem_reinforce::ValidationPolicy::from(&state.config.reinforcement);
    let provider = crate::reinforcement::ServiceVerdictProvider {
        state: state.clone(),
    };
    let outcome = mem_reinforce::run_validation(
        pool,
        candidate,
        &provider,
        &policy,
        mem_reinforce::ValidationTrigger::Scheduled,
    )
    .await?;
    tracing::info!(
        canonical_id = %candidate.canonical_id,
        run_id = %outcome.run_id,
        verdict = outcome.verdict.as_str(),
        action = outcome.action.as_str(outcome.dry_run),
        dry_run = outcome.dry_run,
        "memory validation completed"
    );
    Ok(())
}

/// Reports memories due for validation for one project (curation-time
/// check; read-only).
pub(crate) async fn due_validation_infos(
    state: &AppState,
    project_id: Uuid,
) -> Result<Vec<ValidationDueInfo>> {
    let config = &state.config.reinforcement;
    if !config.enabled || !config.validation_enabled {
        return Ok(Vec::new());
    }
    let Ok(pool) = state.pool() else {
        return Ok(Vec::new());
    };
    let candidates = fetch_due_candidates(
        &pool,
        Some(project_id),
        &SelectionParams::from(config),
        i64::from(config.validation_batch_size),
    )
    .await?;
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<Uuid> = candidates.iter().map(|c| c.memory_id).collect();
    let rows: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id, summary FROM memory_entries WHERE id = ANY($1)")
            .bind(&ids)
            .fetch_all(&pool)
            .await?;
    let summaries: HashMap<Uuid, String> = rows.into_iter().collect();
    Ok(candidates
        .into_iter()
        .map(|candidate| ValidationDueInfo {
            canonical_id: candidate.canonical_id,
            memory_id: candidate.memory_id,
            summary: summaries
                .get(&candidate.memory_id)
                .cloned()
                .unwrap_or_default(),
            activation: candidate.activation,
            volatility: candidate.volatility,
            validated_at: candidate.validated_at,
        })
        .collect())
}

/// Detects provenance status transitions and folds them into the
/// volatility EWMA of the owning memories. Sources verified for the first
/// time do not count as changes; only an actual status flip does.
pub(crate) async fn fold_provenance_volatility(
    pool: &PgPool,
    items: &[SourceProvenanceVerification],
    alpha: f64,
) -> Result<()> {
    if items.is_empty() {
        return Ok(());
    }
    let source_ids: Vec<Uuid> = items.iter().map(|item| item.source_id).collect();
    let previous_statuses: HashMap<Uuid, String> = sqlx::query_as::<_, (Uuid, String)>(
        "SELECT source_id, status FROM memory_source_verifications WHERE source_id = ANY($1)",
    )
    .bind(&source_ids)
    .fetch_all(pool)
    .await?
    .into_iter()
    .collect();
    let mut change_counts: HashMap<Uuid, u32> = HashMap::new();
    for item in items {
        if let Some(previous) = previous_statuses.get(&item.source_id)
            && previous != item.status.as_str()
        {
            *change_counts.entry(item.memory_id).or_default() += 1;
        }
    }
    if change_counts.is_empty() {
        return Ok(());
    }
    let memory_ids: Vec<Uuid> = change_counts.keys().copied().collect();
    let canonical_map = mem_reinforce::repository::resolve_canonicals(pool, &memory_ids).await?;
    let mut canonical_counts: HashMap<Uuid, u32> = HashMap::new();
    for (memory_id, count) in &change_counts {
        if let Some((canonical_id, _)) = canonical_map.get(memory_id) {
            *canonical_counts.entry(*canonical_id).or_default() += count;
        }
    }
    let shifts = mem_reinforce::repository::fold_volatility(pool, &canonical_counts, alpha).await?;
    audit_volatility_shifts(pool, &shifts).await?;
    Ok(())
}

/// Audits large volatility moves reported by a provenance sweep.
pub(crate) async fn audit_volatility_shifts(
    pool: &PgPool,
    shifts: &[mem_reinforce::repository::VolatilityShift],
) -> Result<()> {
    for shift in shifts {
        if (shift.new_volatility - shift.old_volatility).abs() > VOLATILITY_AUDIT_DELTA {
            insert_score_audit(
                pool,
                shift.canonical_id,
                shift.project_id,
                "volatility_shift",
                None,
                None,
                serde_json::json!({
                    "old_volatility": shift.old_volatility,
                    "new_volatility": shift.new_volatility,
                }),
            )
            .await?;
        }
    }
    Ok(())
}
