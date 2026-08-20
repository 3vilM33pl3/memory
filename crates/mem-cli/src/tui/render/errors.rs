// SPDX-License-Identifier: AGPL-3.0-or-later

// SPDX-License-Identifier: AGPL-3.0-or-later

use chrono::{DateTime, Utc};
use mem_record::{
    ActivityDetails, ActivityKind, DiagnosticInfo, DiagnosticSeverity, WatcherHealth,
};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Row},
};

#[allow(unused_imports)]
use super::*;
use crate::tui::{app::*, theme::Theme};

#[derive(Clone)]
pub(in crate::tui) struct ErrorItem {
    pub(in crate::tui) when: Option<DateTime<Utc>>,
    pub(in crate::tui) diagnostic: DiagnosticInfo,
}

pub(in crate::tui) fn collect_error_items(app: &App) -> Vec<ErrorItem> {
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

pub(in crate::tui) fn session_diagnostic(
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

pub(in crate::tui) fn error_count(app: &App) -> usize {
    collect_error_items(app).len()
}

pub(in crate::tui) fn error_row(item: &ErrorItem) -> Row<'static> {
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

pub(in crate::tui) fn error_detail_lines(item: &ErrorItem) -> Vec<Line<'static>> {
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

pub(in crate::tui) fn diagnostic_severity_label(severity: &DiagnosticSeverity) -> &'static str {
    match severity {
        DiagnosticSeverity::Info => "info",
        DiagnosticSeverity::Warning => "warn",
        DiagnosticSeverity::Error => "error",
    }
}

pub(in crate::tui) fn diagnostic_severity_color(severity: &DiagnosticSeverity) -> Color {
    match severity {
        DiagnosticSeverity::Info => Theme::ACCENT,
        DiagnosticSeverity::Warning => Theme::WARNING,
        DiagnosticSeverity::Error => Theme::DANGER,
    }
}
