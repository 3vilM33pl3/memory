//! Service-side glue for memory reinforcement: the bounded access channel,
//! the background worker draining it, and the handler-level hooks that feed
//! it. Hooks are fire-and-forget (`try_send`, dropping on overflow) so the
//! query hot path never waits on scoring.
//!
//! Only the handler hooks in this module enqueue accesses. Validation,
//! curation, provenance verification, and search-internal reads must never
//! count as accesses, or scoring would feed back on itself.

use std::collections::HashMap;

use mem_api::QueryResponse;
use mem_reinforce::{AccessBatch, AccessKind, ScoreParams};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::state::AppState;

#[derive(Clone)]
pub(crate) struct ReinforcementRuntime {
    pub(crate) tx: mpsc::Sender<AccessBatch>,
}

/// Builds the channel pair when reinforcement is enabled. The receiver is
/// handed to [`spawn_access_worker`] once the full `AppState` exists.
pub(crate) fn build_runtime(
    config: &mem_api::ReinforcementConfig,
) -> Option<(ReinforcementRuntime, mpsc::Receiver<AccessBatch>)> {
    if !config.enabled {
        return None;
    }
    let (tx, rx) = mpsc::channel(config.access_channel_capacity.max(1));
    Some((ReinforcementRuntime { tx }, rx))
}

pub(crate) fn spawn_access_worker(state: AppState, mut rx: mpsc::Receiver<AccessBatch>) {
    tokio::spawn(async move {
        let params = ScoreParams::from(&state.config.reinforcement);
        let threshold = state.config.reinforcement.validation_threshold;
        while let Some(batch) = rx.recv().await {
            // Pool may be temporarily gone (offline degraded mode); scoring
            // is advisory, so dropping the batch is the correct behaviour.
            let Ok(pool) = state.pool() else {
                continue;
            };
            match mem_reinforce::record_access_batch(&pool, &batch, &params, threshold).await {
                Ok(crossings) => {
                    for crossing in crossings {
                        tracing::info!(
                            canonical_id = %crossing.canonical_id,
                            activation = crossing.activation,
                            "memory crossed validation threshold"
                        );
                    }
                }
                Err(error) => {
                    tracing::warn!(error = %error, "record reinforcement access batch");
                }
            }
        }
    });
}

/// Records the memories a query returned (retrieval) and the subset the
/// synthesized answer actually cited (citation, which subsumes retrieval).
pub(crate) fn record_query_access(state: &AppState, response: &QueryResponse) {
    let Some(runtime) = &state.reinforcement else {
        return;
    };
    if let Some(batch) = batch_from_query_response(response) {
        send_batch(runtime, batch);
    }
}

/// Builds the access batch for one query response: every result counts as a
/// retrieval, upgraded to a citation when the answer cited it.
pub(crate) fn batch_from_query_response(response: &QueryResponse) -> Option<AccessBatch> {
    let mut kinds: HashMap<Uuid, AccessKind> = response
        .results
        .iter()
        .map(|result| (result.memory_id, AccessKind::Retrieval))
        .collect();
    for citation in &response.answer_citations {
        kinds.insert(citation.memory_id, AccessKind::Citation);
    }
    if kinds.is_empty() {
        return None;
    }
    Some(AccessBatch {
        operation_id: Some("query".to_string()),
        events: kinds.into_iter().collect(),
    })
}

/// Records a direct single-memory read (get/resume). List endpoints and
/// browsing surfaces deliberately do not record.
pub(crate) fn record_direct_read(state: &AppState, memory_id: Uuid) {
    let Some(runtime) = &state.reinforcement else {
        return;
    };
    send_batch(
        runtime,
        AccessBatch {
            operation_id: Some("direct_read".to_string()),
            events: vec![(memory_id, AccessKind::DirectRead)],
        },
    );
}

fn send_batch(runtime: &ReinforcementRuntime, batch: AccessBatch) {
    if let Err(mpsc::error::TrySendError::Full(_)) = runtime.tx.try_send(batch) {
        tracing::debug!("reinforcement access channel full; dropping batch");
    }
}
