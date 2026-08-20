// SPDX-License-Identifier: AGPL-3.0-or-later

// SPDX-License-Identifier: AGPL-3.0-or-later

use chrono::{DateTime, Utc};
use mem_record::{
    ActivityDetails, ActivityEvent, ActivityKind, MemoryType, PlanActivityAction, WatcherHealth,
};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Row},
};

#[allow(unused_imports)]
use super::*;
use crate::tui::{app::*, theme::Theme};

pub(in crate::tui) fn activity_briefing_lines(app: &App) -> Vec<Line<'static>> {
    if app.activity.up_to_speed_loading {
        return vec![Line::from(Span::styled(
            "Generating get-up-to-speed briefing...",
            Style::default().fg(Theme::ACCENT_STRONG),
        ))];
    }
    if let Some(error) = &app.activity.up_to_speed_error {
        return vec![Line::from(Span::styled(
            format!("Briefing failed: {error}"),
            Style::default().fg(Theme::DANGER),
        ))];
    }
    if let Some(response) = &app.activity.up_to_speed_response {
        let mut lines = vec![Line::from(Span::styled(
            response
                .briefing
                .lines()
                .next()
                .unwrap_or("Get-up-to-speed briefing")
                .to_string(),
            Style::default().fg(Theme::TEXT),
        ))];
        if !response.next_actions.is_empty() {
            lines.push(Line::from(vec![
                label_span("Next: "),
                Span::styled(
                    response.next_actions[0].title.clone(),
                    Style::default().fg(Theme::ACCENT_STRONG),
                ),
            ]));
        }
        lines.push(Line::from(vec![
            label_span("Support: "),
            Span::styled(
                format!(
                    "{} activities / {} useful memories / {} token-tracked actions",
                    response.recent_activities.len(),
                    response.useful_memories.len(),
                    response.token_usage.action_count
                ),
                Style::default().fg(Theme::TEXT),
            ),
        ]));
        return lines;
    }
    vec![
        Line::from(Span::styled(
            "Press g for a deterministic briefing, or L for an LLM-assisted briefing.",
            Style::default().fg(Theme::TEXT),
        )),
        Line::from(Span::styled(
            "The briefing uses persisted activities, recent memory changes, commits, warnings, and token counts.",
            Style::default().fg(Theme::MUTED),
        )),
    ]
}

pub(in crate::tui) fn recent_activity_lines(app: &App) -> Vec<Line<'static>> {
    if app.activity.activity_events.is_empty() {
        return vec![Line::from(Span::styled(
            "No recent activity in this TUI session.",
            Style::default().fg(Theme::MUTED),
        ))];
    }

    app.activity
        .activity_events
        .iter()
        .take(6)
        .map(|event| {
            Line::from(vec![
                Span::styled(
                    format_timestamp_short(activity_recorded_at(event)),
                    Style::default().fg(Theme::MUTED),
                ),
                Span::raw(" "),
                activity_entry_kind_span(event),
                Span::raw(" "),
                Span::styled(activity_summary(event), Style::default().fg(Theme::TEXT)),
            ])
        })
        .collect()
}

pub(in crate::tui) fn latest_plan_display(app: &App) -> String {
    app.memories
        .all_memories
        .iter()
        .filter(|item| item.memory_type == MemoryType::Plan)
        .max_by(|left, right| {
            left.updated_at
                .cmp(&right.updated_at)
                .then_with(|| left.id.cmp(&right.id))
        })
        .map(|item| {
            let thread = item
                .tags
                .iter()
                .find_map(|tag| tag.strip_prefix("plan-thread:"));
            match thread {
                Some(thread) => format!("{} ({thread})", item.summary),
                None => item.summary.clone(),
            }
        })
        .unwrap_or_else(|| "none".to_string())
}

pub(in crate::tui) fn activity_row(item: &ActivityEntry) -> Row<'static> {
    Row::new(vec![
        Cell::from(Span::styled(
            format_timestamp_short(activity_recorded_at(item)),
            Style::default().fg(Theme::TEXT),
        )),
        Cell::from(activity_entry_kind_span(item)),
        Cell::from(Span::styled(
            activity_tokens(item),
            Style::default().fg(Theme::ACCENT_STRONG),
        )),
        Cell::from(Span::styled(
            activity_duration(item),
            Style::default().fg(Theme::MUTED),
        )),
        Cell::from(Span::styled(
            activity_summary(item),
            Style::default().fg(Theme::TEXT),
        )),
    ])
}

pub(in crate::tui) fn activity_detail_lines(entry: &ActivityEntry) -> Vec<Line<'static>> {
    match entry {
        ActivityEntry::Backend(event) => backend_activity_detail_lines(event),
        ActivityEntry::Query(entry) => {
            let mut lines = vec![
                Line::from(vec![
                    label_span("When: "),
                    Span::styled(
                        format_timestamp_full(entry.recorded_at),
                        Style::default().fg(Theme::TEXT),
                    ),
                ]),
                Line::from(vec![
                    label_span("Project: "),
                    Span::styled(entry.project.clone(), Style::default().fg(Theme::TEXT)),
                ]),
                Line::from(vec![
                    label_span("Kind: "),
                    activity_entry_kind_span(&ActivityEntry::Query(QueryActivityEntry {
                        recorded_at: entry.recorded_at,
                        project: entry.project.clone(),
                        request: entry.request.clone(),
                        duration_ms: entry.duration_ms,
                        outcome: entry.outcome.clone(),
                    })),
                ]),
                Line::from(vec![
                    label_span("Duration: "),
                    Span::styled(
                        format!("{} ms", entry.duration_ms),
                        Style::default().fg(Theme::TEXT),
                    ),
                    Span::raw("   "),
                    label_span("Top K: "),
                    Span::styled(
                        entry.request.top_k.to_string(),
                        Style::default().fg(Theme::TEXT),
                    ),
                    Span::raw("   "),
                    label_span("Min confidence: "),
                    Span::styled(
                        entry
                            .request
                            .min_confidence
                            .map(|value| format!("{value:.2}"))
                            .unwrap_or_else(|| "none".to_string()),
                        Style::default().fg(Theme::TEXT),
                    ),
                ]),
                Line::from(vec![
                    label_span("Filters: "),
                    Span::styled(
                        format_query_filters(&entry.request.filters),
                        Style::default().fg(Theme::TEXT),
                    ),
                ]),
                Line::from(vec![
                    label_span("Roundtrip: "),
                    Span::styled(
                        format!("{} ms", entry.duration_ms),
                        Style::default().fg(Theme::TEXT),
                    ),
                ]),
                Line::from(""),
                Line::from(vec![section_span("Question")]),
                Line::from(Span::styled(
                    entry.request.query.clone(),
                    Style::default().fg(Theme::TEXT),
                )),
                Line::from(""),
            ];

            match &entry.outcome {
                QueryLogOutcome::Success(response) => {
                    lines.push(Line::from(vec![section_span("Answer")]));
                    lines.push(Line::from(Span::styled(
                        response.answer.clone(),
                        Style::default().fg(Theme::TEXT),
                    )));
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
                        label_span("Confidence: "),
                        Span::styled(
                            format!("{:.2}", response.confidence),
                            confidence_style(response.confidence),
                        ),
                        Span::raw("   "),
                        label_span("Evidence: "),
                        Span::styled(
                            if response.insufficient_evidence {
                                "insufficient"
                            } else {
                                "sufficient"
                            },
                            if response.insufficient_evidence {
                                Style::default().fg(Theme::WARNING)
                            } else {
                                Style::default().fg(Theme::SUCCESS)
                            },
                        ),
                        Span::raw("   "),
                        label_span("Results: "),
                        Span::styled(
                            response.results.len().to_string(),
                            Style::default().fg(Theme::TEXT),
                        ),
                    ]));
                    lines.push(Line::from(vec![
                        label_span("Server timings: "),
                        Span::styled(
                            format!(
                                "lexical {} ms | semantic {} ms | graph {} ms | rerank {} ms | total {} ms",
                                response.diagnostics.lexical_duration_ms,
                                response.diagnostics.semantic_duration_ms,
                                response.diagnostics.graph_duration_ms,
                                response.diagnostics.rerank_duration_ms,
                                response.diagnostics.total_duration_ms
                            ),
                            Style::default().fg(Theme::TEXT),
                        ),
                    ]));
                    lines.push(Line::from(vec![
                        label_span("Candidate counts: "),
                        Span::styled(
                            format!(
                                "lexical {} | semantic {} | graph {} [{}] | merged {} | returned {} | relation {} | graph augmented {}",
                                response.diagnostics.lexical_candidates,
                                response.diagnostics.semantic_candidates,
                                response.diagnostics.graph_candidates,
                                response.diagnostics.graph_status,
                                response.diagnostics.merged_candidates,
                                response.diagnostics.returned_results,
                                response.diagnostics.relation_augmented_candidates,
                                response.diagnostics.graph_augmented_candidates
                            ),
                            Style::default().fg(Theme::TEXT),
                        ),
                    ]));
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![section_span("Returned Memories")]));
                    if response.results.is_empty() {
                        lines.push(Line::from(Span::styled(
                            "No memories returned.",
                            Style::default().fg(Theme::MUTED),
                        )));
                    } else {
                        for result in &response.results {
                            lines.push(Line::from(Span::styled(
                                format!(
                                    "{} | {} [{} / {}] score={:.2}",
                                    result.memory_id,
                                    result.summary,
                                    result.memory_type,
                                    result.match_kind,
                                    result.score
                                ),
                                Style::default().fg(Theme::TEXT),
                            )));
                            lines.push(Line::from(Span::styled(
                                format!("  snippet: {}", result.snippet),
                                Style::default().fg(Theme::MUTED),
                            )));
                            lines.push(Line::from(Span::styled(
                                format!(
                                    "  debug: chunk {:.2} | entry {:.2} | semantic {:.2} | relation {:.2} | graph {:.2}",
                                    result.debug.chunk_fts,
                                    result.debug.entry_fts,
                                    result.debug.semantic_similarity,
                                    result.debug.relation_boost,
                                    result.debug.graph_boost
                                ),
                                Style::default().fg(Theme::MUTED),
                            )));
                            if !result.score_explanation.is_empty() {
                                lines.push(Line::from(Span::styled(
                                    format!("  why: {}", result.score_explanation.join(" | ")),
                                    Style::default().fg(Theme::ACCENT),
                                )));
                            }
                            if !result.graph_connections.is_empty() {
                                let graph = result
                                    .graph_connections
                                    .iter()
                                    .take(2)
                                    .map(|connection| {
                                        format!(
                                            "{} {} boost={:.2}",
                                            connection.reason,
                                            connection.file_path,
                                            connection.score_boost
                                        )
                                    })
                                    .collect::<Vec<_>>()
                                    .join(" | ");
                                lines.push(Line::from(Span::styled(
                                    format!("  graph: {graph}"),
                                    Style::default().fg(Theme::ACCENT),
                                )));
                            }
                            if !result.tags.is_empty() {
                                lines.push(Line::from(Span::styled(
                                    format!("  tags: {}", result.tags.join(", ")),
                                    Style::default().fg(Theme::MUTED),
                                )));
                            }
                        }
                    }
                }
                QueryLogOutcome::Error(error) => {
                    lines.push(Line::from(vec![section_span("Error")]));
                    lines.push(Line::from(Span::styled(
                        error.clone(),
                        Style::default().fg(Theme::DANGER),
                    )));
                }
            }

            lines
        }
    }
}

pub(in crate::tui) fn backend_activity_detail_lines(event: &ActivityEvent) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(vec![
            label_span("When: "),
            Span::styled(
                format_timestamp_full(event.recorded_at),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
        Line::from(vec![
            label_span("Project: "),
            Span::styled(event.project.clone(), Style::default().fg(Theme::TEXT)),
        ]),
        Line::from(vec![label_span("Kind: "), activity_kind_span(&event.kind)]),
        Line::from(vec![
            label_span("Memory Id: "),
            Span::styled(
                event
                    .memory_id
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "n/a".to_string()),
                Style::default().fg(Theme::MUTED),
            ),
        ]),
        activity_kv_line(
            "Duration",
            activity_duration(&ActivityEntry::Backend(Box::new(event.clone()))),
        ),
        activity_kv_line(
            "Tokens",
            activity_tokens(&ActivityEntry::Backend(Box::new(event.clone()))),
        ),
        activity_kv_line(
            "Source",
            event.source.clone().unwrap_or_else(|| "n/a".to_string()),
        ),
        Line::from(""),
        Line::from(vec![section_span("Summary")]),
        Line::from(Span::styled(
            event.summary.clone(),
            Style::default().fg(Theme::TEXT),
        )),
    ];

    if let Some(details) = &event.details {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![section_span("Operation Detail")]));
        match details {
            ActivityDetails::Plan {
                action,
                title,
                thread_key,
                total_items,
                completed_items,
                remaining_items,
                source_path,
                verified_complete,
            } => {
                lines.push(Line::from(vec![
                    label_span("Action: "),
                    plan_activity_action_span(action),
                ]));
                lines.push(activity_kv_line("Title", title.clone()));
                lines.push(activity_kv_line("Thread", thread_key.clone()));
                lines.push(activity_kv_line("Total items", total_items.to_string()));
                lines.push(activity_kv_line("Completed", completed_items.to_string()));
                lines.push(activity_kv_line(
                    "Remaining",
                    remaining_items.len().to_string(),
                ));
                lines.push(activity_kv_line(
                    "Verified complete",
                    verified_complete.to_string(),
                ));
                if let Some(source_path) = source_path {
                    lines.push(activity_kv_line("Source path", source_path.clone()));
                }
                if !remaining_items.is_empty() {
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![section_span("Remaining Items")]));
                    for item in remaining_items {
                        lines.push(Line::from(Span::styled(
                            format!("- {item}"),
                            Style::default().fg(Theme::TEXT),
                        )));
                    }
                }
            }
            ActivityDetails::Scan {
                dry_run,
                candidate_count,
                files_considered,
                commits_considered,
                index_reused,
                report_path,
                capture_id,
                curate_run_id,
            } => {
                lines.push(activity_kv_line("Dry run", dry_run.to_string()));
                lines.push(activity_kv_line("Candidates", candidate_count.to_string()));
                lines.push(activity_kv_line("Files", files_considered.to_string()));
                lines.push(activity_kv_line("Commits", commits_considered.to_string()));
                lines.push(activity_kv_line("Index reused", index_reused.to_string()));
                lines.push(activity_kv_line("Report", report_path.clone()));
                if let Some(capture_id) = capture_id {
                    lines.push(activity_kv_line("Capture", capture_id.clone()));
                }
                if let Some(curate_run_id) = curate_run_id {
                    lines.push(activity_kv_line("Curate run", curate_run_id.clone()));
                }
            }
            ActivityDetails::GraphExtract {
                repo_root,
                git_head,
                since,
                extraction_run_id,
                dry_run,
                reused_existing_run,
                index_reused,
                analyzer_version,
                strategy_version,
                symbol_count,
                reference_count,
                resolved_reference_count,
                unresolved_reference_count,
                ambiguous_reference_count,
                graph_node_count,
                graph_edge_count,
                evidence_count,
            } => {
                lines.push(activity_kv_line("Repo root", repo_root.clone()));
                if let Some(run_id) = extraction_run_id {
                    lines.push(activity_kv_line("Extraction run", run_id.to_string()));
                }
                lines.push(activity_kv_line("Dry run", dry_run.to_string()));
                lines.push(activity_kv_line(
                    "Reused existing run",
                    reused_existing_run.to_string(),
                ));
                lines.push(activity_kv_line("Index reused", index_reused.to_string()));
                lines.push(activity_kv_line("Analyzer", analyzer_version.clone()));
                lines.push(activity_kv_line("Strategy", strategy_version.clone()));
                lines.push(activity_kv_line("Symbols", symbol_count.to_string()));
                lines.push(activity_kv_line("References", reference_count.to_string()));
                lines.push(activity_kv_line(
                    "Resolved",
                    resolved_reference_count.to_string(),
                ));
                lines.push(activity_kv_line(
                    "Unresolved",
                    unresolved_reference_count.to_string(),
                ));
                lines.push(activity_kv_line(
                    "Ambiguous",
                    ambiguous_reference_count.to_string(),
                ));
                lines.push(activity_kv_line(
                    "Graph nodes",
                    graph_node_count.to_string(),
                ));
                lines.push(activity_kv_line(
                    "Graph edges",
                    graph_edge_count.to_string(),
                ));
                lines.push(activity_kv_line("Evidence", evidence_count.to_string()));
                if let Some(head) = git_head {
                    lines.push(activity_kv_line("HEAD", head.clone()));
                }
                if let Some(since) = since {
                    lines.push(activity_kv_line("Since", since.clone()));
                }
            }
            ActivityDetails::Checkpoint {
                repo_root,
                marked_at,
                note,
                git_branch,
                git_head,
            } => {
                lines.push(activity_kv_line(
                    "Marked at",
                    format_timestamp(Some(*marked_at)),
                ));
                lines.push(activity_kv_line("Repo root", repo_root.clone()));
                lines.push(activity_kv_line(
                    "Note",
                    note.clone().unwrap_or_else(|| "n/a".to_string()),
                ));
                lines.push(activity_kv_line(
                    "Branch",
                    git_branch.clone().unwrap_or_else(|| "n/a".to_string()),
                ));
                lines.push(activity_kv_line(
                    "HEAD",
                    git_head.clone().unwrap_or_else(|| "n/a".to_string()),
                ));
            }
            ActivityDetails::CommitSync {
                imported_count,
                updated_count,
                total_received,
                newest_commit,
                oldest_commit,
            } => {
                lines.push(activity_kv_line("Imported", imported_count.to_string()));
                lines.push(activity_kv_line("Updated", updated_count.to_string()));
                lines.push(activity_kv_line("Received", total_received.to_string()));
                if let Some(newest_commit) = newest_commit {
                    lines.push(activity_kv_line("Newest", newest_commit.clone()));
                }
                if let Some(oldest_commit) = oldest_commit {
                    lines.push(activity_kv_line("Oldest", oldest_commit.clone()));
                }
            }
            ActivityDetails::BundleTransfer {
                bundle_id,
                item_count,
                source_project,
            } => {
                lines.push(activity_kv_line("Bundle", bundle_id.clone()));
                lines.push(activity_kv_line("Items", item_count.to_string()));
                if let Some(source_project) = source_project {
                    lines.push(activity_kv_line("Source project", source_project.clone()));
                }
            }
            ActivityDetails::Query {
                query,
                top_k,
                result_count,
                confidence,
                insufficient_evidence,
                total_duration_ms,
                graph_status,
                graph_candidates,
                graph_augmented_candidates,
                graph_duration_ms,
                graph_result_count,
                graph_connection_count,
                graph_connections,
                answer,
                error,
            } => {
                lines.push(activity_kv_line("Query", query.clone()));
                lines.push(activity_kv_line("Top K", top_k.to_string()));
                lines.push(activity_kv_line("Results", result_count.to_string()));
                lines.push(activity_kv_line("Confidence", format!("{confidence:.2}")));
                lines.push(activity_kv_line(
                    "Insufficient evidence",
                    insufficient_evidence.to_string(),
                ));
                lines.push(activity_kv_line(
                    "Duration",
                    format!("{total_duration_ms} ms"),
                ));
                if let Some(graph_status) = graph_status {
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![section_span("Graph Retrieval")]));
                    lines.push(activity_kv_line("Status", graph_status.clone()));
                    lines.push(activity_kv_line("Candidates", graph_candidates.to_string()));
                    lines.push(activity_kv_line(
                        "Augmented results",
                        graph_augmented_candidates.to_string(),
                    ));
                    lines.push(activity_kv_line(
                        "Duration",
                        format!("{graph_duration_ms} ms"),
                    ));
                    lines.push(activity_kv_line(
                        "Results with graph",
                        graph_result_count.to_string(),
                    ));
                    lines.push(activity_kv_line(
                        "Connections",
                        graph_connection_count.to_string(),
                    ));
                    if !graph_connections.is_empty() {
                        lines.push(Line::from(""));
                        lines.push(Line::from(vec![section_span("Graph Connections")]));
                        for connection in graph_connections {
                            let mut parts = vec![
                                connection.reason.clone(),
                                connection.file_path.clone(),
                                format!("boost={:.2}", connection.score_boost),
                            ];
                            if let Some(symbol) = &connection.symbol {
                                parts.push(format!("symbol={symbol}"));
                            }
                            if let Some(edge_kind) = &connection.edge_kind {
                                parts.push(format!("edge={edge_kind}"));
                            }
                            if let Some(neighbor) = &connection.neighbor_symbol {
                                parts.push(format!("neighbor={neighbor}"));
                            }
                            lines.push(Line::from(Span::styled(
                                format!("- {}", parts.join(" | ")),
                                Style::default().fg(Theme::TEXT),
                            )));
                        }
                    }
                }
                if let Some(answer) = answer {
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![section_span("Answer")]));
                    lines.push(Line::from(Span::styled(
                        answer.clone(),
                        Style::default().fg(Theme::TEXT),
                    )));
                }
                if let Some(error) = error {
                    lines.push(activity_kv_line("Error", error.clone()));
                }
            }
            ActivityDetails::LlmAudit {
                operation,
                request_summary,
                status,
                redacted,
                truncated,
                messages,
                error,
            } => {
                lines.push(activity_kv_line("Operation", operation.clone()));
                lines.push(activity_kv_line("Request", request_summary.clone()));
                lines.push(activity_kv_line("Status", status.clone()));
                lines.push(activity_kv_line("Redacted", redacted.to_string()));
                lines.push(activity_kv_line("Truncated", truncated.to_string()));
                if let Some(error) = error {
                    lines.push(activity_kv_line("Error", error.clone()));
                }
                if !messages.is_empty() {
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![section_span("LLM Messages")]));
                    for message in messages {
                        lines.push(Line::from(vec![
                            label_span(format!("Role {}: ", message.role)),
                            Span::styled(
                                if message.truncated {
                                    format!("{}\n[message truncated]", message.content)
                                } else {
                                    message.content.clone()
                                },
                                Style::default().fg(Theme::TEXT),
                            ),
                        ]));
                    }
                }
            }
            ActivityDetails::CaptureTask {
                session_id,
                task_id,
                raw_capture_id,
                idempotency_key,
                task_title,
                writer_id,
            } => {
                lines.push(activity_kv_line("Session", session_id.to_string()));
                lines.push(activity_kv_line("Task", task_id.to_string()));
                lines.push(activity_kv_line("Raw capture", raw_capture_id.to_string()));
                lines.push(activity_kv_line("Idempotency", idempotency_key.clone()));
                if let Some(task_title) = task_title {
                    lines.push(activity_kv_line("Task title", task_title.clone()));
                }
                lines.push(activity_kv_line("Writer", writer_id.clone()));
            }
            ActivityDetails::Curate {
                run_id,
                input_count,
                output_count,
                replaced_count,
                proposal_count,
            } => {
                lines.push(activity_kv_line("Run", run_id.to_string()));
                lines.push(activity_kv_line("Input captures", input_count.to_string()));
                lines.push(activity_kv_line(
                    "Output memories",
                    output_count.to_string(),
                ));
                lines.push(activity_kv_line("Replacements", replaced_count.to_string()));
                lines.push(activity_kv_line(
                    "Queued proposals",
                    proposal_count.to_string(),
                ));
            }
            ActivityDetails::MemoryReplacement {
                old_memory_id,
                old_summary,
                new_memory_id,
                new_summary,
                automatic,
                policy,
            } => {
                lines.push(activity_kv_line("Old memory", old_memory_id.to_string()));
                lines.push(activity_kv_line("Old summary", old_summary.clone()));
                lines.push(activity_kv_line("New memory", new_memory_id.to_string()));
                lines.push(activity_kv_line("New summary", new_summary.clone()));
                lines.push(activity_kv_line("Automatic", automatic.to_string()));
                lines.push(activity_kv_line("Policy", policy.to_string()));
            }
            ActivityDetails::Reindex { reindexed_entries } => {
                lines.push(activity_kv_line(
                    "Reindexed entries",
                    reindexed_entries.to_string(),
                ));
            }
            ActivityDetails::Reembed { reembedded_chunks } => {
                lines.push(activity_kv_line(
                    "Re-embedded chunks",
                    reembedded_chunks.to_string(),
                ));
            }
            ActivityDetails::Archive {
                archived_count,
                max_confidence,
                max_importance,
            } => {
                lines.push(activity_kv_line(
                    "Archived count",
                    archived_count.to_string(),
                ));
                lines.push(activity_kv_line(
                    "Max confidence",
                    format!("{max_confidence:.2}"),
                ));
                lines.push(activity_kv_line(
                    "Max importance",
                    max_importance.to_string(),
                ));
            }
            ActivityDetails::DeleteMemory { deleted, summary } => {
                lines.push(activity_kv_line("Deleted", deleted.to_string()));
                lines.push(activity_kv_line("Deleted summary", summary.clone()));
            }
            ActivityDetails::Diagnostic { diagnostic } => {
                lines.extend(error_detail_lines(&ErrorItem {
                    when: Some(event.recorded_at),
                    diagnostic: diagnostic.clone(),
                }));
            }
            ActivityDetails::WatcherHealth {
                watcher_id,
                hostname,
                health,
                managed_by_service,
                restart_attempt_count,
                agent_cli,
                agent_session_id,
                agent_pid,
                previous_health,
                recovered_after_restart_attempts,
                message,
            } => {
                lines.push(activity_kv_line("Watcher", watcher_id.clone()));
                lines.push(activity_kv_line("Hostname", hostname.clone()));
                if let Some(agent_cli) = agent_cli {
                    lines.push(activity_kv_line("Agent CLI", agent_cli.clone()));
                }
                if let Some(agent_session_id) = agent_session_id {
                    lines.push(activity_kv_line("Agent session", agent_session_id.clone()));
                }
                if let Some(agent_pid) = agent_pid {
                    lines.push(activity_kv_line("Agent PID", agent_pid.to_string()));
                }
                lines.push(Line::from(vec![
                    label_span("Health: "),
                    watcher_health_span(health),
                ]));
                if let Some(previous_health) = previous_health {
                    lines.push(Line::from(vec![
                        label_span("Previous health: "),
                        watcher_health_span(previous_health),
                    ]));
                }
                lines.push(activity_kv_line(
                    "Managed by service",
                    managed_by_service.to_string(),
                ));
                lines.push(activity_kv_line(
                    "Restart attempts",
                    restart_attempt_count.to_string(),
                ));
                if let Some(attempts) = recovered_after_restart_attempts {
                    lines.push(activity_kv_line(
                        "Recovered after attempts",
                        attempts.to_string(),
                    ));
                }
                lines.push(activity_kv_line(
                    "Message",
                    message.clone().unwrap_or_else(|| "n/a".to_string()),
                ));
            }
        }
    }

    lines
}

pub(in crate::tui) fn activity_kv_line(label: &str, value: String) -> Line<'static> {
    Line::from(vec![
        label_span(format!("{label}: ")),
        Span::styled(value, Style::default().fg(Theme::TEXT)),
    ])
}

pub(in crate::tui) fn truncate_activity_text(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

pub(in crate::tui) fn activity_recorded_at(item: &ActivityEntry) -> DateTime<Utc> {
    match item {
        ActivityEntry::Backend(event) => event.recorded_at,
        ActivityEntry::Query(entry) => entry.recorded_at,
    }
}

pub(in crate::tui) fn activity_summary(item: &ActivityEntry) -> String {
    match item {
        ActivityEntry::Backend(event) => event.summary.clone(),
        ActivityEntry::Query(entry) => {
            let preview = truncate_activity_text(&entry.request.query, 52);
            match &entry.outcome {
                QueryLogOutcome::Success(response) => format!(
                    "{} | {} results | {} ms | conf {:.2}",
                    preview,
                    response.results.len(),
                    entry.duration_ms,
                    response.confidence
                ),
                QueryLogOutcome::Error(_) => {
                    format!("{preview} | error | {} ms", entry.duration_ms)
                }
            }
        }
    }
}

pub(in crate::tui) fn activity_tokens(item: &ActivityEntry) -> String {
    match item {
        ActivityEntry::Backend(event) => event
            .token_usage
            .as_ref()
            .map(|usage| format_compact_count(usage.total_tokens))
            .unwrap_or_else(|| "-".to_string()),
        ActivityEntry::Query(entry) => match &entry.outcome {
            QueryLogOutcome::Success(response) => response
                .answer_generation
                .token_usage
                .as_ref()
                .map(|usage| format_compact_count(usage.total_tokens))
                .unwrap_or_else(|| "-".to_string()),
            QueryLogOutcome::Error(_) => "-".to_string(),
        },
    }
}

pub(in crate::tui) fn activity_duration(item: &ActivityEntry) -> String {
    match item {
        ActivityEntry::Backend(event) => event
            .duration_ms
            .map(format_compact_count)
            .unwrap_or_else(|| "-".to_string()),
        ActivityEntry::Query(entry) => format_compact_count(entry.duration_ms),
    }
}

pub(in crate::tui) fn format_compact_count(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}m", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}k", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

pub(in crate::tui) fn activity_kind_span(kind: &ActivityKind) -> Span<'static> {
    let (label, color) = match kind {
        ActivityKind::Checkpoint => ("checkpoint", Theme::ACCENT_STRONG),
        ActivityKind::Scan => ("scan", Theme::ACCENT_STRONG),
        ActivityKind::Plan => ("plan", Theme::ACCENT_STRONG),
        ActivityKind::CommitSync => ("commit-sync", Theme::ACCENT_STRONG),
        ActivityKind::BundleExport => ("bundle-export", Theme::ACCENT_STRONG),
        ActivityKind::BundleImport => ("bundle-import", Theme::ACCENT_STRONG),
        ActivityKind::GraphExtract => ("graph", Theme::ACCENT_STRONG),
        ActivityKind::Query => ("query", Theme::ACCENT),
        ActivityKind::QueryError => ("query-error", Theme::DANGER),
        ActivityKind::MemoryReplacement => ("replacement", Theme::WARNING),
        ActivityKind::CaptureTask => ("capture", Theme::ACCENT),
        ActivityKind::Curate => ("curate", Theme::SUCCESS),
        ActivityKind::Reindex => ("reindex", Theme::ACCENT_STRONG),
        ActivityKind::Reembed => ("reembed", Theme::ACCENT_STRONG),
        ActivityKind::Archive => ("archive", Theme::WARNING),
        ActivityKind::DeleteMemory => ("delete", Theme::DANGER),
        ActivityKind::Briefing => ("briefing", Theme::SUCCESS),
        ActivityKind::WatcherHealth => ("watcher-health", Theme::WARNING),
        ActivityKind::Diagnostic => ("diagnostic", Theme::DANGER),
        ActivityKind::LlmAudit => ("llm-audit", Theme::WARNING),
        ActivityKind::MemoryValidation => ("validation", Theme::ACCENT),
        ActivityKind::LoopRunStarted => ("loop-run", Theme::ACCENT),
        ActivityKind::LoopRunFinished => ("loop-run", Theme::SUCCESS),
        ActivityKind::LoopRunFailed => ("loop-run", Theme::DANGER),
        ActivityKind::LoopSettingChanged => ("loop-setting", Theme::ACCENT_STRONG),
        ActivityKind::ProposalCreated => ("proposal", Theme::ACCENT),
        ActivityKind::ProposalDecided => ("proposal", Theme::WARNING),
        ActivityKind::ProposalApplied => ("proposal", Theme::SUCCESS),
        ActivityKind::Consolidation => ("consolidation", Theme::ACCENT_STRONG),
        ActivityKind::ProvenanceCheck => ("provenance", Theme::ACCENT),
        ActivityKind::WorkspaceChanged => ("workspace", Theme::ACCENT),
        ActivityKind::TriggerReceived => ("trigger", Theme::ACCENT),
        ActivityKind::AuthEvent => ("auth", Theme::WARNING),
    };
    Span::styled(
        label,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

pub(in crate::tui) fn plan_activity_action_span(action: &PlanActivityAction) -> Span<'static> {
    let (label, color) = match action {
        PlanActivityAction::Started => ("started", Theme::ACCENT_STRONG),
        PlanActivityAction::Synced => ("synced", Theme::ACCENT),
        PlanActivityAction::FinishBlocked => ("finish-blocked", Theme::WARNING),
        PlanActivityAction::FinishVerified => ("finish-verified", Theme::SUCCESS),
    };
    Span::styled(
        label,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

pub(in crate::tui) fn activity_entry_kind_span(item: &ActivityEntry) -> Span<'static> {
    match item {
        ActivityEntry::Backend(event) => {
            if let Some(ActivityDetails::Plan { action, .. }) = event.details.as_ref() {
                return plan_activity_action_span(action);
            }
            if let Some(ActivityDetails::WatcherHealth {
                health: WatcherHealth::Healthy,
                previous_health: Some(previous_health),
                ..
            }) = event.details.as_ref()
            {
                return Span::styled(
                    format!("watcher-{}", watcher_health_label(previous_health)),
                    Style::default()
                        .fg(Theme::SUCCESS)
                        .add_modifier(Modifier::BOLD),
                );
            }
            activity_kind_span(&event.kind)
        }
        ActivityEntry::Query(entry) => match &entry.outcome {
            QueryLogOutcome::Success(response) => {
                if response.insufficient_evidence {
                    Span::styled(
                        "query-weak",
                        Style::default()
                            .fg(Theme::WARNING)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    Span::styled(
                        "query",
                        Style::default()
                            .fg(Theme::ACCENT)
                            .add_modifier(Modifier::BOLD),
                    )
                }
            }
            QueryLogOutcome::Error(_) => Span::styled(
                "query-error",
                Style::default()
                    .fg(Theme::DANGER)
                    .add_modifier(Modifier::BOLD),
            ),
        },
    }
}
