//! `memory structure` — the meta-memory structure view: the committed insight
//! tree (`summarizes` relations, recursive across tiers) plus the clusters the
//! deterministic consolidation scan discovers right now.

use anyhow::Result;
use mem_api::{ProjectStructureResponse, StructureInsightNode};
use serde::Serialize;

use crate::commands::{api::ApiClient, memory_ops::resolve_project_slug, runtime::StructureArgs};

pub(super) async fn handle(args: StructureArgs, api: &ApiClient) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let project = resolve_project_slug(args.project, &cwd)?;
    let response = api.project_structure(&project).await?;
    if args.json {
        print_json(&response)?;
        return Ok(());
    }
    print!("{}", render(&response));
    Ok(())
}

fn render(response: &ProjectStructureResponse) -> String {
    let mut out = String::new();
    let push = |out: &mut String, line: String| {
        out.push_str(&line);
        out.push('\n');
    };

    push(
        &mut out,
        format!("Memory structure for '{}'", response.project),
    );

    push(&mut out, String::new());
    if response.insights.is_empty() {
        push(
            &mut out,
            "Insight tree: none yet — approved consolidation proposals will appear here."
                .to_string(),
        );
    } else {
        push(
            &mut out,
            format!("Insight tree ({} root(s)):", response.insights.len()),
        );
        for node in &response.insights {
            render_node(&mut out, node, 0);
        }
    }

    push(&mut out, String::new());
    push(
        &mut out,
        format!(
            "Discovered groups: {} of {} candidate cluster(s) pass the value gate ({} rejected, {} already covered).",
            response.groups.len(),
            response.candidate_count,
            response.rejected_count,
            response.covered_count
        ),
    );
    for (index, group) in response.groups.iter().enumerate() {
        push(
            &mut out,
            format!(
                "\nGroup {} — {} member(s), trigger {}, density {:.2}, co-access {:.1}, activation {:.2}",
                index + 1,
                group.size,
                group.trigger,
                group.intra_density,
                group.coaccess_mass,
                group.activation_mass
            ),
        );
        for member in &group.members {
            push(
                &mut out,
                format!(
                    "  - [{}] {} {}",
                    member.memory_type,
                    member.canonical_id,
                    truncate(&member.summary, 80)
                ),
            );
        }
    }
    if !response.groups.is_empty() {
        push(
            &mut out,
            "\nRun memory consolidate to synthesize these groups into insight proposals."
                .to_string(),
        );
    }
    out
}

fn render_node(out: &mut String, node: &StructureInsightNode, depth: usize) {
    let indent = "  ".repeat(depth);
    let marker = if node.memory_type == "insight" {
        "◆"
    } else {
        "·"
    };
    out.push_str(&format!(
        "{indent}{marker} [{}] {} {}\n",
        node.memory_type,
        node.canonical_id,
        truncate(&node.summary, 80)
    ));
    for child in &node.children {
        render_node(out, child, depth + 1);
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
    use mem_api::{StructureGroupInfo, StructureMemberInfo};
    use uuid::Uuid;

    fn node(
        summary: &str,
        memory_type: &str,
        children: Vec<StructureInsightNode>,
    ) -> StructureInsightNode {
        StructureInsightNode {
            canonical_id: Uuid::nil(),
            summary: summary.to_string(),
            memory_type: memory_type.to_string(),
            children,
        }
    }

    #[test]
    fn renders_tiered_tree_and_groups() {
        let response = ProjectStructureResponse {
            project: "demo".to_string(),
            groups: vec![StructureGroupInfo {
                size: 2,
                trigger: "salient".to_string(),
                intra_density: 1.0,
                coaccess_mass: 4.0,
                activation_mass: 1.25,
                members: vec![StructureMemberInfo {
                    canonical_id: Uuid::nil(),
                    summary: "watcher restart policy".to_string(),
                    memory_type: "decision".to_string(),
                }],
            }],
            candidate_count: 3,
            rejected_count: 1,
            covered_count: 1,
            insights: vec![node(
                "tier-2 insight",
                "insight",
                vec![
                    node(
                        "tier-1 insight",
                        "insight",
                        vec![node("leaf fact", "reference", vec![])],
                    ),
                    node("another leaf", "decision", vec![]),
                ],
            )],
        };
        let text = render(&response);
        assert!(text.contains("Insight tree (1 root(s))"));
        // Tier nesting is expressed through indentation depth.
        assert!(text.contains("◆ [insight] 00000000-0000-0000-0000-000000000000 tier-2 insight"));
        assert!(text.contains("  ◆ [insight] 00000000-0000-0000-0000-000000000000 tier-1 insight"));
        assert!(text.contains("    · [reference] 00000000-0000-0000-0000-000000000000 leaf fact"));
        assert!(text.contains("1 of 3 candidate cluster(s)"));
        assert!(text.contains("[decision]"));
        assert!(text.contains("Run memory consolidate"));
    }

    #[test]
    fn renders_empty_state() {
        let response = ProjectStructureResponse {
            project: "demo".to_string(),
            groups: vec![],
            candidate_count: 0,
            rejected_count: 0,
            covered_count: 0,
            insights: vec![],
        };
        let text = render(&response);
        assert!(text.contains("none yet"));
        assert!(text.contains("0 of 0 candidate cluster(s)"));
        assert!(!text.contains("Run memory consolidate"));
    }
}
