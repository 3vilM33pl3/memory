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

use std::sync::{Arc, Mutex};

use crate::state::{AppState, ReinforcementRuntimeState};

#[derive(Clone)]
pub(crate) struct ReinforcementRuntime {
    pub(crate) tx: mpsc::Sender<AccessBatch>,
    /// Wakes the background scheduler early (e.g. right after curation
    /// reports due candidates) instead of waiting for the next tick.
    pub(crate) notify: Arc<tokio::sync::Notify>,
    pub(crate) status: Arc<Mutex<ReinforcementRuntimeState>>,
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
    Some((
        ReinforcementRuntime {
            tx,
            notify: Arc::new(tokio::sync::Notify::new()),
            status: Arc::new(Mutex::new(ReinforcementRuntimeState {
                status: "idle".to_string(),
                ..ReinforcementRuntimeState::default()
            })),
        },
        rx,
    ))
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

/// LLM-backed verdict provider: builds a structured prompt from the
/// gathered context and parses the strict-JSON verdict. All calls flow
/// through the shared LLM helper and its audit trail.
pub(crate) struct ServiceVerdictProvider {
    pub(crate) state: AppState,
}

const VALIDATION_SYSTEM_PROMPT: &str = "You are auditing one stored project memory against the evidence supplied by the user. Decide whether the memory is still accurate. Return strict JSON with keys: verdict (one of valid, partially_valid, outdated, ambiguous, unsupported), confidence (0..1), reasons (array of short strings), evidence (array of {kind, ref, stance, excerpt} where kind is one of file, code_symbol, doc, commit, test, issue, memory, search_hit and stance is supports, contradicts or neutral), clarity_ok (boolean, false when the memory is correct but its wording could be clearer or easier to retrieve), proposed_summary (string, optional), proposed_text (string, optional). Rules: every evidence ref MUST be copied verbatim from a line marked 'citable:' in the supplied context; if nothing citable supports a point, omit the evidence entry and explain in reasons instead. Background sources without a citable ref must not be cited. Never invent files, commits or ids. Propose new wording only when it preserves the memory's meaning. If the memory is outdated, propose a corrected text when the evidence clearly supports one. If evidence is weak, missing, or contradictory, use verdict ambiguous or unsupported with low confidence rather than guessing.";

#[async_trait::async_trait]
impl mem_reinforce::VerdictProvider for ServiceVerdictProvider {
    async fn assess(
        &self,
        context: &mem_reinforce::ValidationContext,
    ) -> anyhow::Result<mem_reinforce::RawVerdict> {
        let subject = format!(
            "Validate memory: {}",
            context.memory.summary.chars().take(120).collect::<String>()
        );
        let outcome = crate::llm::call_llm_strict_json(
            &self.state,
            &crate::llm::LlmStrictJsonRequest {
                project: &context.memory.project_slug,
                purpose: "memory_validation",
                subject: subject.clone(),
                system_prompt: VALIDATION_SYSTEM_PROMPT,
                user_prompt: build_validation_prompt(context),
                max_output_tokens_cap: 1200,
            },
        )
        .await?;
        match mem_reinforce::validate::parse_verdict_content(&outcome.content) {
            Ok(raw) => Ok(raw),
            Err(error) => {
                crate::repository::events::emit_llm_audit_activity(
                    &self.state,
                    &context.memory.project_slug,
                    "memory_validation",
                    subject,
                    &outcome.request_body,
                    "error",
                    Some(&error.to_string()),
                    Some(outcome.started.elapsed().as_millis() as u64),
                    outcome.token_usage,
                );
                Err(error)
            }
        }
    }

    fn model_name(&self) -> Option<String> {
        Some(self.state.config.llm.model.clone())
    }
}

/// Renders the deterministic evidence bundle for the verdict prompt.
pub(crate) fn build_validation_prompt(context: &mem_reinforce::ValidationContext) -> String {
    let memory = &context.memory;
    let mut lines = vec![
        format!("Project: {}", memory.project_slug),
        format!(
            "Memory under validation (id {}, type {}, importance {}, stored confidence {:.2}):",
            memory.memory_id, memory.memory_type, memory.importance, memory.confidence
        ),
        format!("Summary: {}", memory.summary),
        format!("Text: {}", memory.canonical_text),
        format!(
            "Created {}, last updated {}.",
            memory.created_at.format("%Y-%m-%d"),
            memory.updated_at.format("%Y-%m-%d")
        ),
    ];
    if !context.tags.is_empty() {
        lines.push(format!("Tags: {}", context.tags.join(", ")));
    }
    lines.push(String::new());
    lines.push(format!("citable: {} (this memory's id)", memory.memory_id));
    if context.sources.is_empty() {
        lines.push("Recorded sources: none.".to_string());
    } else {
        lines.push("Recorded sources:".to_string());
        for source in &context.sources {
            let mut parts = Vec::new();
            match &source.file_path {
                Some(path) => {
                    let reference = match &source.symbol_name {
                        Some(symbol) => format!("{path}#{symbol}"),
                        None => path.clone(),
                    };
                    parts.push(format!("citable: {reference}"));
                }
                // Path-less sources (task prompts, notes) are background
                // only: there is nothing in the allowlist to cite.
                None => parts.push("background (no citable ref)".to_string()),
            }
            parts.push(format!("kind={}", source.source_kind));
            if let Some(status) = &source.provenance_status {
                parts.push(format!("provenance={status}"));
            }
            if let Some(commit) = &source.git_commit {
                parts.push(format!("citable commit: {commit}"));
            }
            lines.push(format!("- {}", parts.join(" ")));
            if let Some(excerpt) = &source.excerpt {
                lines.push(format!(
                    "  excerpt: {}",
                    excerpt.chars().take(300).collect::<String>()
                ));
            }
        }
    }
    if !context.related.is_empty() {
        lines.push(String::new());
        lines.push("Related memories:".to_string());
        for related in &context.related {
            lines.push(format!(
                "- citable: {} [{}] {}",
                related.memory_id, related.relation_type, related.summary
            ));
        }
    }
    if !context.git_log.is_empty() {
        lines.push(String::new());
        lines.push("Commits touching the source paths since last validation:".to_string());
        for line in &context.git_log {
            match line.split_once(' ') {
                Some((sha, rest)) => lines.push(format!("- citable: {sha} ({rest})")),
                None => lines.push(format!("- citable: {line}")),
            }
        }
    }
    if !context.prior_runs.is_empty() {
        lines.push(String::new());
        lines.push("Previous validation runs:".to_string());
        for run in &context.prior_runs {
            lines.push(format!(
                "- verdict={} confidence={} action={} at {}",
                run.verdict.as_deref().unwrap_or("?"),
                run.confidence
                    .map(|value| format!("{value:.2}"))
                    .unwrap_or_else(|| "?".to_string()),
                run.action.as_deref().unwrap_or("?"),
                run.finished_at
                    .map(|at| at.format("%Y-%m-%d").to_string())
                    .unwrap_or_else(|| "?".to_string()),
            ));
        }
    }
    lines.push(String::new());
    lines.push("Assess the memory now. Return JSON only.".to_string());
    lines.join("\n")
}
