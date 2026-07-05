//! Batch access recording: resolves accessed memory versions to canonical
//! memories, applies decay-then-boost score updates, spreads activation to
//! linked memories, logs compact access events, and reports threshold
//! crossings. Runs off the query hot path (the service feeds it through a
//! bounded channel).

use std::collections::HashMap;

use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use crate::propagation::propagation_increments;
use crate::repository::{
    AccessEventRow, ScoreCounters, ScoreUpdate, apply_score_boost, fetch_canonical_edges,
    insert_access_events, insert_score_audit, resolve_canonicals, resolve_project_ids,
};
use crate::scoring::{AccessKind, ScoreParams};

/// A batch of accesses observed by one operation (typically one query).
#[derive(Debug, Clone, Default)]
pub struct AccessBatch {
    pub operation_id: Option<String>,
    /// Accessed memory version ids with the strongest signal observed for
    /// each (a citation subsumes a retrieval of the same memory).
    pub events: Vec<(Uuid, AccessKind)>,
}

/// A memory whose activation crossed the validation threshold during a
/// batch. The caller decides what to do with it (audit + timeline event);
/// validation itself is always scheduled separately.
#[derive(Debug, Clone, Copy)]
pub struct ThresholdCrossing {
    pub canonical_id: Uuid,
    pub project_id: Uuid,
    pub activation: f64,
}

/// Applies one access batch. Returns threshold crossings (already recorded
/// in the score audit).
pub async fn record_access_batch(
    pool: &PgPool,
    batch: &AccessBatch,
    params: &ScoreParams,
    validation_threshold: f64,
) -> Result<Vec<ThresholdCrossing>> {
    if batch.events.is_empty() {
        return Ok(Vec::new());
    }

    let memory_ids: Vec<Uuid> = batch.events.iter().map(|(id, _)| *id).collect();
    let canonicals = resolve_canonicals(pool, &memory_ids).await?;

    // Strongest signal per canonical: a memory can appear as both a plain
    // retrieval and a citation in one batch (or via several versions).
    let mut direct: HashMap<Uuid, (Uuid, AccessKind)> = HashMap::new();
    for (memory_id, kind) in &batch.events {
        let Some(&(canonical_id, project_id)) = canonicals.get(memory_id) else {
            continue; // memory pruned between retrieval and recording
        };
        direct
            .entry(canonical_id)
            .and_modify(|(_, existing)| {
                if kind.boost(params) > existing.boost(params) {
                    *existing = *kind;
                }
            })
            .or_insert((project_id, *kind));
    }
    if direct.is_empty() {
        return Ok(Vec::new());
    }

    let seeds: Vec<Uuid> = direct.keys().copied().collect();
    let edges = fetch_canonical_edges(pool, &seeds, params.max_hops).await?;

    // Propagated increments from every seed; a directly accessed memory
    // never also receives a propagated increment in the same batch, and
    // multiple seeds propagating to the same neighbour keep the largest
    // increment (consistent with the per-seed multi-path rule).
    let mut propagated: HashMap<Uuid, (f64, u8)> = HashMap::new();
    for (&canonical_id, &(_, kind)) in &direct {
        for inc in propagation_increments(canonical_id, kind.boost(params), &edges, params) {
            if direct.contains_key(&inc.canonical_id) {
                continue;
            }
            propagated
                .entry(inc.canonical_id)
                .and_modify(|(existing, hop)| {
                    if inc.increment > *existing {
                        *existing = inc.increment;
                        *hop = inc.hop_distance;
                    }
                })
                .or_insert((inc.increment, inc.hop_distance));
        }
    }

    // Propagated targets may have no score row yet and are not in
    // `direct`, so resolve their project ids from memory_entries.
    let propagated_ids: Vec<Uuid> = propagated.keys().copied().collect();
    let propagated_projects = resolve_project_ids(pool, &propagated_ids).await?;

    let mut updates: Vec<(ScoreUpdate, AccessEventRow)> = Vec::new();
    for (&canonical_id, &(project_id, kind)) in &direct {
        let boost = kind.boost(params);
        let counters = ScoreCounters {
            access: 1,
            citation: i64::from(kind == AccessKind::Citation),
            propagated: 0,
        };
        let update =
            apply_score_boost(pool, canonical_id, project_id, boost, counters, params).await?;
        updates.push((
            update,
            AccessEventRow {
                canonical_id,
                project_id,
                kind: kind.as_str(),
                boost,
                hop_distance: 0,
                operation_id: batch.operation_id.clone(),
            },
        ));
    }
    for (&canonical_id, &(increment, hop)) in &propagated {
        let Some(&project_id) = propagated_projects.get(&canonical_id) else {
            continue;
        };
        let counters = ScoreCounters {
            access: 0,
            citation: 0,
            propagated: 1,
        };
        let update =
            apply_score_boost(pool, canonical_id, project_id, increment, counters, params).await?;
        updates.push((
            update,
            AccessEventRow {
                canonical_id,
                project_id,
                kind: "propagated",
                boost: increment,
                hop_distance: i16::from(hop),
                operation_id: batch.operation_id.clone(),
            },
        ));
    }

    let event_rows: Vec<AccessEventRow> = updates.iter().map(|(_, row)| row.clone()).collect();
    insert_access_events(pool, &event_rows).await?;

    let mut crossings = Vec::new();
    for (update, _) in &updates {
        if update.crossed(validation_threshold) {
            insert_score_audit(
                pool,
                update.canonical_id,
                update.project_id,
                "threshold_crossed",
                Some(update.old_activation),
                Some(update.new_activation),
                serde_json::json!({
                    "threshold": validation_threshold,
                    "operation_id": batch.operation_id,
                }),
            )
            .await?;
            crossings.push(ThresholdCrossing {
                canonical_id: update.canonical_id,
                project_id: update.project_id,
                activation: update.new_activation,
            });
        }
    }
    Ok(crossings)
}
