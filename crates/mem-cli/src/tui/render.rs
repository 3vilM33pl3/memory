use std::{fs, path::Path};

use anyhow::{Context, Result};
use chrono::{DateTime, Local, Utc};
use mem_agenttop::{
    AgentSession, AgentSnapshot, ChildProcess as AgentChildProcess,
    SessionStatus as AgentSessionStatus,
};
use mem_api::{
    ActivityDetails, ActivityEvent, ActivityKind, DiagnosticInfo, DiagnosticSeverity, MemoryStatus,
    MemoryType, PlanActivityAction, Profile, ProjectMemoryListItem, QueryAnswerMethod,
    QueryFilters, QueryMatchKind, QueryResponse, QueryResult, ReplacementPolicy, WatcherHealth,
    repo_agent_settings_path,
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Tabs, Wrap},
};

use crate::commands::{memory_ops::SourceKindString, skill_support::SkillBundleStatus};

use super::{
    app::*,
    markdown::render_markdown_lines,
    tabs::{
        TabRenderContext, activity::draw_activity_tab, agents::draw_agents_tab,
        embeddings::draw_embeddings_tab, errors::draw_errors_tab, memories::draw_memories_tab,
        project::draw_project_tab, query::draw_query_tab, resume::draw_resume_tab,
        review::draw_review_tab, watchers::draw_watchers_tab,
    },
    theme::{Theme, themed_block, themed_focus_block},
};

pub(super) fn build_history_lines(history: &mem_api::MemoryHistoryResponse) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        label_span("Canonical: "),
        Span::styled(
            history.canonical_id.to_string(),
            Style::default().fg(Theme::TEXT),
        ),
        Span::raw("   "),
        label_span("Versions: "),
        Span::styled(
            history.versions.len().to_string(),
            Style::default().fg(Theme::ACCENT_STRONG),
        ),
    ]));
    lines.push(Line::from(Span::styled(
        "Press Shift+H again to return to the single-version detail.",
        Style::default().fg(Theme::MUTED),
    )));
    lines.push(Line::from(""));
    for version in &history.versions {
        let header_style = if version.is_tombstone {
            Style::default()
                .fg(Theme::DANGER)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(Theme::ACCENT_STRONG)
                .add_modifier(Modifier::BOLD)
        };
        let tombstone_suffix = if version.is_tombstone {
            "  [tombstone]"
        } else {
            ""
        };
        lines.push(Line::from(vec![
            Span::styled(format!("v{}", version.version_no), header_style),
            Span::raw("  "),
            memory_type_span(&version.memory_type),
            Span::raw("  "),
            status_span(match version.status {
                MemoryStatus::Active => "active",
                MemoryStatus::Archived => "archived",
            }),
            Span::styled(
                tombstone_suffix.to_string(),
                Style::default().fg(Theme::DANGER),
            ),
        ]));
        lines.push(Line::from(vec![
            label_span("id: "),
            Span::styled(version.id.to_string(), Style::default().fg(Theme::MUTED)),
            Span::raw("   "),
            label_span("updated: "),
            Span::styled(
                format_timestamp_medium(version.updated_at),
                Style::default().fg(Theme::MUTED),
            ),
        ]));
        if version.is_tombstone {
            lines.push(Line::from(Span::styled(
                "  (empty — memory was deleted at this point)",
                Style::default().fg(Theme::MUTED),
            )));
        } else {
            lines.push(Line::from(vec![
                label_span("summary: "),
                Span::styled(version.summary.clone(), Style::default().fg(Theme::TEXT)),
            ]));
            let preview: String = version.canonical_text.chars().take(320).collect();
            let ellipsis = if version.canonical_text.chars().count() > 320 {
                "..."
            } else {
                ""
            };
            lines.push(Line::from(Span::styled(
                format!("{preview}{ellipsis}"),
                Style::default().fg(Theme::TEXT),
            )));
        }
        lines.push(Line::from(""));
    }
    lines
}

pub(super) fn build_memory_detail_lines(app: &App) -> Vec<Line<'static>> {
    if let Some(history) = &app.memories.selected_history {
        return build_history_lines(history);
    }
    if let Some(detail) = &app.memories.selected_detail {
        let mut lines = vec![
            Line::from(vec![
                label_span("Summary: "),
                Span::styled(detail.summary.clone(), Style::default().fg(Theme::TEXT)),
            ]),
            Line::from(vec![
                label_span("Type: "),
                memory_type_span(&detail.memory_type),
                Span::raw("   "),
                label_span("Status: "),
                status_span(match detail.status {
                    MemoryStatus::Active => "active",
                    MemoryStatus::Archived => "archived",
                }),
            ]),
            Line::from(vec![
                label_span("Confidence: "),
                Span::styled(
                    format!("{:.2}", detail.confidence),
                    confidence_style(detail.confidence),
                ),
                Span::raw("   "),
                label_span("Importance: "),
                Span::styled(
                    detail.importance.to_string(),
                    Style::default().fg(Theme::TEXT),
                ),
            ]),
            Line::from(vec![
                label_span("Updated: "),
                Span::styled(
                    format_timestamp_medium(detail.updated_at),
                    Style::default().fg(Theme::TEXT),
                ),
            ]),
            Line::from(""),
            Line::from(vec![section_span("Embeddings")]),
        ];
        if detail.embedding_spaces.is_empty() {
            lines.push(Line::from(Span::styled(
                "No embeddings for this memory yet. Run Re-embed for this project to populate the active embedding space.",
                Style::default().fg(Theme::MUTED),
            )));
        } else {
            for space in &detail.embedding_spaces {
                let chunks_label = if space.chunk_count == 1 {
                    "1 chunk".to_string()
                } else {
                    format!("{} chunks", space.chunk_count)
                };
                let mut spans = vec![
                    Span::styled(space.provider.clone(), Style::default().fg(Theme::ACCENT)),
                    Span::raw(" · "),
                    Span::styled(space.model.clone(), Style::default().fg(Theme::TEXT)),
                    Span::raw(" · "),
                    Span::styled(chunks_label, Style::default().fg(Theme::TEXT)),
                ];
                if let Some(updated) = space.last_updated {
                    spans.push(Span::raw(" · "));
                    spans.push(Span::styled(
                        format!("updated {}", format_timestamp_medium(updated)),
                        Style::default().fg(Theme::MUTED),
                    ));
                }
                lines.push(Line::from(spans));
                if !embedding_base_url_is_default(&space.provider, &space.base_url) {
                    lines.push(Line::from(Span::styled(
                        format!("    {}", space.base_url),
                        Style::default().fg(Theme::MUTED),
                    )));
                }
            }
        }
        lines.push(Line::from(""));
        lines.push(Line::from(vec![section_span("Canonical Text")]));
        lines.extend(render_markdown_lines(&detail.canonical_text));
        lines.push(Line::from(""));
        lines.extend([
            Line::from(vec![
                label_span("Tags: "),
                Span::styled(
                    if detail.tags.is_empty() {
                        "none".to_string()
                    } else {
                        detail.tags.join(", ")
                    },
                    Style::default().fg(Theme::TEXT),
                ),
            ]),
            Line::from(""),
            Line::from(vec![section_span("Sources")]),
        ]);

        if detail.sources.is_empty() {
            lines.push(Line::from(Span::styled(
                "No provenance sources recorded.",
                Style::default().fg(Theme::MUTED),
            )));
        } else {
            for source in &detail.sources {
                let mut parts = vec![source.source_kind.source_kind_string().to_string()];
                if let Some(path) = &source.file_path {
                    parts.push(path.clone());
                }
                if let Some(excerpt) = &source.excerpt {
                    parts.push(excerpt.clone());
                }
                if let Some(provenance) = &source.provenance {
                    parts.push(format!("provenance: {}", provenance.status.as_str()));
                    if let Some(reason) = &provenance.reason {
                        parts.push(reason.clone());
                    }
                }
                lines.push(Line::from(Span::styled(
                    parts.join(" | "),
                    Style::default().fg(Theme::TEXT),
                )));
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(vec![section_span("Related Memories")]));
        if detail.related_memories.is_empty() {
            lines.push(Line::from(Span::styled(
                "No related memories recorded.",
                Style::default().fg(Theme::MUTED),
            )));
        } else {
            for related in &detail.related_memories {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{} ", related.relation_type),
                        Style::default().fg(Theme::ACCENT),
                    ),
                    memory_type_span(&related.memory_type),
                    Span::raw(" "),
                    Span::styled(
                        format!("({:.2}) ", related.confidence),
                        confidence_style(related.confidence),
                    ),
                    Span::styled(related.summary.clone(), Style::default().fg(Theme::TEXT)),
                ]));
            }
        }
        lines
    } else if app.memories.filtered_memories.is_empty() {
        vec![Line::from(Span::styled(
            format!(
                "No memories match the current filters for project {}.",
                app.project
            ),
            Style::default().fg(Theme::MUTED),
        ))]
    } else {
        vec![Line::from(Span::styled(
            "Select a memory to load its details.",
            Style::default().fg(Theme::MUTED),
        ))]
    }
}

pub(super) fn review_detail_lines(app: &App) -> Vec<Line<'static>> {
    let Some(proposal) = app
        .review
        .replacement_proposals
        .get(app.review.replacement_selected_index)
    else {
        return vec![Line::from(Span::styled(
            "Select a proposal on the left to inspect it here.",
            Style::default().fg(Theme::MUTED),
        ))];
    };

    let mut lines = vec![
        Line::from(vec![
            label_span("Target: "),
            Span::styled(
                proposal.target_summary.clone(),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
        Line::from(vec![
            label_span("Candidate: "),
            Span::styled(
                proposal.candidate_summary.clone(),
                Style::default().fg(Theme::ACCENT),
            ),
        ]),
        Line::from(vec![
            label_span("Type / Score / Policy: "),
            Span::styled(
                format!(
                    "{} / {} / {}",
                    proposal.candidate_memory_type, proposal.score, proposal.policy
                ),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
    ];
    if !proposal.reasons.is_empty() {
        lines.push(Line::from(vec![
            label_span("Why: "),
            Span::styled(
                proposal.reasons.join(", "),
                Style::default().fg(Theme::MUTED),
            ),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        proposal.candidate_canonical_text.clone(),
        Style::default().fg(Theme::MUTED),
    )));
    lines
}

pub(super) fn truncate_for_list(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

pub(super) fn active_embedding_backend_index(
    snapshot: &mem_api::EmbeddingBackendsResponse,
) -> Option<usize> {
    snapshot.backends.iter().position(|backend| backend.active)
}

pub(super) fn embedding_backend_index_by_name(
    snapshot: &mem_api::EmbeddingBackendsResponse,
    name: &str,
) -> Option<usize> {
    snapshot
        .backends
        .iter()
        .position(|backend| backend.name == name)
}

pub(super) fn clamped_embedding_backend_index(
    current: usize,
    snapshot: &mem_api::EmbeddingBackendsResponse,
) -> Option<usize> {
    (!snapshot.backends.is_empty()).then(|| current.min(snapshot.backends.len().saturating_sub(1)))
}

pub(super) fn current_query_display(app: &App) -> String {
    match &app.chrome.input_mode {
        InputMode::Query(value) => value.clone(),
        _ => app.query.query_text.clone(),
    }
}

pub(super) struct QueryInputDisplay {
    pub(in crate::tui) text: String,
    pub(in crate::tui) cursor_col: u16,
    pub(in crate::tui) placeholder: bool,
}

pub(super) fn query_input_display(value: &str, inner_width: u16) -> QueryInputDisplay {
    let width = inner_width as usize;
    if width == 0 {
        return QueryInputDisplay {
            text: String::new(),
            cursor_col: 0,
            placeholder: value.is_empty(),
        };
    }
    if value.is_empty() {
        let placeholder = "Ask project memory a question...";
        let text = placeholder.chars().take(width).collect::<String>();
        return QueryInputDisplay {
            text,
            cursor_col: 0,
            placeholder: true,
        };
    }

    let char_count = value.chars().count();
    if char_count <= width {
        return QueryInputDisplay {
            text: value.to_string(),
            cursor_col: char_count.min(width.saturating_sub(1)) as u16,
            placeholder: false,
        };
    }

    let tail_width = width.saturating_sub(1);
    let mut tail = value
        .chars()
        .skip(char_count.saturating_sub(tail_width))
        .collect::<String>();
    tail.insert(0, '<');
    QueryInputDisplay {
        text: tail,
        cursor_col: width.saturating_sub(1) as u16,
        placeholder: false,
    }
}

pub(super) fn append_resume_briefing_lines(lines: &mut Vec<Line<'static>>, briefing: &str) {
    for raw_line in briefing.lines() {
        let trimmed = raw_line.trim_end();
        if trimmed.is_empty() {
            lines.push(Line::from(""));
            continue;
        }

        let line = if let Some(heading) = trimmed.strip_prefix("### ") {
            Line::from(Span::styled(
                heading.to_string(),
                Style::default()
                    .fg(Theme::ACCENT_STRONG)
                    .add_modifier(Modifier::BOLD),
            ))
        } else {
            Line::from(Span::styled(
                trimmed.to_string(),
                Style::default().fg(Theme::TEXT),
            ))
        };
        lines.push(line);
    }
}

#[derive(Clone)]
pub(super) struct ErrorItem {
    pub(in crate::tui) when: Option<DateTime<Utc>>,
    pub(in crate::tui) diagnostic: DiagnosticInfo,
}

pub(super) fn collect_error_items(app: &App) -> Vec<ErrorItem> {
    let mut items = Vec::new();
    if !app.service.health_ok {
        items.push(ErrorItem {
            when: Some(Utc::now()),
            diagnostic: session_diagnostic(
                "backend_unavailable",
                "tui",
                "service",
                "health",
                "Memory Layer backend is unavailable.",
                Some("The TUI cannot reach the service yet or the service health check is failing."),
                Some("Start the service or run `memory doctor` to inspect configuration and database connectivity."),
            ),
        });
    }
    for (code, component, operation, message) in [
        (
            "query_failed",
            "tui",
            "query",
            app.query.query_error.as_ref(),
        ),
        (
            "agents_failed",
            "tui",
            "agents",
            app.agents.agent_error.as_ref(),
        ),
        (
            "resume_failed",
            "tui",
            "resume",
            app.resume.resume_error.as_ref(),
        ),
        (
            "activity_failed",
            "tui",
            "activity",
            app.activity.activity_error.as_ref(),
        ),
        (
            "briefing_failed",
            "tui",
            "up_to_speed",
            app.activity.up_to_speed_error.as_ref(),
        ),
        (
            "embeddings_failed",
            "tui",
            "embeddings",
            app.embeddings.embedding_backends_error.as_ref(),
        ),
    ] {
        if let Some(message) = message {
            items.push(ErrorItem {
                when: Some(Utc::now()),
                diagnostic: session_diagnostic(
                    code,
                    "tui",
                    component,
                    operation,
                    message,
                    Some("This error was observed locally by the current TUI session."),
                    Some("Refresh the tab, then run `memory doctor` if the problem persists."),
                ),
            });
        }
    }
    for entry in &app.activity.activity_events {
        if let ActivityEntry::Backend(event) = entry {
            match &event.details {
                Some(ActivityDetails::Diagnostic { diagnostic }) => items.push(ErrorItem {
                    when: Some(event.recorded_at),
                    diagnostic: diagnostic.clone(),
                }),
                Some(ActivityDetails::Query {
                    error: Some(error), ..
                }) => {
                    items.push(ErrorItem {
                        when: Some(event.recorded_at),
                        diagnostic: session_diagnostic(
                            "query_error",
                            event.source.as_deref().unwrap_or("service"),
                            "query",
                            "query",
                            error,
                            Some("A persisted project query failed."),
                            Some("Open the query/activity detail and run `memory doctor` if this repeats."),
                        ),
                    });
                }
                Some(ActivityDetails::WatcherHealth {
                    health: WatcherHealth::Failed | WatcherHealth::Stale | WatcherHealth::Restarting,
                    message,
                    watcher_id,
                    ..
                }) => {
                    items.push(ErrorItem {
                        when: Some(event.recorded_at),
                        diagnostic: session_diagnostic(
                            "watcher_health",
                            event.source.as_deref().unwrap_or("watcher"),
                            "watcher",
                            "heartbeat",
                            message.as_deref().unwrap_or(&event.summary),
                            Some("A watcher reported unhealthy or restarting state."),
                            Some(&format!(
                                "Inspect watcher `{watcher_id}` with `memory watcher list` or run `memory doctor`."
                            )),
                        ),
                    });
                }
                _ if matches!(event.kind, ActivityKind::QueryError) => items.push(ErrorItem {
                    when: Some(event.recorded_at),
                    diagnostic: session_diagnostic(
                        "query_error",
                        event.source.as_deref().unwrap_or("service"),
                        "query",
                        "query",
                        &event.summary,
                        Some("A persisted project query failed."),
                        Some("Open the activity detail and run `memory doctor` if this repeats."),
                    ),
                }),
                _ => {}
            }
        }
    }
    items.sort_by_key(|item| std::cmp::Reverse(item.when));
    items
}

pub(super) fn session_diagnostic(
    code: &str,
    source: &str,
    component: &str,
    operation: &str,
    message: &str,
    explanation: Option<&str>,
    fix_hint: Option<&str>,
) -> DiagnosticInfo {
    DiagnosticInfo {
        code: code.to_string(),
        source: source.to_string(),
        component: component.to_string(),
        operation: operation.to_string(),
        severity: DiagnosticSeverity::Error,
        message: message.to_string(),
        raw_error: Some(message.to_string()),
        explanation: explanation.map(str::to_string),
        fix_hint: fix_hint.map(str::to_string),
        doctor_hint: Some("memory doctor".to_string()),
        command_hint: Some("memory doctor".to_string()),
    }
}

pub(super) fn error_count(app: &App) -> usize {
    collect_error_items(app).len()
}

pub(super) fn error_row(item: &ErrorItem) -> Row<'static> {
    Row::new(vec![
        Cell::from(Span::styled(
            item.when
                .map(format_timestamp_short)
                .unwrap_or_else(|| "-".to_string()),
            Style::default().fg(Theme::TEXT),
        )),
        Cell::from(Span::styled(
            diagnostic_severity_label(&item.diagnostic.severity),
            Style::default().fg(diagnostic_severity_color(&item.diagnostic.severity)),
        )),
        Cell::from(Span::styled(
            non_empty_or(&item.diagnostic.source, "unknown"),
            Style::default().fg(Theme::MUTED),
        )),
        Cell::from(Span::styled(
            non_empty_or(&item.diagnostic.component, "unknown"),
            Style::default().fg(Theme::ACCENT),
        )),
        Cell::from(Span::styled(
            item.diagnostic.message.clone(),
            Style::default().fg(Theme::TEXT),
        )),
    ])
}

pub(super) fn error_detail_lines(item: &ErrorItem) -> Vec<Line<'static>> {
    let diagnostic = &item.diagnostic;
    let mut lines = vec![
        Line::from(vec![
            label_span("When: "),
            Span::styled(
                item.when
                    .map(format_timestamp_full)
                    .unwrap_or_else(|| "session-local".to_string()),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
        Line::from(vec![
            label_span("Severity: "),
            Span::styled(
                diagnostic_severity_label(&diagnostic.severity),
                Style::default()
                    .fg(diagnostic_severity_color(&diagnostic.severity))
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            label_span("Code: "),
            Span::styled(
                non_empty_or(&diagnostic.code, "unknown"),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
        Line::from(vec![
            label_span("Source: "),
            Span::styled(
                non_empty_or(&diagnostic.source, "unknown"),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Component: "),
            Span::styled(
                non_empty_or(&diagnostic.component, "unknown"),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Operation: "),
            Span::styled(
                non_empty_or(&diagnostic.operation, "unknown"),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
        Line::from(""),
        Line::from(vec![section_span("Summary")]),
        Line::from(Span::styled(
            diagnostic.message.clone(),
            Style::default().fg(Theme::TEXT),
        )),
    ];
    if let Some(explanation) = &diagnostic.explanation {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![section_span("Explanation")]));
        lines.push(Line::from(Span::styled(
            explanation.clone(),
            Style::default().fg(Theme::TEXT),
        )));
    }
    if let Some(fix_hint) = &diagnostic.fix_hint {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![section_span("How To Fix")]));
        lines.push(Line::from(Span::styled(
            fix_hint.clone(),
            Style::default().fg(Theme::SUCCESS),
        )));
    }
    if diagnostic.doctor_hint.is_some() || diagnostic.command_hint.is_some() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![section_span("Commands")]));
        if let Some(command) = &diagnostic.doctor_hint {
            lines.push(Line::from(vec![
                label_span("Doctor: "),
                Span::styled(command.clone(), Style::default().fg(Theme::ACCENT_STRONG)),
            ]));
        }
        if let Some(command) = &diagnostic.command_hint {
            lines.push(Line::from(vec![
                label_span("Related: "),
                Span::styled(command.clone(), Style::default().fg(Theme::ACCENT_STRONG)),
            ]));
        }
    }
    if let Some(raw_error) = &diagnostic.raw_error {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![section_span("Raw Error")]));
        for line in raw_error.lines().take(12) {
            lines.push(Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(Theme::MUTED),
            )));
        }
    }
    lines
}

pub(super) fn diagnostic_severity_label(severity: &DiagnosticSeverity) -> &'static str {
    match severity {
        DiagnosticSeverity::Info => "info",
        DiagnosticSeverity::Warning => "warn",
        DiagnosticSeverity::Error => "error",
    }
}

pub(super) fn diagnostic_severity_color(severity: &DiagnosticSeverity) -> Color {
    match severity {
        DiagnosticSeverity::Info => Theme::ACCENT,
        DiagnosticSeverity::Warning => Theme::WARNING,
        DiagnosticSeverity::Error => Theme::DANGER,
    }
}

pub(super) fn non_empty_or(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

pub(super) fn activity_briefing_lines(app: &App) -> Vec<Line<'static>> {
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

pub(super) fn llm_audit_status_lines(app: &App) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from("")];
    if app.activity.llm_audit_toggling {
        lines.push(Line::from(vec![
            label_span("LLM audit: "),
            Span::styled("updating...", Style::default().fg(Theme::ACCENT_STRONG)),
            Span::styled("  A toggle", Style::default().fg(Theme::MUTED)),
        ]));
        return lines;
    }
    if app.activity.llm_audit_loading {
        lines.push(Line::from(vec![
            label_span("LLM audit: "),
            Span::styled("loading...", Style::default().fg(Theme::ACCENT)),
        ]));
        return lines;
    }
    if let Some(error) = &app.activity.llm_audit_error {
        lines.push(Line::from(vec![
            label_span("LLM audit: "),
            Span::styled("unknown", Style::default().fg(Theme::WARNING)),
            Span::styled(format!("  {error}"), Style::default().fg(Theme::MUTED)),
        ]));
        lines.push(Line::from(Span::styled(
            "Press A to retry toggling, or run memory doctor if status stays unavailable.",
            Style::default().fg(Theme::MUTED),
        )));
        return lines;
    }
    let Some(status) = &app.activity.llm_audit_status else {
        lines.push(Line::from(vec![
            label_span("LLM audit: "),
            Span::styled("unknown", Style::default().fg(Theme::MUTED)),
            Span::styled("  A enable", Style::default().fg(Theme::MUTED)),
        ]));
        return lines;
    };
    lines.push(Line::from(vec![
        label_span("LLM audit: "),
        Span::styled(
            if status.enabled { "on" } else { "off" },
            Style::default()
                .fg(if status.enabled {
                    Theme::SUCCESS
                } else {
                    Theme::MUTED
                })
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                "  redaction={}  profile={}  A {}",
                if status.redacted { "on" } else { "off" },
                status.profile,
                if status.enabled { "disable" } else { "enable" }
            ),
            Style::default().fg(Theme::MUTED),
        ),
    ]));
    if let Some(path) = &status.config_path {
        lines.push(Line::from(vec![
            label_span("Audit config: "),
            Span::styled(path.clone(), Style::default().fg(Theme::MUTED)),
        ]));
    }
    lines
}

pub(super) fn lines_for_named_counts(items: Vec<(String, i64)>, empty: &str) -> Vec<Line<'static>> {
    if items.is_empty() {
        vec![Line::from(empty.to_string())]
    } else {
        items
            .into_iter()
            .map(|(name, count)| {
                Line::from(vec![
                    Span::styled(name, Style::default().fg(Theme::TEXT)),
                    Span::styled(": ", Style::default().fg(Theme::MUTED)),
                    Span::styled(count.to_string(), Style::default().fg(Theme::ACCENT_STRONG)),
                ])
            })
            .collect()
    }
}

pub(super) fn recent_activity_lines(app: &App) -> Vec<Line<'static>> {
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

pub(super) fn latest_plan_display(app: &App) -> String {
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

pub(super) fn watcher_summary_text(app: &App) -> String {
    let Some(summary) = &app.meta.overview.watchers else {
        return "no watcher presence reported".to_string();
    };

    format!(
        "{} healthy / {} unhealthy / stale after {}s / last {}",
        summary.active_count,
        summary.unhealthy_count,
        summary.stale_after_seconds,
        summary
            .last_heartbeat_at
            .map(format_timestamp_short)
            .unwrap_or_else(|| "n/a".to_string())
    )
}

pub(super) fn watcher_detail_lines(app: &App) -> Vec<Line<'static>> {
    let Some(summary) = &app.meta.overview.watchers else {
        return vec![
            Line::from(Span::styled(
                "No watcher presence reported.",
                Style::default().fg(Theme::MUTED),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Start the Linux manager with `memory watcher manager enable`, or use `memory watcher enable --project <slug>` / `memory watcher run --project <slug>` for manual mode.",
                Style::default().fg(Theme::MUTED),
            )),
        ];
    };
    if summary.watchers.is_empty() {
        return vec![
            Line::from(Span::styled(
                format!(
                    "0 healthy watcher(s), {} unhealthy. Stale after {}s.",
                    summary.unhealthy_count, summary.stale_after_seconds
                ),
                Style::default().fg(Theme::MUTED),
            )),
            Line::from(Span::styled(
                "Start the Linux manager with `memory watcher manager enable`, or use `memory watcher enable --project <slug>` / `memory watcher run --project <slug>` for manual mode.",
                Style::default().fg(Theme::MUTED),
            )),
        ];
    }

    let mut lines = vec![Line::from(Span::styled(
        format!(
            "{} active watcher(s), stale after {}s.",
            summary.active_count, summary.stale_after_seconds
        ),
        Style::default().fg(Theme::TEXT),
    ))];
    if summary.unhealthy_count > 0 {
        lines.push(Line::from(Span::styled(
            format!(
                "{} watcher(s) currently unhealthy.",
                summary.unhealthy_count
            ),
            Style::default().fg(Theme::WARNING),
        )));
    }
    if let Some(last_heartbeat) = summary.last_heartbeat_at {
        lines.push(Line::from(vec![
            label_span("Last heartbeat: "),
            Span::styled(
                format_timestamp_full(last_heartbeat),
                Style::default().fg(Theme::TEXT),
            ),
        ]));
    }
    lines.push(Line::from(""));
    for watcher in &summary.watchers {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} ", watcher.hostname),
                Style::default().fg(Theme::ACCENT),
            ),
            Span::styled(
                format!("pid={} ", watcher.pid),
                Style::default().fg(Theme::ACCENT_STRONG),
            ),
            Span::styled(
                format!("{} ", watcher.mode),
                Style::default().fg(Theme::TEXT),
            ),
            Span::styled(
                format_timestamp_short(watcher.last_heartbeat_at),
                Style::default().fg(Theme::MUTED),
            ),
        ]));
        lines.push(Line::from(vec![
            label_span("  status: "),
            watcher_health_span(&watcher.health),
            Span::styled(
                if watcher.managed_by_service {
                    " managed".to_string()
                } else {
                    " manual".to_string()
                },
                Style::default().fg(Theme::MUTED),
            ),
        ]));
        lines.push(Line::from(Span::styled(
            format!("  repo: {}", watcher.repo_root),
            Style::default().fg(Theme::MUTED),
        )));
        lines.push(Line::from(Span::styled(
            format!("  watcher: {}", watcher.watcher_id),
            Style::default().fg(Theme::MUTED),
        )));
        if watcher.agent_cli.is_some() || watcher.agent_session_id.is_some() {
            lines.push(Line::from(Span::styled(
                format!(
                    "  owner: {} session={} pid={}",
                    watcher
                        .agent_cli
                        .clone()
                        .unwrap_or_else(|| "unknown".to_string()),
                    watcher
                        .agent_session_id
                        .clone()
                        .unwrap_or_else(|| "n/a".to_string()),
                    watcher
                        .agent_pid
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "n/a".to_string()),
                ),
                Style::default().fg(Theme::MUTED),
            )));
        }
        lines.push(Line::from(Span::styled(
            format!("  host service: {}", watcher.host_service_id),
            Style::default().fg(Theme::MUTED),
        )));
        lines.push(Line::from(Span::styled(
            format!("  restart attempts: {}", watcher.restart_attempt_count),
            Style::default().fg(Theme::MUTED),
        )));
        if let Some(last_restart) = watcher.last_restart_attempt_at {
            lines.push(Line::from(Span::styled(
                format!(
                    "  last restart attempt: {}",
                    format_timestamp_full(last_restart)
                ),
                Style::default().fg(Theme::MUTED),
            )));
        }
        lines.push(Line::from(""));
    }
    lines
}

pub(super) fn write_replacement_policy(repo_root: &Path, policy: ReplacementPolicy) -> Result<()> {
    let path = repo_agent_settings_path(repo_root);
    let mut value = if path.exists() {
        fs::read_to_string(&path)?
            .parse::<toml::Value>()
            .context("parse .agents/memory-layer.toml")?
    } else {
        toml::Value::Table(toml::map::Map::new())
    };
    let table = value
        .as_table_mut()
        .context(".agents/memory-layer.toml must be a top-level table")?;
    let curation = table
        .entry("curation".to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    let curation_table = curation
        .as_table_mut()
        .context("[curation] must be a table")?;
    curation_table.insert(
        "replacement_policy".to_string(),
        toml::Value::String(policy.to_string()),
    );
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, toml::to_string_pretty(&value)?)?;
    Ok(())
}

pub(super) fn memory_row(item: &ProjectMemoryListItem) -> Row<'static> {
    let row_style = match item.status {
        MemoryStatus::Active => Style::default().fg(Theme::TEXT).bg(Theme::PANEL),
        MemoryStatus::Archived => Style::default().fg(Theme::MUTED).bg(Theme::PANEL),
    };
    // Build the summary cell with an optional "v2"/"v3"/... badge so the
    // user can tell at a glance that the row is a replacement rather than
    // an original capture. v1 never shows a badge to keep the list clean.
    let mut summary_spans = Vec::with_capacity(2);
    summary_spans.push(Span::styled(
        item.summary.clone(),
        Style::default().fg(Theme::TEXT),
    ));
    if item.version_no > 1 {
        summary_spans.push(Span::raw("  "));
        summary_spans.push(Span::styled(
            format!("v{}", item.version_no),
            Style::default()
                .fg(Theme::ACCENT_STRONG)
                .add_modifier(Modifier::BOLD),
        ));
    }
    Row::new(vec![
        Cell::from(Line::from(summary_spans)),
        Cell::from(memory_type_span(&item.memory_type)),
        Cell::from(status_span(match item.status {
            MemoryStatus::Active => "active",
            MemoryStatus::Archived => "archived",
        })),
        Cell::from(Span::styled(
            format!("{:.2}", item.confidence),
            confidence_style(item.confidence),
        )),
        Cell::from(Span::styled(
            format_timestamp_medium(item.updated_at),
            Style::default().fg(Theme::MUTED),
        )),
    ])
    .style(row_style)
}

pub(super) fn agent_row(session: &AgentSession) -> Row<'static> {
    Row::new(vec![
        Cell::from(Span::styled(
            session.project_name.clone(),
            Style::default().fg(Theme::TEXT),
        )),
        Cell::from(Span::styled(
            session.agent_cli.to_string(),
            Style::default().fg(Theme::ACCENT),
        )),
        Cell::from(agent_status_span(&session.status)),
        Cell::from(Span::styled(
            format_token_count(session.total_tokens()),
            Style::default().fg(Theme::TEXT),
        )),
        Cell::from(Span::styled(
            format_context_percent(session.context_percent),
            context_percent_style(session.context_percent),
        )),
        Cell::from(Span::styled(
            agent_task_summary(session),
            Style::default().fg(Theme::TEXT),
        )),
    ])
}

pub(super) fn query_row(result_number: usize, item: &QueryResult, cited: bool) -> Row<'static> {
    let number = if cited {
        format!("[{result_number}]")
    } else {
        result_number.to_string()
    };
    let number_style = if cited {
        Style::default()
            .fg(Theme::SUCCESS)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Theme::MUTED)
    };
    Row::new(vec![
        Cell::from(Span::styled(number, number_style)),
        Cell::from(Span::styled(
            item.summary.clone(),
            Style::default().fg(Theme::TEXT),
        )),
        Cell::from(memory_type_span(&item.memory_type)),
        Cell::from(query_match_span(&item.match_kind)),
        Cell::from(Span::styled(
            format!("{:.2}", item.score),
            Style::default().fg(Theme::ACCENT_STRONG),
        )),
    ])
}

pub(super) fn format_query_citation_numbers(numbers: &[usize]) -> String {
    if numbers.is_empty() {
        "none".to_string()
    } else {
        numbers
            .iter()
            .map(|number| format!("[{number}]"))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

pub(super) fn query_answer_method_span(method: &QueryAnswerMethod) -> Span<'static> {
    let color = match method {
        QueryAnswerMethod::Llm => Theme::SUCCESS,
        QueryAnswerMethod::Deterministic => Theme::ACCENT,
        QueryAnswerMethod::Fallback => Theme::WARNING,
    };
    Span::styled(method.to_string(), Style::default().fg(color))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct QueryTimingBreakdown {
    pub(in crate::tui) backend_reported_ms: u64,
    pub(in crate::tui) transport_overhead_ms: u64,
    pub(in crate::tui) retrieval_other_ms: u64,
}

pub(super) fn query_timing_breakdown(
    response: &QueryResponse,
    timing: QueryRoundtripTiming,
) -> QueryTimingBreakdown {
    let diagnostics = &response.diagnostics;
    let backend_reported_ms = diagnostics
        .total_duration_ms
        .saturating_add(response.answer_generation.duration_ms);
    let retrieval_known_ms = diagnostics
        .lexical_duration_ms
        .saturating_add(diagnostics.semantic_duration_ms)
        .saturating_add(diagnostics.graph_duration_ms)
        .saturating_add(diagnostics.rerank_duration_ms);
    QueryTimingBreakdown {
        backend_reported_ms,
        transport_overhead_ms: timing.query_api_ms.saturating_sub(backend_reported_ms),
        retrieval_other_ms: diagnostics
            .total_duration_ms
            .saturating_sub(retrieval_known_ms),
    }
}

pub(super) fn format_query_timing(value: Option<u64>) -> String {
    value
        .map(|value| format!("{value} ms"))
        .unwrap_or_else(|| "n/a".to_string())
}

pub(super) fn format_query_timing_with_percent(value: u64, total: u64) -> String {
    value
        .saturating_mul(100)
        .checked_div(total)
        .map(|percent| format!("{value} ms ({percent}%)"))
        .unwrap_or_else(|| format!("{value} ms"))
}

pub(super) fn query_timing_breakdown_lines(
    response: &QueryResponse,
    timing: Option<QueryRoundtripTiming>,
) -> Vec<Line<'static>> {
    let fallback_timing = QueryRoundtripTiming {
        query_api_ms: response
            .diagnostics
            .total_duration_ms
            .saturating_add(response.answer_generation.duration_ms),
        initial_detail_ms: None,
        ui_ready_ms: response
            .diagnostics
            .total_duration_ms
            .saturating_add(response.answer_generation.duration_ms),
    };
    let timing = timing.unwrap_or(fallback_timing);
    let breakdown = query_timing_breakdown(response, timing);
    let retrieval_total = response.diagnostics.total_duration_ms;

    vec![
        Line::from(vec![section_span("Timing Breakdown")]),
        Line::from(vec![
            label_span("UI ready: "),
            Span::styled(
                format_query_timing(Some(timing.ui_ready_ms)),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Query API: "),
            Span::styled(
                format_query_timing(Some(timing.query_api_ms)),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Initial detail: "),
            Span::styled(
                format_query_timing(timing.initial_detail_ms),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
        Line::from(vec![
            label_span("Backend: "),
            Span::styled(
                format_query_timing_with_percent(breakdown.backend_reported_ms, timing.ui_ready_ms),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Retrieval: "),
            Span::styled(
                format_query_timing_with_percent(retrieval_total, timing.ui_ready_ms),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Answer: "),
            Span::styled(
                format_query_timing_with_percent(
                    response.answer_generation.duration_ms,
                    timing.ui_ready_ms,
                ),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Overhead: "),
            Span::styled(
                format_query_timing(Some(breakdown.transport_overhead_ms)),
                Style::default().fg(Theme::MUTED),
            ),
        ]),
        Line::from(vec![
            label_span("Lexical: "),
            Span::styled(
                format!(
                    "{} candidates, {}",
                    response.diagnostics.lexical_candidates,
                    format_query_timing_with_percent(
                        response.diagnostics.lexical_duration_ms,
                        retrieval_total,
                    )
                ),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Semantic: "),
            Span::styled(
                format!(
                    "{} [{}], {}",
                    response.diagnostics.semantic_candidates,
                    response.diagnostics.semantic_status,
                    format_query_timing_with_percent(
                        response.diagnostics.semantic_duration_ms,
                        retrieval_total,
                    )
                ),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
        Line::from(vec![
            label_span("Graph: "),
            Span::styled(
                format!(
                    "{} [{}], {}",
                    response.diagnostics.graph_candidates,
                    response.diagnostics.graph_status,
                    format_query_timing_with_percent(
                        response.diagnostics.graph_duration_ms,
                        retrieval_total,
                    )
                ),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Rerank/relation: "),
            Span::styled(
                format_query_timing_with_percent(
                    response.diagnostics.rerank_duration_ms,
                    retrieval_total,
                ),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Other: "),
            Span::styled(
                format_query_timing_with_percent(breakdown.retrieval_other_ms, retrieval_total),
                Style::default().fg(Theme::MUTED),
            ),
        ]),
    ]
}

pub(super) fn activity_row(item: &ActivityEntry) -> Row<'static> {
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

pub(super) fn agent_detail_lines(app: &App, snapshot: &AgentSnapshot) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(vec![
            label_span("Collected: "),
            Span::styled(
                format_timestamp_short(snapshot.collected_at),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
        Line::from(vec![
            label_span("Sessions: "),
            Span::styled(
                snapshot.sessions.len().to_string(),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Orphan ports: "),
            Span::styled(
                snapshot.orphan_ports.len().to_string(),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
    ];

    let selected_agent_cli = app
        .agents
        .agent_table_state
        .selected()
        .and_then(|i| snapshot.sessions.get(i))
        .map(|s| s.agent_cli);
    let matching_limits: Vec<_> = snapshot
        .rate_limits
        .iter()
        .filter(|rl| selected_agent_cli.is_none_or(|cli| cli == rl.source))
        .collect();
    if !matching_limits.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![section_span("Rate Limits")]));
        for rate_limit in &matching_limits {
            lines.push(Line::from(vec![
                label_span("Source: "),
                Span::styled(rate_limit.source.clone(), Style::default().fg(Theme::TEXT)),
            ]));
            if let Some(percent) = rate_limit.five_hour_pct {
                lines.push(quota_bar_line(
                    "5h",
                    percent,
                    20,
                    rate_limit_reset_label(rate_limit.five_hour_resets_at),
                ));
            }
            if let Some(percent) = rate_limit.seven_day_pct {
                lines.push(quota_bar_line(
                    "7d",
                    percent,
                    20,
                    rate_limit_reset_label(rate_limit.seven_day_resets_at),
                ));
            }
        }
    }

    if !snapshot.orphan_ports.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![section_span("Open Orphan Ports")]));
        for orphan in snapshot.orphan_ports.iter().take(6) {
            lines.push(Line::from(Span::styled(
                format!(
                    "- {}:{}  {}",
                    orphan.project_name, orphan.port, orphan.command
                ),
                Style::default().fg(Theme::WARNING),
            )));
        }
    }

    let Some(session) = snapshot.sessions.get(app.agents.agent_selected_index) else {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "No agent sessions are currently visible.",
            Style::default().fg(Theme::MUTED),
        )));
        return lines;
    };

    lines.push(Line::from(""));
    lines.push(Line::from(vec![section_span("Selected Session")]));
    lines.push(Line::from(vec![
        label_span("Project: "),
        Span::styled(
            session.project_name.clone(),
            Style::default().fg(Theme::TEXT),
        ),
        Span::raw("   "),
        label_span("Agent: "),
        Span::styled(
            session.agent_cli.to_string(),
            Style::default().fg(Theme::TEXT),
        ),
    ]));
    lines.push(Line::from(vec![
        label_span("Status: "),
        agent_status_span(&session.status),
        Span::raw("   "),
        label_span("PID: "),
        Span::styled(session.pid.to_string(), Style::default().fg(Theme::TEXT)),
    ]));
    lines.push(Line::from(vec![
        label_span("Model: "),
        Span::styled(session.model.clone(), Style::default().fg(Theme::TEXT)),
    ]));
    lines.push(Line::from(vec![
        label_span("Session: "),
        Span::styled(session.session_id.clone(), Style::default().fg(Theme::TEXT)),
    ]));
    lines.push(Line::from(vec![
        label_span("CWD: "),
        Span::styled(session.cwd.clone(), Style::default().fg(Theme::TEXT)),
    ]));
    lines.push(Line::from(vec![
        label_span("Started: "),
        Span::styled(
            format_elapsed_from_started(session.started_at),
            Style::default().fg(Theme::TEXT),
        ),
        Span::raw("   "),
        label_span("Version: "),
        Span::styled(session.version.clone(), Style::default().fg(Theme::TEXT)),
    ]));
    lines.push(Line::from(vec![
        label_span("Context: "),
        Span::styled(
            format_context_percent(session.context_percent),
            context_percent_style(session.context_percent),
        ),
        Span::raw("   "),
        label_span("Tokens: "),
        Span::styled(
            format_token_count(session.total_tokens()),
            Style::default().fg(Theme::TEXT),
        ),
    ]));
    lines.push(usage_bar_line("Ctx", session.context_percent, 20, None));
    lines.push(Line::from(vec![
        label_span("Git: "),
        Span::styled(
            format!(
                "{}  +{} ~{}",
                session.git_branch, session.git_added, session.git_modified
            ),
            Style::default().fg(Theme::TEXT),
        ),
    ]));
    lines.push(Line::from(vec![
        label_span("Task: "),
        Span::styled(
            agent_task_summary(session),
            Style::default().fg(Theme::TEXT),
        ),
    ]));

    if !session.children.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![section_span("Child Processes")]));
        for child in session.children.iter().take(8) {
            lines.push(Line::from(Span::styled(
                format_agent_child(child),
                Style::default().fg(Theme::TEXT),
            )));
        }
    }

    lines
}

pub(super) fn activity_detail_lines(entry: &ActivityEntry) -> Vec<Line<'static>> {
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

pub(super) fn backend_activity_detail_lines(event: &ActivityEvent) -> Vec<Line<'static>> {
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

pub(super) fn activity_kv_line(label: &str, value: String) -> Line<'static> {
    Line::from(vec![
        label_span(format!("{label}: ")),
        Span::styled(value, Style::default().fg(Theme::TEXT)),
    ])
}

pub(super) fn format_query_filters(filters: &QueryFilters) -> String {
    let types = if filters.types.is_empty() {
        "types=all".to_string()
    } else {
        format!(
            "types={}",
            filters
                .types
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        )
    };
    let tags = if filters.tags.is_empty() {
        "tags=all".to_string()
    } else {
        format!("tags={}", filters.tags.join(","))
    };
    format!("{types} {tags}")
}

pub(super) fn truncate_activity_text(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

pub(super) fn activity_recorded_at(item: &ActivityEntry) -> DateTime<Utc> {
    match item {
        ActivityEntry::Backend(event) => event.recorded_at,
        ActivityEntry::Query(entry) => entry.recorded_at,
    }
}

pub(super) fn activity_summary(item: &ActivityEntry) -> String {
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

pub(super) fn activity_tokens(item: &ActivityEntry) -> String {
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

pub(super) fn activity_duration(item: &ActivityEntry) -> String {
    match item {
        ActivityEntry::Backend(event) => event
            .duration_ms
            .map(format_compact_count)
            .unwrap_or_else(|| "-".to_string()),
        ActivityEntry::Query(entry) => format_compact_count(entry.duration_ms),
    }
}

pub(super) fn format_compact_count(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}m", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}k", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

pub(super) fn watcher_transition_status_message(
    summary: &str,
    health: &WatcherHealth,
    previous_health: Option<&WatcherHealth>,
    message: Option<&str>,
) -> String {
    if matches!(health, WatcherHealth::Healthy)
        && previous_health.is_some_and(|value| !matches!(value, WatcherHealth::Healthy))
    {
        format!("Watcher recovered: {summary}")
    } else if let Some(message) = message {
        format!("Watcher status: {summary} ({message})")
    } else {
        format!("Watcher status: {summary}")
    }
}

pub(super) fn embedding_base_url_is_default(provider: &str, base_url: &str) -> bool {
    // Keep in sync with mem_search::embedding_backend::default_base_url.
    let expected = match provider {
        "openai_compatible" | "openai" => "https://api.openai.com/v1",
        "ollama" => "http://127.0.0.1:11434/v1",
        "voyage" => "https://api.voyageai.com",
        "cohere" => "https://api.cohere.com",
        "gemini" => "https://generativelanguage.googleapis.com/v1beta",
        _ => return false,
    };
    base_url.trim_end_matches('/') == expected
}

pub(super) fn format_timestamp(value: Option<DateTime<Utc>>) -> String {
    value
        .map(format_timestamp_full)
        .unwrap_or_else(|| "n/a".to_string())
}

pub(super) fn format_timestamp_full(value: DateTime<Utc>) -> String {
    value
        .with_timezone(&Local)
        .format("%Y-%m-%d %H:%M:%S %Z")
        .to_string()
}

pub(super) fn format_timestamp_medium(value: DateTime<Utc>) -> String {
    value
        .with_timezone(&Local)
        .format("%Y-%m-%d %H:%M %Z")
        .to_string()
}

pub(super) fn format_timestamp_short(value: DateTime<Utc>) -> String {
    value
        .with_timezone(&Local)
        .format("%H:%M:%S %Z")
        .to_string()
}

pub(super) fn format_timestamp_timeline(value: DateTime<Utc>) -> String {
    value
        .with_timezone(&Local)
        .format("%m-%d %H:%M %Z")
        .to_string()
}

pub(super) fn display_filter(value: &str) -> String {
    if value.is_empty() {
        "none".to_string()
    } else {
        value.to_string()
    }
}

pub(super) fn format_automation_status(status: &mem_api::AutomationStatus) -> String {
    format!(
        "enabled={} mode={} dirty_files={} last_decision={}",
        status.enabled,
        match status.mode {
            mem_api::AutomationMode::Suggest => "suggest",
            mem_api::AutomationMode::Auto => "auto",
        },
        status.dirty_file_count.unwrap_or(0),
        status
            .last_decision
            .clone()
            .unwrap_or_else(|| "none".to_string())
    )
}

pub(super) fn split_root_area(area: Rect) -> [Rect; 4] {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(4),
        ])
        .split(area);
    [chunks[0], chunks[1], chunks[2], chunks[3]]
}

pub(super) fn split_memories_area(area: Rect) -> [Rect; 2] {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);
    [chunks[0], chunks[1]]
}

pub(super) fn current_frame_area() -> Option<Rect> {
    let (width, height) = crossterm::terminal::size().ok()?;
    Some(Rect::new(0, 0, width, height))
}

pub(super) fn default_frame_area() -> Rect {
    Rect::new(0, 0, 160, 48)
}

pub(super) fn memory_detail_max_scroll(app: &App, frame_area: Rect) -> u16 {
    let root = split_root_area(frame_area);
    let detail_area = split_memories_area(root[2])[1];
    let block = themed_focus_block(
        "Detail",
        app.memories.memories_focus == MemoriesFocus::Detail,
    );
    let inner = block.inner(detail_area);
    if inner.width == 0 || inner.height == 0 {
        return 0;
    }
    wrapped_line_count(&build_memory_detail_lines(app), inner.width)
        .saturating_sub(inner.height as usize) as u16
}

pub(super) fn help_max_scroll(tab: TabKind, frame_area: Rect) -> u16 {
    let root = split_root_area(frame_area);
    help_max_scroll_in_area(tab, root[2])
}

pub(super) fn help_max_scroll_in_area(tab: TabKind, area: Rect) -> u16 {
    let block = themed_block("Help");
    let inner = block.inner(area);
    if inner.width == 0 || inner.height == 0 {
        return 0;
    }
    wrapped_line_count(&tab_help_lines(tab), inner.width).saturating_sub(inner.height as usize)
        as u16
}

pub(super) fn tab_help_lines(tab: TabKind) -> Vec<Line<'static>> {
    render_markdown_lines(tab_help_markdown(tab))
}

pub(super) fn tab_help_markdown(tab: TabKind) -> &'static str {
    match tab {
        TabKind::Memories => {
            r#"# Memories Help

## Purpose
Browse canonical project memory, inspect one entry in detail, and maintain durable knowledge.

## Layout
- Left table: filtered memories with summary, type, status, confidence, and update time.
- Right detail: canonical text, embeddings, tags, sources, history, and related memories.
- Focus indicator: shows whether movement keys select memories or scroll detail.

## Controls
- `j/k` or `Up/Down`: select memories or scroll detail when detail focus is active.
- `Enter`: toggle list/detail focus. `Esc`: return to list focus.
- `PgUp/PgDn`, `Home`, `End`: scroll or jump detail.
- `/`: text filter. `g`: tag filter. `s`: status filter. `t`: type filter. `x`: clear filters.
- `c`: curate. `i`: reindex chunks. `e`: re-embed active space. `a`: archive low-value memories. `Shift+D`: delete. `Shift+H`: history.

## Workflows
- Filter by type or text, select a memory, then read canonical text and sources.
- Verify provenance before relying on a memory in implementation work.
- Use curation and Review rather than creating duplicate memories.

## Troubleshooting
- If detail is empty, move selection or refresh project state.
- If embeddings are missing, use `e` here or the Embeddings tab.
"#
        }
        TabKind::Agents => {
            r#"# Agents Help

## Purpose
Monitor live coding-agent sessions across projects, including process state, token pressure, context usage, rate limits, and active work.

## Layout
- Session table: detected Codex and Claude sessions, preferring the current project when possible.
- Detail pane: model, status, transcript, ports, child processes, current task, context budget, and rate limits.
- Auto-refresh: fast while this tab is visible, slower while hidden.

## Controls
- `j/k` or `Up/Down`: select a session.
- `PgUp/PgDn`: scroll details. `Home`: jump to top.

## Workflows
- Check which agent owns a watcher or whether a session is active, idle, stale, or over budget.
- Inspect context and rate-limit bars before adding more work to a busy session.
- Use process and port details to diagnose stuck local tools.

## Troubleshooting
- If no agents appear, check transcript permissions and watcher-manager state.
- If the wrong project is selected, restart the TUI from the intended repository.
"#
        }
        TabKind::Query => {
            r#"# Query Help

## Purpose
Ask questions against project memory and inspect the evidence, citations, timings, and graph connections behind the answer.

## Layout
- Question box: current or last submitted question.
- Query Result: answer, confidence, citations, evidence state, match count, and timing breakdown.
- Results/detail: ranked memories and why the selected memory matched.

## Controls
- `Enter`: start a new empty question from Query.
- `?`: jump to Query and start a question from anywhere.
- While editing: `Enter` submits, `Esc` cancels, `Up/Down` restores cached query history.
- `j/k`: move through results. `Shift+D`: delete selected result memory.

## Workflows
- Compare answer citations with numbered returned memories before trusting an answer.
- Use timing breakdown to locate slow lexical, semantic, graph, rerank, answer, or UI phases.
- Treat graph connections as retrieval explanations; citations still point to memories.

## Troubleshooting
- If evidence is insufficient, add or curate memory and ask again.
- If a restored history item is stale, press `Enter` to re-run it.
"#
        }
        TabKind::Activity => {
            r#"# Activity Help

## Purpose
Review persisted backend activity and generate get-up-to-speed briefings for new or returning agents.

## Layout
- Briefing panel: deterministic or LLM-generated continuity context plus LLM audit/debug status.
- Activity table: event time, kind, tokens, duration, and summary.
- Detail pane: selected event metadata, including query diagnostics, graph details, token usage, or curation counts.

## Controls
- `j/k` or `Up/Down`: select activity.
- `PgUp/PgDn`: scroll detail. `Home`: jump to top.
- `g`: deterministic briefing. `Shift+L`: LLM briefing. `r`: refresh.
- `Shift+A`: toggle LLM audit/debug logging in the running service and persist the config.

## Workflows
- Use this tab at handoff or after interruption.
- Enable audit briefly when you need to inspect service-side LLM prompts, then disable it after debugging.
- Inspect token and duration fields to understand cost and latency.
- Open query activities to see retrieval mode, graph behavior, and answer cost.

## Troubleshooting
- If activity is empty, perform a query, capture, curation, graph extraction, or embedding operation.
- If LLM briefing fails, use deterministic briefing and check Errors.
"#
        }
        TabKind::Errors => {
            r#"# Errors Help

## Purpose
Inspect backend diagnostics and session-local TUI errors with explanations and suggested fixes.

## Layout
- Error table: time, severity, source, component, and summary.
- Detail pane: explanation, fix hints, command suggestions, and raw metadata.
- Sources include TUI, service, watcher, manager, database, and providers.

## Controls
- `j/k` or `Up/Down`: select an error.
- `PgUp/PgDn`: scroll detail. `Home`: jump to top.
- `r`: refresh diagnostics.

## Workflows
- Open this tab when the footer shows warnings/errors or an operation fails.
- Prefer suggested `memory doctor` commands when shown.
- Use source/component to route fixes to config, service, watcher, manager, provider, or database.

## Troubleshooting
- If the table is empty but the footer is red, refresh and check live connection state.
- If provider errors repeat, verify API keys and backend readiness.
"#
        }
        TabKind::Project => {
            r#"# Project Help

## Purpose
Show high-level project health, counts, embedding/search state, recent activity, and automation status.

## Layout
- Scrollable report with memory totals, type/status breakdowns, backend health, watcher/automation state, and embedding coverage.
- It is a dashboard for deciding which specialist tab to inspect next.

## Controls
- `j/k` or `Up/Down`: scroll.
- `PgUp/PgDn`: page. `Home`: jump to top.
- `r`: refresh project state outside help.

## Workflows
- Start here for a quick project health check.
- Use counts to spot missing memory, missing embeddings, or pending curation.
- Follow up in Memories, Activity, Errors, Watchers, or Embeddings.

## Troubleshooting
- If counts look stale, refresh after backend work completes.
- If backend state is unavailable, check Errors and the footer.
"#
        }
        TabKind::Review => {
            r#"# Review Help

## Purpose
Review replacement proposals so duplicate or superseded memories can be approved or rejected safely.

## Layout
- Proposal list: pending replacement candidates.
- Detail pane: target, candidate, policy, score, reasons, source overlap, and canonical text comparison.
- Replacement policy controls how aggressively curation proposes or applies replacements.

## Controls
- `j/k`, `Up/Down`, `[` and `]`: move through proposals.
- `PgUp/PgDn`: jump by page. `Home/End`: first/last proposal.
- `y`: approve. `n`: reject. `p`: cycle policy. `r`: refresh.

## Workflows
- Approve only when the candidate is clearly better and provenance remains valid.
- Reject lexical or ambiguous matches that would lose context.
- Change policy deliberately; stricter policies reduce replacement noise.

## Troubleshooting
- No proposals means no pending candidates or conservative policy.
- If approval fails, check Errors and refresh.
"#
        }
        TabKind::Watchers => {
            r#"# Watchers Help

## Purpose
Show project watchers, heartbeat state, agent ownership, service identity, restart attempts, and recovery behavior.

## Layout
- Scrollable watcher report.
- Each watcher shows health, mode, repo root, watcher id, owner agent/session/pid, host service, heartbeat, and restart attempts.
- Managed watchers belong to agent sessions; manual watchers were started directly.

## Controls
- `j/k` or `Up/Down`: scroll.
- `PgUp/PgDn`: page. `Home`: jump to top.
- `r`: refresh project state outside help.

## Workflows
- Use this tab when captures are not appearing or watcher health is degraded.
- Check owner/session and stale heartbeat before restarting anything.
- Use restart attempts to distinguish transient restarts from repeated failures.

## Troubleshooting
- If a managed watcher stays stale, check Manager footer and Errors.
- If only manual watchers exist, start through the manager-integrated path.
"#
        }
        TabKind::Embeddings => {
            r#"# Embeddings Help

## Purpose
Inspect embedding backends, compare per-project coverage, switch semantic search, and backfill missing vectors.

## Layout
- Header: active backend, create setting, ready/not-ready counts, and operation status.
- Table: backend name, provider, model, create flag, base URL, chunk count, and memory count.
- `*` marks active. `!` marks a backend that did not resolve at startup.

## Controls
- `j/k` or `Up/Down`: select backend.
- `Enter`: activate selected backend, or deactivate when selected backend is active.
- `c`: toggle automatic embedding creation.
- `e`: create missing embeddings for selected backend.
- `I`: rebuild chunks and embeddings for all configured backends.
- `r`: refresh backend list and counts.

## Workflows
- Use `e` for normal missing-embedding backfill.
- Use `I` only when chunks need rebuilding or all backends should be refreshed.
- Switch active backend after both spaces are populated to compare semantic retrieval.

## Troubleshooting
- If a backend has `!`, fix API key/model config and restart service.
- If counts differ, run `e` on the lower-coverage backend.
"#
        }
        TabKind::Resume => {
            r#"# Resume Help

## Purpose
Get back into flow after interruption with checkpoint, current thread, next step, recent changes, attention items, and durable context.

## Layout
- Scrollable briefing with checkpoint metadata, current thread, next step, change summary, attention items, context memories, and recent activity.
- Loading and error lines appear at the top.

## Controls
- `j/k` or `Up/Down`: scroll.
- `PgUp/PgDn`: page. `Home`: jump to top.
- `r`: refresh resume context outside help.

## Workflows
- Open this first when returning to a task or handing work to another agent.
- Use the next-step section as the immediate continuation point.
- Follow context references into Memories or Query for provenance.

## Troubleshooting
- If there is no checkpoint, save one before leaving future sessions.
- If resume feels stale, refresh after recent activity or curation completes.
"#
        }
    }
}

pub(super) fn wrapped_line_count(lines: &[Line<'_>], width: u16) -> usize {
    if width == 0 {
        return 0;
    }
    let width = width as usize;
    lines
        .iter()
        .map(|line| {
            let line_width = line.width();
            if line_width == 0 {
                1
            } else {
                line_width.div_ceil(width)
            }
        })
        .sum()
}

pub(super) fn accent_span(value: impl Into<String>) -> Span<'static> {
    Span::styled(
        value.into(),
        Style::default()
            .fg(Theme::ACCENT)
            .add_modifier(Modifier::BOLD),
    )
}

pub(super) fn label_span(value: impl Into<String>) -> Span<'static> {
    Span::styled(
        value.into(),
        Style::default()
            .fg(Theme::ACCENT_STRONG)
            .add_modifier(Modifier::BOLD),
    )
}

pub(super) fn section_span(value: impl Into<String>) -> Span<'static> {
    Span::styled(
        value.into(),
        Style::default()
            .fg(Theme::TITLE)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
    )
}

pub(super) fn activity_kind_span(kind: &ActivityKind) -> Span<'static> {
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
    };
    Span::styled(
        label,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

pub(super) fn plan_activity_action_span(action: &PlanActivityAction) -> Span<'static> {
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

pub(super) fn watcher_health_span(health: &WatcherHealth) -> Span<'static> {
    let (label, color) = match health {
        WatcherHealth::Healthy => ("healthy", Theme::SUCCESS),
        WatcherHealth::Stale => ("stale", Theme::WARNING),
        WatcherHealth::Restarting => ("restarting", Theme::ACCENT_STRONG),
        WatcherHealth::Failed => ("failed", Theme::DANGER),
    };
    Span::styled(
        label,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

pub(super) fn watcher_health_label(health: &WatcherHealth) -> &'static str {
    match health {
        WatcherHealth::Healthy => "healthy",
        WatcherHealth::Stale => "stale",
        WatcherHealth::Restarting => "restarting",
        WatcherHealth::Failed => "failed",
    }
}

pub(super) fn query_match_span(kind: &QueryMatchKind) -> Span<'static> {
    let (label, color) = match kind {
        QueryMatchKind::Lexical => ("lexical", Theme::ACCENT_STRONG),
        QueryMatchKind::Semantic => ("semantic", Theme::SUCCESS),
        QueryMatchKind::Hybrid => ("hybrid", Theme::ACCENT),
    };
    Span::styled(
        label,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

pub(super) fn activity_entry_kind_span(item: &ActivityEntry) -> Span<'static> {
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

pub(super) fn status_span(value: &str) -> Span<'static> {
    let color = match value {
        "active" | "ok" | "up" => Theme::SUCCESS,
        "archived" | "unknown" => Theme::WARNING,
        _ => Theme::DANGER,
    };
    Span::styled(
        value.to_string(),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

pub(super) fn service_span(value: &str) -> Span<'static> {
    let color = match value {
        "ok" | "up" => Theme::SUCCESS,
        "unknown" => Theme::WARNING,
        _ => Theme::DANGER,
    };
    Span::styled(
        value.to_string(),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

pub(super) fn tui_status_label(app: &App) -> &'static str {
    if app.service.restart_notice.is_some() {
        return "restart";
    }
    match app.chrome.ui_status {
        UiStatus::Loading => "loading",
        UiStatus::Busy => "busy",
        UiStatus::Ready => "ready",
        UiStatus::Restart => "restart",
        UiStatus::Error => "error",
    }
}

pub(super) fn tui_status_color(app: &App) -> Color {
    if app.service.restart_notice.is_some() {
        return Theme::DANGER;
    }
    match app.chrome.ui_status {
        UiStatus::Loading => Theme::ACCENT,
        UiStatus::Busy => Theme::ACCENT_STRONG,
        UiStatus::Ready => Theme::SUCCESS,
        UiStatus::Restart => Theme::DANGER,
        UiStatus::Error => Theme::DANGER,
    }
}

pub(super) fn tui_status_detail(app: &App) -> Option<String> {
    let count = error_count(app);
    (count > 0).then(|| format!("{count} error{}", if count == 1 { "" } else { "s" }))
}

pub(super) fn service_status_label(app: &App) -> &'static str {
    if !app.service.health_ok {
        "down"
    } else {
        let is_relay = matches!(app.service.service_role.as_deref(), Some("relay"));
        let database_status = app
            .service
            .service_database_state
            .as_deref()
            .unwrap_or(app.meta.overview.database_status.as_str());
        let service_status = app
            .service
            .service_health_state
            .as_deref()
            .unwrap_or(app.meta.overview.service_status.as_str());
        if !is_relay && !matches!(database_status, "ok" | "up") {
            return "degraded";
        }
        match service_status {
            "ok" | "up" => "up",
            "unknown" => "unknown",
            _ => "degraded",
        }
    }
}

pub(super) fn service_status_color(app: &App) -> Color {
    match service_status_label(app) {
        "up" => Theme::SUCCESS,
        "unknown" => Theme::WARNING,
        "degraded" => Theme::WARNING,
        _ => Theme::DANGER,
    }
}

pub(super) fn service_status_detail(app: &App) -> Option<String> {
    if !app.service.health_ok {
        return None;
    }
    let mut parts = Vec::new();
    if let Some(role) = app.service.service_role.as_deref() {
        parts.push(role.to_string());
    }
    let is_relay = matches!(app.service.service_role.as_deref(), Some("relay"));
    let database_status = app
        .service
        .service_database_state
        .as_deref()
        .unwrap_or(app.meta.overview.database_status.as_str());
    if !is_relay && !matches!(database_status, "ok" | "up") {
        parts.push(format!("db {database_status}"));
    }
    (!parts.is_empty()).then_some(parts.join(", "))
}

pub(super) fn manager_status_label(app: &App) -> &'static str {
    match app
        .service
        .manager_status
        .as_ref()
        .map(|status| status.state)
    {
        Some(ManagerState::Active) => "active",
        Some(ManagerState::Installed) => "installed",
        Some(ManagerState::Off) => "off",
        Some(ManagerState::Error) => "error",
        None => "unknown",
    }
}

pub(super) fn manager_status_color(app: &App) -> Color {
    match manager_status_label(app) {
        "active" => Theme::SUCCESS,
        "installed" => Theme::WARNING,
        "off" => Theme::MUTED,
        "error" => Theme::DANGER,
        _ => Theme::WARNING,
    }
}

pub(super) fn manager_status_detail(app: &App) -> Option<String> {
    let status = app.service.manager_status.as_ref()?;
    let mut parts = Vec::new();
    if let Some(mode) = status.mode {
        parts.push(match mode {
            ManagerMode::Service => "service".to_string(),
            ManagerMode::Foreground => "manual".to_string(),
        });
    }
    if let Some(runtime_mode) = &status.runtime_mode {
        parts.push(runtime_mode.clone());
    }
    if let Some(reason) = &status.last_reconcile_reason {
        parts.push(format!("last {reason}"));
    }
    parts.push(format!(
        "{} session{}",
        status.tracked_sessions,
        if status.tracked_sessions == 1 {
            ""
        } else {
            "s"
        }
    ));
    if status.warning_count > 0 {
        parts.push(format!("{} warn", status.warning_count));
    }
    if status.event_count > 0 || status.fallback_scan_count > 0 {
        parts.push(format!(
            "{} events, {} fallback",
            status.event_count, status.fallback_scan_count
        ));
    }
    Some(parts.join(", "))
}

pub(super) fn watcher_bar_status_label(app: &App) -> &'static str {
    if !app.service.health_ok {
        return "unknown";
    }

    let Some(summary) = &app.meta.overview.watchers else {
        return "none";
    };

    if summary.unhealthy_count > 0 {
        "degraded"
    } else if summary.active_count > 0 {
        "ok"
    } else {
        "none"
    }
}

pub(super) fn watcher_bar_status_color(app: &App) -> Color {
    match watcher_bar_status_label(app) {
        "ok" => Theme::SUCCESS,
        "none" => Theme::MUTED,
        "unknown" => Theme::WARNING,
        "degraded" => Theme::WARNING,
        _ => Theme::TEXT,
    }
}

pub(super) fn watcher_bar_status_detail(app: &App) -> Option<String> {
    let summary = app.meta.overview.watchers.as_ref()?;
    if summary.unhealthy_count > 0 {
        Some(format!("{} unhealthy", summary.unhealthy_count))
    } else if summary.active_count > 0 {
        Some(format!("{} active", summary.active_count))
    } else {
        None
    }
}

pub(super) fn memory_type_span(memory_type: &MemoryType) -> Span<'static> {
    let label = memory_type.to_string();
    memory_type_span_from_label(&label)
}

pub(super) fn memory_type_span_from_label(label: &str) -> Span<'static> {
    let color = match label {
        "architecture" => Color::Rgb(120, 190, 255),
        "convention" => Color::Rgb(149, 220, 180),
        "decision" => Color::Rgb(255, 205, 120),
        "incident" => Color::Rgb(255, 140, 140),
        "debugging" => Color::Rgb(255, 170, 110),
        "environment" => Color::Rgb(190, 170, 255),
        "domain_fact" => Color::Rgb(130, 225, 220),
        "documentation" => Color::Rgb(170, 210, 255),
        "plan" => Color::Rgb(255, 120, 200),
        "implementation" => Color::Rgb(120, 230, 140),
        "refactor" => Color::Rgb(130, 220, 160),
        "all" => Theme::TEXT,
        _ => Theme::TEXT,
    };
    Span::styled(
        label.to_string(),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

pub(super) fn agent_status_span(status: &AgentSessionStatus) -> Span<'static> {
    let (label, color) = match status {
        AgentSessionStatus::Working => ("working", Theme::SUCCESS),
        AgentSessionStatus::Waiting => ("waiting", Theme::WARNING),
        AgentSessionStatus::Done => ("done", Theme::MUTED),
    };
    Span::styled(
        label.to_string(),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

pub(super) fn context_percent_style(percent: f64) -> Style {
    let color = if percent >= 90.0 {
        Theme::DANGER
    } else if percent >= 70.0 {
        Theme::WARNING
    } else {
        Theme::SUCCESS
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

pub(super) fn format_context_percent(percent: f64) -> String {
    if percent.is_finite() && percent > 100.0 {
        "100%+".to_string()
    } else {
        format!("{percent:.0}%")
    }
}

pub(super) fn normalized_percent(percent: f64) -> f64 {
    if !percent.is_finite() {
        0.0
    } else {
        percent.clamp(0.0, 100.0)
    }
}

pub(super) fn filled_bar_cells(percent: f64, width: usize) -> usize {
    let width = width.max(1);
    let normalized = normalized_percent(percent);
    ((normalized / 100.0) * width as f64).round() as usize
}

pub(super) fn remaining_bar_cells(percent_used: f64, width: usize) -> usize {
    let width = width.max(1);
    let remaining = 100.0 - normalized_percent(percent_used);
    ((remaining / 100.0) * width as f64).round() as usize
}

pub(super) fn interpolate_theme_color(start: Color, end: Color, factor: f64) -> Color {
    let factor = factor.clamp(0.0, 1.0);
    match (start, end) {
        (Color::Rgb(sr, sg, sb), Color::Rgb(er, eg, eb)) => {
            let lerp =
                |s: u8, e: u8| -> u8 { (s as f64 + (e as f64 - s as f64) * factor).round() as u8 };
            Color::Rgb(lerp(sr, er), lerp(sg, eg), lerp(sb, eb))
        }
        _ => end,
    }
}

pub(super) fn context_gradient_color(percent: f64) -> Color {
    interpolate_theme_color(
        Theme::SUCCESS,
        Theme::DANGER,
        normalized_percent(percent) / 100.0,
    )
}

pub(super) fn usage_bar_line(
    label: &str,
    percent: f64,
    width: usize,
    suffix: Option<String>,
) -> Line<'static> {
    let width = width.max(1);
    let filled = filled_bar_cells(percent, width).min(width);
    let empty = width.saturating_sub(filled);
    let percent_color = context_gradient_color(percent);
    let mut spans = vec![label_span(format!("{label}: "))];
    for idx in 0..filled {
        let cell_percent = ((idx + 1) as f64 / width as f64) * 100.0;
        spans.push(Span::styled(
            "█",
            Style::default()
                .fg(context_gradient_color(cell_percent))
                .add_modifier(Modifier::BOLD),
        ));
    }
    spans.extend([
        Span::styled("░".repeat(empty), Style::default().fg(Theme::BORDER)),
        Span::raw(" "),
        Span::styled(
            format_context_percent(percent),
            Style::default()
                .fg(percent_color)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    if let Some(suffix) = suffix {
        spans.push(Span::raw("   "));
        spans.push(Span::styled(suffix, Style::default().fg(Theme::MUTED)));
    }
    Line::from(spans)
}

pub(super) fn quota_bar_line(
    label: &str,
    percent_used: f64,
    width: usize,
    suffix: Option<String>,
) -> Line<'static> {
    let width = width.max(1);
    let remaining_cells = remaining_bar_cells(percent_used, width).min(width);
    let used_cells = width.saturating_sub(remaining_cells);
    let remaining_percent = 100.0 - normalized_percent(percent_used);
    let remaining_style = context_percent_style(100.0 - remaining_percent);
    let mut spans = vec![
        label_span(format!("{label}: ")),
        Span::styled("█".repeat(remaining_cells), remaining_style),
        Span::styled("░".repeat(used_cells), Style::default().fg(Theme::BORDER)),
        Span::raw(" "),
        Span::styled(format!("{remaining_percent:.0}% left"), remaining_style),
    ];
    if let Some(suffix) = suffix {
        spans.push(Span::raw("   "));
        spans.push(Span::styled(suffix, Style::default().fg(Theme::MUTED)));
    }
    Line::from(spans)
}

pub(super) fn rate_limit_reset_label(resets_at: Option<u64>) -> Option<String> {
    resets_at.map(|resets_at| format!("resets {}", format_epoch_reset_time(resets_at)))
}

pub(super) fn format_epoch_reset_time(epoch_seconds: u64) -> String {
    let Some(timestamp) = DateTime::<Utc>::from_timestamp(epoch_seconds as i64, 0) else {
        return "n/a".to_string();
    };
    format_timestamp_short(timestamp)
}

pub(super) fn format_token_count(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

pub(super) fn agent_task_summary(session: &AgentSession) -> String {
    if let Some(task) = session.current_tasks.first() {
        task.clone()
    } else if !session.initial_prompt.is_empty() {
        session.initial_prompt.clone()
    } else if !session.first_assistant_text.is_empty() {
        session.first_assistant_text.clone()
    } else {
        "no current task".to_string()
    }
}

pub(super) fn format_agent_child(child: &AgentChildProcess) -> String {
    match child.port {
        Some(port) => format!(
            "- {}  {}  {}  :{}",
            child.pid,
            child.command,
            format_token_count(child.mem_kb / 1024),
            port
        ),
        None => format!(
            "- {}  {}  {}",
            child.pid,
            child.command,
            format_token_count(child.mem_kb / 1024)
        ),
    }
}

pub(super) fn format_elapsed_from_started(started_at: u64) -> String {
    if started_at == 0 {
        return "n/a".to_string();
    }
    let Some(started_at) = DateTime::<Utc>::from_timestamp_millis(started_at as i64) else {
        return "n/a".to_string();
    };
    let elapsed = Utc::now().signed_duration_since(started_at);
    if elapsed.num_seconds() < 60 {
        format!("{}s", elapsed.num_seconds().max(0))
    } else if elapsed.num_minutes() < 60 {
        format!("{}m", elapsed.num_minutes().max(0))
    } else {
        format!(
            "{}h {}m",
            elapsed.num_hours().max(0),
            elapsed.num_minutes().max(0) % 60
        )
    }
}

pub(super) fn confidence_style(confidence: f32) -> Style {
    let color = if confidence >= 0.8 {
        Theme::SUCCESS
    } else if confidence >= 0.5 {
        Theme::WARNING
    } else {
        Theme::DANGER
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

pub(super) fn metric_line<'a>(label: &str, value: Span<'a>) -> Line<'a> {
    Line::from(vec![
        Span::styled(
            format!("{label}: "),
            Style::default()
                .fg(Theme::ACCENT_STRONG)
                .add_modifier(Modifier::BOLD),
        ),
        value,
    ])
}

pub(super) fn skill_bundle_status_color(status: SkillBundleStatus) -> Color {
    match status {
        SkillBundleStatus::Ok => Theme::SUCCESS,
        SkillBundleStatus::Warn => Theme::WARNING,
        SkillBundleStatus::Error => Theme::DANGER,
    }
}

pub(super) fn status_message_style(app: &App) -> Style {
    let lowered = app.chrome.status_message.to_lowercase();
    let color = if lowered.contains("error") || lowered.contains("failed") {
        Theme::DANGER
    } else if lowered.contains("refresh")
        || lowered.contains("loaded")
        || lowered.contains("curated")
    {
        Theme::ACCENT
    } else {
        Theme::TEXT
    };
    Style::default().fg(color).bg(Theme::PANEL_ALT)
}

pub(super) fn draw(frame: &mut ratatui::Frame<'_>, app: &App) {
    frame.render_widget(
        Block::default().style(Style::default().bg(Theme::BACKGROUND)),
        frame.area(),
    );

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(4),
        ])
        .split(frame.area());

    let titles = VISIBLE_TABS
        .into_iter()
        .map(|tab| Line::from(Span::styled(tab.label(), Style::default().fg(Theme::TEXT))))
        .collect::<Vec<_>>();
    let title = match app.meta.profile {
        Profile::Dev => format!("Memory Layer TUI [dev] - project {}", app.project),
        Profile::Prod => format!("Memory Layer TUI - project {}", app.project),
    };
    let tabs = Tabs::new(titles)
        .select(app.active_tab.index())
        .block(themed_block(title).borders(Borders::ALL))
        .style(Style::default().fg(Theme::MUTED).bg(Theme::PANEL))
        .highlight_style(
            Style::default()
                .fg(Theme::SELECTION_FG)
                .bg(Theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_widget(tabs, chunks[0]);

    let control_line = if app.chrome.help.help_open {
        Line::from(vec![
            accent_span("back "),
            Span::styled("h/Esc  ", Style::default().fg(Theme::TEXT)),
            accent_span("scroll "),
            Span::styled("j/k PgUp/PgDn  ", Style::default().fg(Theme::TEXT)),
            accent_span("jump "),
            Span::styled("Home/End  ", Style::default().fg(Theme::TEXT)),
            Span::styled(
                format!("showing {} help", app.chrome.help.help_tab.label()),
                Style::default().fg(Theme::MUTED),
            ),
        ])
    } else {
        let mut spans = match app.active_tab {
            TabKind::Resume => vec![
                accent_span("refresh "),
                Span::styled("r  ", Style::default().fg(Theme::TEXT)),
                accent_span("scroll "),
                Span::styled("j/k PgUp/PgDn Home", Style::default().fg(Theme::TEXT)),
            ],
            TabKind::Memories => vec![
                accent_span("search=/ "),
                Span::styled(
                    display_filter(&app.filters.text),
                    Style::default().fg(Theme::TEXT),
                ),
                Span::raw("  "),
                accent_span("tag=g "),
                Span::styled(
                    display_filter(&app.filters.tag),
                    Style::default().fg(Theme::TEXT),
                ),
                Span::raw("  "),
                accent_span("status=s "),
                status_span(app.filters.status.label()),
                Span::raw("  "),
                accent_span("type=t "),
                memory_type_span_from_label(app.filters.memory_type.label()),
                Span::raw("  "),
                accent_span("focus "),
                Span::styled(
                    match app.memories.memories_focus {
                        MemoriesFocus::List => "list",
                        MemoriesFocus::Detail => "detail",
                    },
                    Style::default()
                        .fg(match app.memories.memories_focus {
                            MemoriesFocus::List => Theme::ACCENT,
                            MemoriesFocus::Detail => Theme::ACCENT_STRONG,
                        })
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    match app.memories.memories_focus {
                        MemoriesFocus::List => {
                            "Enter=detail  j/k=select  PgUp/PgDn/Home/End=scroll  clear=x curate=c reindex=i reembed=e archive=a delete=D history=H"
                        }
                        MemoriesFocus::Detail => {
                            "Enter/Esc=list  j/k=scroll  PgUp/PgDn/Home/End=scroll  clear=x curate=c reindex=i reembed=e archive=a delete=D history=H"
                        }
                    },
                    Style::default().fg(Theme::MUTED),
                ),
            ],
            TabKind::Agents => vec![
                accent_span("auto-refresh "),
                Span::styled("2s  ", Style::default().fg(Theme::TEXT)),
                accent_span("select "),
                Span::styled("j/k  ", Style::default().fg(Theme::TEXT)),
                accent_span("detail "),
                Span::styled("PgUp/PgDn Home  ", Style::default().fg(Theme::TEXT)),
                Span::styled(
                    "read-only agent/session monitor inspired by abtop",
                    Style::default().fg(Theme::MUTED),
                ),
            ],
            TabKind::Query => vec![
                accent_span("new=Enter/? "),
                Span::styled(
                    display_filter(&current_query_display(app)),
                    Style::default().fg(Theme::TEXT),
                ),
                Span::raw("  "),
                Span::styled("j/k move result", Style::default().fg(Theme::MUTED)),
                Span::raw("  "),
                Span::styled(
                    "Up/Down history while editing",
                    Style::default().fg(Theme::MUTED),
                ),
            ],
            TabKind::Activity => vec![
                accent_span("brief "),
                Span::styled(
                    "g deterministic / L llm  ",
                    Style::default().fg(Theme::TEXT),
                ),
                accent_span("audit "),
                Span::styled("A  ", Style::default().fg(Theme::TEXT)),
                accent_span("refresh "),
                Span::styled("r  ", Style::default().fg(Theme::TEXT)),
                accent_span("move "),
                Span::styled("j/k PgUp/PgDn", Style::default().fg(Theme::TEXT)),
            ],
            TabKind::Errors => vec![
                accent_span("refresh "),
                Span::styled("r  ", Style::default().fg(Theme::TEXT)),
                accent_span("move "),
                Span::styled("j/k  ", Style::default().fg(Theme::TEXT)),
                accent_span("detail "),
                Span::styled("PgUp/PgDn Home  ", Style::default().fg(Theme::TEXT)),
                Span::styled(
                    "persisted backend diagnostics plus session-local TUI errors",
                    Style::default().fg(Theme::MUTED),
                ),
            ],
            TabKind::Project => vec![
                accent_span("scroll "),
                Span::styled("j/k  ", Style::default().fg(Theme::TEXT)),
                accent_span("page "),
                Span::styled("PgUp/PgDn  ", Style::default().fg(Theme::TEXT)),
                accent_span("jump "),
                Span::styled("Home", Style::default().fg(Theme::TEXT)),
            ],
            TabKind::Review => vec![
                accent_span("move "),
                Span::styled("j/k [ ]  ", Style::default().fg(Theme::TEXT)),
                accent_span("approve "),
                Span::styled("y  ", Style::default().fg(Theme::TEXT)),
                accent_span("reject "),
                Span::styled("n  ", Style::default().fg(Theme::TEXT)),
                accent_span("policy "),
                Span::styled("p  ", Style::default().fg(Theme::TEXT)),
                accent_span("refresh "),
                Span::styled("r", Style::default().fg(Theme::TEXT)),
            ],
            TabKind::Watchers => vec![
                accent_span("scroll "),
                Span::styled("j/k  ", Style::default().fg(Theme::TEXT)),
                accent_span("page "),
                Span::styled("PgUp/PgDn  ", Style::default().fg(Theme::TEXT)),
                accent_span("jump "),
                Span::styled("Home", Style::default().fg(Theme::TEXT)),
            ],
            TabKind::Embeddings => vec![
                accent_span("move "),
                Span::styled("j/k  ", Style::default().fg(Theme::TEXT)),
                accent_span("toggle "),
                Span::styled("Enter  ", Style::default().fg(Theme::TEXT)),
                accent_span("create "),
                Span::styled("c  ", Style::default().fg(Theme::TEXT)),
                accent_span("embed "),
                Span::styled("e  ", Style::default().fg(Theme::TEXT)),
                accent_span("reindex "),
                Span::styled("I  ", Style::default().fg(Theme::TEXT)),
                accent_span("refresh "),
                Span::styled("r", Style::default().fg(Theme::TEXT)),
            ],
        };
        spans.push(Span::raw("  "));
        spans.push(accent_span("help "));
        spans.push(Span::styled("h", Style::default().fg(Theme::TEXT)));
        Line::from(spans)
    };
    let filter_bar = Paragraph::new(vec![control_line])
        .style(Style::default().bg(Theme::PANEL_ALT))
        .block(themed_block(if app.chrome.help.help_open {
            "Help Controls"
        } else {
            match &app.chrome.input_mode {
                InputMode::Normal => "Controls",
                InputMode::Search(value) => {
                    if value.is_empty() {
                        "Search Input"
                    } else {
                        "Search Input (editing)"
                    }
                }
                InputMode::Tag(value) => {
                    if value.is_empty() {
                        "Tag Filter Input"
                    } else {
                        "Tag Filter Input (editing)"
                    }
                }
                InputMode::Query(value) => {
                    if value.is_empty() {
                        "Query Input"
                    } else {
                        "Query Input (editing)"
                    }
                }
            }
        }));
    frame.render_widget(filter_bar, chunks[1]);

    if app.chrome.help.help_open {
        draw_help_tab(frame, app, chunks[2]);
    } else if app.service.health_ok {
        let tab_ctx = TabRenderContext::new(app);
        match app.active_tab {
            TabKind::Resume => draw_resume_tab(frame, &tab_ctx, chunks[2]),
            TabKind::Memories => draw_memories_tab(frame, &tab_ctx, chunks[2]),
            TabKind::Agents => draw_agents_tab(frame, &tab_ctx, chunks[2]),
            TabKind::Query => draw_query_tab(frame, &tab_ctx, chunks[2]),
            TabKind::Activity => draw_activity_tab(frame, &tab_ctx, chunks[2]),
            TabKind::Errors => draw_errors_tab(frame, &tab_ctx, chunks[2]),
            TabKind::Project => draw_project_tab(frame, &tab_ctx, chunks[2]),
            TabKind::Review => draw_review_tab(frame, &tab_ctx, chunks[2]),
            TabKind::Watchers => draw_watchers_tab(frame, &tab_ctx, chunks[2]),
            TabKind::Embeddings => draw_embeddings_tab(frame, &tab_ctx, chunks[2]),
        }
    } else {
        draw_backend_recovery(frame, app, chunks[2]);
    }

    let footer_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Length(1)])
        .split(chunks[3]);

    let footer = Paragraph::new(app.chrome.status_message.clone())
        .style(status_message_style(app))
        .wrap(Wrap { trim: false })
        .block(themed_block("Status"));
    frame.render_widget(footer, footer_chunks[0]);
    draw_bottom_status_bar(frame, app, footer_chunks[1]);
}

pub(super) fn draw_bottom_status_bar(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    frame.render_widget(
        Block::default().style(Style::default().bg(Theme::PANEL_ALT)),
        area,
    );

    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
        ])
        .split(area);

    frame.render_widget(
        Paragraph::new(component_status_line(
            "TUI",
            &app.meta.versions.mem_cli,
            tui_status_label(app),
            tui_status_color(app),
            tui_status_detail(app),
        ))
        .style(Style::default().bg(Theme::PANEL_ALT)),
        sections[0],
    );
    frame.render_widget(
        Paragraph::new(component_status_line(
            "Service",
            &app.meta.versions.mem_service,
            service_status_label(app),
            service_status_color(app),
            service_status_detail(app),
        ))
        .style(Style::default().bg(Theme::PANEL_ALT)),
        sections[1],
    );

    frame.render_widget(
        Paragraph::new(component_status_line(
            "Manager",
            &app.meta.versions.watch_manager,
            manager_status_label(app),
            manager_status_color(app),
            manager_status_detail(app),
        ))
        .style(Style::default().bg(Theme::PANEL_ALT)),
        sections[2],
    );

    frame.render_widget(
        Paragraph::new(component_status_line(
            "Watchers",
            &app.meta.versions.memory_watch,
            watcher_bar_status_label(app),
            watcher_bar_status_color(app),
            watcher_bar_status_detail(app),
        ))
        .style(Style::default().bg(Theme::PANEL_ALT)),
        sections[3],
    );

    frame.render_widget(
        Paragraph::new(component_status_line(
            "Skills",
            &app.meta.skill_inventory.bundle_version,
            app.meta.skill_inventory.status.label(),
            skill_bundle_status_color(app.meta.skill_inventory.status),
            Some(app.meta.skill_inventory.summary.clone()),
        ))
        .style(Style::default().bg(Theme::PANEL_ALT)),
        sections[4],
    );
}

pub(super) fn component_status_line<'a>(
    label: &'a str,
    version: &'a str,
    status: &'a str,
    status_color: Color,
    detail: Option<String>,
) -> Line<'a> {
    let mut spans = vec![
        Span::styled(
            format!("{label} "),
            Style::default()
                .fg(Theme::ACCENT_STRONG)
                .bg(Theme::PANEL_ALT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("v{version} "),
            Style::default().fg(Theme::TEXT).bg(Theme::PANEL_ALT),
        ),
        Span::styled(
            status.to_string(),
            Style::default()
                .fg(status_color)
                .bg(Theme::PANEL_ALT)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    if let Some(detail) = detail {
        spans.push(Span::styled(
            format!(" {detail}"),
            Style::default().fg(Theme::MUTED).bg(Theme::PANEL_ALT),
        ));
    }
    Line::from(spans)
}

pub(super) fn draw_backend_recovery(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    if app.service.backend_connection_state == BackendConnectionState::Connecting {
        draw_backend_connecting(frame, area);
        return;
    }

    let mut lines = vec![
        Line::from(Span::styled(
            "Memory Layer backend is unavailable.",
            Style::default()
                .fg(Theme::DANGER)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("The TUI could not reach /healthz on the configured backend."),
    ];
    if app.service.relay_discovery_enabled {
        lines.push(Line::from(
            "Relay discovery fallback is already enabled in shared config.",
        ));
        lines.push(Line::from(
            "If another Memory Layer backend is running on the local network, press r to retry.",
        ));
    } else {
        lines.push(Line::from(
            "Press b to enable relay discovery fallback and restart the shared backend.",
        ));
    }
    lines.push(Line::from("Press r to retry or q to quit."));

    let widget = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(themed_block("Backend Recovery"));
    frame.render_widget(widget, area);
}

pub(super) fn draw_backend_connecting(frame: &mut ratatui::Frame<'_>, area: Rect) {
    let lines = vec![
        Line::from(Span::styled(
            "Connecting to Memory Layer backend...",
            Style::default()
                .fg(Theme::ACCENT_STRONG)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("The TUI is waiting for the first backend health check to complete."),
        Line::from(
            "This can take a moment while the service starts, runs migrations, or reconnects.",
        ),
        Line::from(""),
        Line::from("Press q to quit."),
    ];

    let widget = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(themed_block("Backend Connection"));
    frame.render_widget(widget, area);
}

pub(super) fn draw_help_tab(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let max_scroll = help_max_scroll_in_area(app.chrome.help.help_tab, area);
    let scroll = app.chrome.help.help_scroll.min(max_scroll);
    let help = Paragraph::new(tab_help_lines(app.chrome.help.help_tab))
        .style(Style::default().bg(Theme::PANEL))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0))
        .block(themed_block(format!(
            "{} Help (scroll {}/{})",
            app.chrome.help.help_tab.label(),
            scroll,
            max_scroll
        )));
    frame.render_widget(help, area);
}
