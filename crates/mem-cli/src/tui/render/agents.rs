// SPDX-License-Identifier: AGPL-3.0-or-later

// SPDX-License-Identifier: AGPL-3.0-or-later

use chrono::{DateTime, Utc};
use mem_agenttop::{
    AgentSession, AgentSnapshot, ChildProcess as AgentChildProcess,
    SessionStatus as AgentSessionStatus,
};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Row},
};

#[allow(unused_imports)]
use super::*;
use crate::tui::{app::*, theme::Theme};

pub(in crate::tui) fn agent_row(session: &AgentSession) -> Row<'static> {
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

pub(in crate::tui) fn agent_detail_lines(
    app: &App,
    snapshot: &AgentSnapshot,
) -> Vec<Line<'static>> {
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

pub(in crate::tui) fn agent_status_span(status: &AgentSessionStatus) -> Span<'static> {
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

pub(in crate::tui) fn context_percent_style(percent: f64) -> Style {
    let color = if percent >= 90.0 {
        Theme::DANGER
    } else if percent >= 70.0 {
        Theme::WARNING
    } else {
        Theme::SUCCESS
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

pub(in crate::tui) fn format_context_percent(percent: f64) -> String {
    if percent.is_finite() && percent > 100.0 {
        "100%+".to_string()
    } else {
        format!("{percent:.0}%")
    }
}

pub(in crate::tui) fn normalized_percent(percent: f64) -> f64 {
    if !percent.is_finite() {
        0.0
    } else {
        percent.clamp(0.0, 100.0)
    }
}

pub(in crate::tui) fn filled_bar_cells(percent: f64, width: usize) -> usize {
    let width = width.max(1);
    let normalized = normalized_percent(percent);
    ((normalized / 100.0) * width as f64).round() as usize
}

pub(in crate::tui) fn remaining_bar_cells(percent_used: f64, width: usize) -> usize {
    let width = width.max(1);
    let remaining = 100.0 - normalized_percent(percent_used);
    ((remaining / 100.0) * width as f64).round() as usize
}

pub(in crate::tui) fn interpolate_theme_color(start: Color, end: Color, factor: f64) -> Color {
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

pub(in crate::tui) fn context_gradient_color(percent: f64) -> Color {
    interpolate_theme_color(
        Theme::SUCCESS,
        Theme::DANGER,
        normalized_percent(percent) / 100.0,
    )
}

pub(in crate::tui) fn usage_bar_line(
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

pub(in crate::tui) fn quota_bar_line(
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

pub(in crate::tui) fn rate_limit_reset_label(resets_at: Option<u64>) -> Option<String> {
    resets_at.map(|resets_at| format!("resets {}", format_epoch_reset_time(resets_at)))
}

pub(in crate::tui) fn format_epoch_reset_time(epoch_seconds: u64) -> String {
    let Some(timestamp) = DateTime::<Utc>::from_timestamp(epoch_seconds as i64, 0) else {
        return "n/a".to_string();
    };
    format_timestamp_short(timestamp)
}

pub(in crate::tui) fn format_token_count(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

pub(in crate::tui) fn agent_task_summary(session: &AgentSession) -> String {
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

pub(in crate::tui) fn format_agent_child(child: &AgentChildProcess) -> String {
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

pub(in crate::tui) fn format_elapsed_from_started(started_at: u64) -> String {
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
