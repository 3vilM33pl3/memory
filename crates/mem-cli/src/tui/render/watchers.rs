// SPDX-License-Identifier: AGPL-3.0-or-later

// SPDX-License-Identifier: AGPL-3.0-or-later

use mem_record::WatcherHealth;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

#[allow(unused_imports)]
use super::*;
use crate::tui::{app::*, theme::Theme};

pub(in crate::tui) fn watcher_summary_text(app: &App) -> String {
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

pub(in crate::tui) fn watcher_detail_lines(app: &App) -> Vec<Line<'static>> {
    let Some(summary) = &app.meta.overview.watchers else {
        return vec![
            Line::from(Span::styled(
                "No watcher presence reported.",
                Style::default().fg(Theme::MUTED),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Start the manager with `memory watcher manager enable`, or use `memory watcher enable --project <slug>` / `memory watcher run --project <slug>` for manual mode.",
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
                "Start the manager with `memory watcher manager enable`, or use `memory watcher enable --project <slug>` / `memory watcher run --project <slug>` for manual mode.",
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

pub(in crate::tui) fn watcher_transition_status_message(
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

pub(in crate::tui) fn format_automation_status(status: &mem_record::AutomationStatus) -> String {
    format!(
        "enabled={} mode={} dirty_files={} last_decision={}",
        status.enabled,
        match status.mode {
            mem_record::AutomationMode::Suggest => "suggest",
            mem_record::AutomationMode::Auto => "auto",
        },
        status.dirty_file_count.unwrap_or(0),
        status
            .last_decision
            .clone()
            .unwrap_or_else(|| "none".to_string())
    )
}

pub(in crate::tui) fn watcher_health_span(health: &WatcherHealth) -> Span<'static> {
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

pub(in crate::tui) fn watcher_health_label(health: &WatcherHealth) -> &'static str {
    match health {
        WatcherHealth::Healthy => "healthy",
        WatcherHealth::Stale => "stale",
        WatcherHealth::Restarting => "restarting",
        WatcherHealth::Failed => "failed",
    }
}

pub(in crate::tui) fn watcher_bar_status_label(app: &App) -> &'static str {
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

pub(in crate::tui) fn watcher_bar_status_color(app: &App) -> Color {
    match watcher_bar_status_label(app) {
        "ok" => Theme::SUCCESS,
        "none" => Theme::MUTED,
        "unknown" => Theme::WARNING,
        "degraded" => Theme::WARNING,
        _ => Theme::TEXT,
    }
}

pub(in crate::tui) fn watcher_bar_status_detail(app: &App) -> Option<String> {
    let summary = app.meta.overview.watchers.as_ref()?;
    if summary.unhealthy_count > 0 {
        Some(format!("{} unhealthy", summary.unhealthy_count))
    } else if summary.active_count > 0 {
        Some(format!("{} active", summary.active_count))
    } else {
        None
    }
}
