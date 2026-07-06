//! Memory consolidation orchestration that needs an `AppState`: LLM synthesis
//! of an `insight` meta-memory from a cluster, and emission of a human-gated
//! `consolidate` proposal. The deterministic clustering itself lives in
//! `repository/handlers/consolidation.rs` (pool-only); this module turns an
//! accepted cluster into a proposal via the shared strict-JSON LLM helper.

use anyhow::Result;
use mem_api::LoopMemoryProposalCreateRequest;
use serde::Deserialize;
use uuid::Uuid;

use crate::llm::{LlmStrictJsonRequest, call_llm_strict_json};
use crate::repository::events::emit_llm_audit_activity;
use crate::repository::handlers::consolidation::{
    AcceptedCluster, ClusterMember, run_memory_consolidation,
};
use crate::state::AppState;

const THEMES_SYSTEM_PROMPT: &str = "You are analyzing a cluster of related project memories to find what unifies them. Return strict JSON: {\"themes\": [string]} with 1-3 short high-level themes or questions the cluster collectively answers. No prose outside the JSON.";

const SYNTHESIS_SYSTEM_PROMPT: &str = "You are consolidating a cluster of related project memories into one higher-level insight memory. You are shown each member with an explicit 'id:'. Return strict JSON with keys: theme (string), meta_summary (string, one line), meta_text (string, the consolidated insight; state the unifying concept, then note internal tensions/contradictions, gaps or open questions, and concrete implications for design or refactoring), tensions (array of short strings), gaps (array of short strings), refactors (array of short strings), cited_member_ids (array of the member id strings this insight is grounded in). Rules: only cite ids shown to you; never invent ids; ground every claim in the supplied members; if the members do not cohere, say so in meta_text and cite the ones that do.";

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ConsolidationSynthesis {
    pub theme: String,
    pub meta_summary: String,
    pub meta_text: String,
    #[serde(default)]
    pub tensions: Vec<String>,
    #[serde(default)]
    pub gaps: Vec<String>,
    #[serde(default)]
    pub refactors: Vec<String>,
    #[serde(default)]
    pub cited_member_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Deserialize)]
struct ThemesResponse {
    #[serde(default)]
    themes: Vec<String>,
}

/// Two-step synthesis (extract themes, then synthesize) with an
/// anti-hallucination guard: the returned `cited_member_ids` are intersected
/// with the ids actually shown, and a synthesis that cites none of them is
/// rejected.
pub(crate) async fn synthesize_consolidation(
    state: &AppState,
    project: &str,
    members: &[ClusterMember],
) -> Result<ConsolidationSynthesis> {
    let shown: Vec<Uuid> = members.iter().map(|m| m.canonical_id).collect();
    let member_block = members
        .iter()
        .map(|m| {
            format!(
                "id: {}\n  type-summary: {}\n  text: {}",
                m.canonical_id,
                m.summary,
                m.canonical_text.chars().take(600).collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    let themes = call_llm_strict_json(
        state,
        &LlmStrictJsonRequest {
            project,
            purpose: "consolidation_themes",
            subject: format!("Consolidation themes for {} memories", members.len()),
            system_prompt: THEMES_SYSTEM_PROMPT,
            user_prompt: format!("Cluster members:\n\n{member_block}\n\nReturn JSON only."),
            max_output_tokens_cap: 400,
        },
    )
    .await?;
    let themes: ThemesResponse =
        serde_json::from_str(&themes.content).unwrap_or(ThemesResponse { themes: Vec::new() });
    let theme_hint = if themes.themes.is_empty() {
        String::new()
    } else {
        format!("Candidate themes: {}\n\n", themes.themes.join("; "))
    };

    let subject = format!("Consolidate {} memories", members.len());
    let outcome = call_llm_strict_json(
        state,
        &LlmStrictJsonRequest {
            project,
            purpose: "consolidation_synthesis",
            subject: subject.clone(),
            system_prompt: SYNTHESIS_SYSTEM_PROMPT,
            user_prompt: format!(
                "{theme_hint}Cluster members:\n\n{member_block}\n\nReturn JSON only."
            ),
            max_output_tokens_cap: state.config.consolidation.max_output_tokens_cap,
        },
    )
    .await?;

    let mut synthesis: ConsolidationSynthesis = match serde_json::from_str(&outcome.content) {
        Ok(value) => value,
        Err(error) => {
            emit_llm_audit_activity(
                state,
                project,
                "consolidation_synthesis",
                subject,
                &outcome.request_body,
                "error",
                Some(&error.to_string()),
                Some(outcome.started.elapsed().as_millis() as u64),
                outcome.token_usage,
            );
            return Err(error.into());
        }
    };

    // Anti-hallucination: keep only cited ids that were actually shown.
    synthesis.cited_member_ids.retain(|id| shown.contains(id));
    if synthesis.cited_member_ids.is_empty() {
        anyhow::bail!("consolidation synthesis cited no shown member ids");
    }
    if synthesis.meta_text.trim().is_empty() || synthesis.meta_summary.trim().is_empty() {
        anyhow::bail!("consolidation synthesis returned empty meta memory");
    }
    Ok(synthesis)
}

/// Runs the real-config clustering scan and emits one human-gated
/// `consolidate` proposal per accepted, novel cluster (bounded by the daily
/// cap). Returns the number of proposals queued. Called from the manual loop
/// entry point and the auto-trigger worker, both of which hold an `AppState`.
pub(crate) async fn emit_consolidation_proposals(
    state: &AppState,
    project: &str,
    run_id: Option<Uuid>,
) -> Result<usize> {
    let cfg = &state.config.consolidation;
    if !cfg.enabled {
        return Ok(0);
    }
    let pool = state
        .pool()
        .map_err(|error| anyhow::anyhow!(error.message))?;
    let half_life_secs = state.config.reinforcement.half_life.as_secs_f64().max(1.0);
    let report = run_memory_consolidation(&pool, project, cfg, half_life_secs)
        .await
        .map_err(|error| anyhow::anyhow!(error.message))?;

    let mut queued = 0usize;
    for cluster in report.accepted.iter().take(cfg.daily_cap as usize) {
        match synthesize_and_queue(state, &pool, project, run_id, cluster).await {
            Ok(true) => queued += 1,
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(error = %error, "consolidation synthesis failed for a cluster");
            }
        }
    }
    Ok(queued)
}

async fn synthesize_and_queue(
    state: &AppState,
    pool: &sqlx::PgPool,
    project: &str,
    run_id: Option<Uuid>,
    cluster: &AcceptedCluster,
) -> Result<bool> {
    let synthesis = synthesize_consolidation(state, project, &cluster.members).await?;

    // Tensions, gaps, and refactors are folded into the stored text so they
    // surface on retrieval, and duplicated in tags for filtering.
    let mut sections = vec![synthesis.meta_text.clone()];
    if !synthesis.tensions.is_empty() {
        sections.push(format!("Tensions: {}", synthesis.tensions.join("; ")));
    }
    if !synthesis.gaps.is_empty() {
        sections.push(format!("Gaps: {}", synthesis.gaps.join("; ")));
    }
    if !synthesis.refactors.is_empty() {
        sections.push(format!("Implications: {}", synthesis.refactors.join("; ")));
    }
    let meta_text = sections.join("\n\n");

    let member_ids: Vec<String> = synthesis
        .cited_member_ids
        .iter()
        .map(Uuid::to_string)
        .collect();
    let evidence: Vec<serde_json::Value> = synthesis
        .cited_member_ids
        .iter()
        .map(|id| serde_json::json!({ "source_kind": "memory", "excerpt": id.to_string() }))
        .collect();

    let request = LoopMemoryProposalCreateRequest {
        project: project.to_string(),
        loop_id: mem_loops::LOOP_MEMORY_CONSOLIDATION.to_string(),
        proposal_type: "consolidate".to_string(),
        run_id,
        target_memory_id: None,
        candidate: serde_json::json!({
            "canonical_text": meta_text,
            "summary": synthesis.meta_summary,
            "memory_type": "insight",
            "scope": "project",
            "importance": 4,
            "confidence": 0.7,
            "tags": ["insight", "consolidation", cluster.trigger.clone()],
            "member_canonical_ids": member_ids,
            "theme": synthesis.theme,
        }),
        evidence: serde_json::Value::Array(evidence),
        confidence: 0.7,
        risk_notes: Some(format!(
            "Synthesized insight over {} memories ({} trigger); review the summary and member links before approving.",
            cluster.members.len(),
            cluster.trigger
        )),
    };
    crate::repository::handlers::loops::create_memory_proposal_record(pool, &request).await?;
    Ok(true)
}
