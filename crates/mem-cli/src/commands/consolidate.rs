//! `memory consolidate` — first-class convenience over the
//! `memory_consolidation` loop: runs the deterministic cluster scan (and,
//! unless `--dry-run`, the LLM synthesis into human-gated insight proposals)
//! and renders the stored report.

use anyhow::Result;
use mem_api::{LoopRunDetail, LoopRunRequest};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::commands::{api::ApiClient, memory_ops::resolve_project_slug, runtime::ConsolidateArgs};

const LOOP_ID: &str = "memory_consolidation";

/// Client-side mirror of the service's consolidation report as stored in
/// `loop_runs.output_json.consolidation`. Unknown fields are ignored so older
/// or newer services keep working.
#[derive(Debug, Deserialize)]
struct ReportView {
    #[serde(default)]
    candidate_count: usize,
    #[serde(default)]
    accepted: Vec<ClusterView>,
    #[serde(default)]
    rejected_count: usize,
    #[serde(default)]
    covered_skipped: usize,
}

#[derive(Debug, Deserialize)]
struct ClusterView {
    #[serde(default)]
    size: usize,
    #[serde(default)]
    trigger: String,
    #[serde(default)]
    intra_density: f64,
    #[serde(default)]
    activation_mass: f64,
    #[serde(default)]
    members: Vec<MemberView>,
}

#[derive(Debug, Deserialize)]
struct MemberView {
    canonical_id: Uuid,
    #[serde(default)]
    summary: String,
}

pub(super) async fn handle(args: ConsolidateArgs, api: &ApiClient) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let project = resolve_project_slug(args.project, &cwd)?;
    let request = LoopRunRequest {
        project: Some(project.clone()),
        repo_root: None,
        scope_type: None,
        scope_id: None,
        dry_run: args.dry_run,
        reason: Some("memory consolidate".to_string()),
        trigger_payload: None,
    };
    request.validate().map_err(anyhow::Error::msg)?;
    // Synthesis makes two LLM calls per accepted cluster server-side, so this
    // request can far outlive the default client timeout.
    let timeout = (!args.dry_run).then(|| std::time::Duration::from_secs(300));
    let response = api
        .loop_run_with_timeout(LOOP_ID, &request, timeout)
        .await?;
    // Proposals are emitted after the run record the response was built from,
    // so re-fetch the detail to include them.
    let detail = if args.dry_run {
        response.run
    } else {
        api.loop_run_detail(response.run.summary.id).await?.run
    };

    if args.json {
        print_json(&detail)?;
        return Ok(());
    }
    render(&project, args.dry_run, &detail);
    Ok(())
}

fn render(project: &str, dry_run: bool, detail: &LoopRunDetail) {
    let mode = if dry_run { " (dry run)" } else { "" };
    println!(
        "Consolidation run for '{project}'{mode} — run {}",
        detail.summary.id
    );

    let Some(report) = detail
        .output
        .get("consolidation")
        .and_then(|value| serde_json::from_value::<ReportView>(value.clone()).ok())
    else {
        if let Some(summary) = &detail.summary.output_summary {
            println!("{summary}");
        } else {
            println!("No consolidation report was stored on the run.");
        }
        return;
    };

    println!(
        "Scanned {} candidate cluster(s): {} accepted, {} rejected, {} already covered by insights.",
        report.candidate_count,
        report.accepted.len(),
        report.rejected_count,
        report.covered_skipped
    );
    for (index, cluster) in report.accepted.iter().enumerate() {
        println!(
            "\nCluster {} — {} member(s), trigger {}, density {:.2}, activation {:.2}",
            index + 1,
            cluster.size,
            cluster.trigger,
            cluster.intra_density,
            cluster.activation_mass
        );
        for member in &cluster.members {
            println!(
                "  - {} {}",
                member.canonical_id,
                truncate(&member.summary, 90)
            );
        }
    }

    if dry_run {
        if !report.accepted.is_empty() {
            println!(
                "\nDry run: no insight proposals were queued. Re-run without --dry-run to synthesize them."
            );
        }
        return;
    }
    let pending = detail
        .memory_proposals
        .iter()
        .filter(|proposal| proposal.proposal_type == "consolidate")
        .count();
    if pending > 0 {
        println!(
            "\nQueued {pending} insight proposal(s) for review: memory loops memory-proposals --project {project} --status pending"
        );
    } else if !report.accepted.is_empty() {
        println!(
            "\nNo proposals were queued despite accepted clusters — consolidation may be disabled in the service config, or synthesis failed; check memory loops show memory_consolidation and the service log."
        );
    } else {
        println!("\nNothing to consolidate right now.");
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let cut: String = text.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_view_parses_stored_report_shape() {
        let stored = serde_json::json!({
            "project": "demo",
            "candidate_count": 3,
            "accepted": [{
                "size": 2,
                "trigger": "salient",
                "intra_density": 1.0,
                "coaccess_mass": 4.0,
                "activation_mass": 1.5,
                "members": [
                    {"canonical_id": "5b4f9d0e-4b7c-4b57-9dbb-111111111111",
                     "summary": "a", "canonical_text": "a", "activation": 0.5}
                ]
            }],
            "rejected_count": 1,
            "covered_skipped": 1
        });
        let view: ReportView = serde_json::from_value(stored).expect("parse");
        assert_eq!(view.candidate_count, 3);
        assert_eq!(view.accepted.len(), 1);
        assert_eq!(view.accepted[0].members[0].summary, "a");
    }

    #[test]
    fn truncate_respects_char_boundaries() {
        assert_eq!(truncate("héllo wörld", 5), "héll…");
        assert_eq!(truncate("short", 90), "short");
    }
}
