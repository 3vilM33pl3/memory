use super::super::app::*;
use super::super::theme::{Theme, themed_block};
use super::{TabAction, TabContext, TabRenderContext};
use crossterm::event::Event;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Row, Table, Wrap},
};

pub(in crate::tui) fn draw_automations_tab(
    frame: &mut ratatui::Frame<'_>,
    ctx: &TabRenderContext<'_>,
    area: Rect,
) {
    let app = ctx.app;
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(44), Constraint::Percentage(56)])
        .split(area);

    let Some(snapshot) = app.automations.snapshot.as_ref() else {
        let message = app
            .automations
            .error
            .as_deref()
            .unwrap_or("No automation inventory loaded yet. Press r to refresh.");
        frame.render_widget(
            Paragraph::new(message)
                .wrap(Wrap { trim: false })
                .block(themed_block("Automations")),
            area,
        );
        return;
    };

    let selected = snapshot.items.get(app.automations.selected_index);
    let rows = snapshot.items.iter().map(|item| {
        let pending = snapshot
            .pending_approvals
            .iter()
            .filter(|approval| approval.loop_id == item.definition.loop_id)
            .count();
        Row::new(vec![
            item.definition.loop_id.clone(),
            effective_mode_label(item),
            effective_scope_label(item),
            latest_run_label(item),
            pending.to_string(),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(32),
            Constraint::Percentage(18),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(10),
        ],
    )
    .header(
        Row::new(vec!["Loop", "Mode", "Scope", "Last run", "Pend"]).style(
            Style::default()
                .fg(Theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(themed_block(format!(
        "Automations - {} loop(s), {} pending approval(s)",
        snapshot.items.len(),
        snapshot.pending_approvals.len()
    )))
    .row_highlight_style(
        Style::default()
            .bg(Theme::SELECTION_BG)
            .fg(Theme::SELECTION_FG)
            .add_modifier(Modifier::BOLD),
    );
    let mut table_state = app.automations.table_state.clone();
    frame.render_stateful_widget(table, chunks[0], &mut table_state);

    let detail = selected
        .map(|item| automation_detail_lines(item, snapshot))
        .unwrap_or_else(|| vec![Line::from("No loop automation is available.")]);
    let detail = Paragraph::new(detail)
        .wrap(Wrap { trim: false })
        .scroll((app.automations.detail_scroll, 0))
        .block(themed_block("Automation Detail"));
    frame.render_widget(detail, chunks[1]);
}

pub(in crate::tui) fn update(
    _event: &Event,
    _state: &mut AutomationsTabState,
    _ctx: &mut TabContext,
) -> TabAction {
    TabAction::None
}

fn automation_detail_lines(
    item: &AutomationListItem,
    snapshot: &AutomationSnapshot,
) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(vec![
            label_span("Loop: "),
            Span::styled(
                format!("{} v{}", item.definition.loop_id, item.definition.version),
                Style::default().fg(Theme::ACCENT_STRONG),
            ),
        ]),
        Line::from(vec![
            label_span("Name: "),
            Span::styled(
                item.definition.name.clone(),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
        Line::from(vec![
            label_span("Risk: "),
            Span::styled(
                item.definition.risk_level.as_str().to_string(),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Default: "),
            Span::styled(
                item.definition.default_mode.as_str().to_string(),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
        Line::from(vec![
            label_span("Description: "),
            Span::styled(
                item.definition.description.clone(),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
        Line::from(""),
    ];

    if let Some(state) = &snapshot.global_state {
        lines.push(Line::from(vec![
            label_span("Global kill switch: "),
            Span::styled(
                if state.kill_switch_enabled {
                    "on"
                } else {
                    "off"
                },
                Style::default().fg(if state.kill_switch_enabled {
                    Theme::DANGER
                } else {
                    Theme::SUCCESS
                }),
            ),
            Span::raw("   "),
            label_span("Updated: "),
            Span::styled(
                format_timestamp(Some(state.updated_at)),
                Style::default().fg(Theme::TEXT),
            ),
        ]));
    }

    lines.push(Line::from(Span::styled(
        "Effective Settings",
        Style::default()
            .fg(Theme::ACCENT)
            .add_modifier(Modifier::BOLD),
    )));
    if let Some(settings) = &item.effective_settings {
        lines.push(Line::from(vec![
            label_span("Enabled: "),
            Span::styled(
                settings.enabled.to_string(),
                Style::default().fg(if settings.enabled {
                    Theme::SUCCESS
                } else {
                    Theme::MUTED
                }),
            ),
            Span::raw("   "),
            label_span("Mode: "),
            Span::styled(
                settings.mode.as_str().to_string(),
                Style::default().fg(Theme::TEXT),
            ),
        ]));
        lines.push(Line::from(vec![
            label_span("Scope: "),
            Span::styled(
                format!("{} {}", settings.scope_type.as_str(), settings.scope_id),
                Style::default().fg(Theme::TEXT),
            ),
        ]));
        if !settings.blocked_reasons.is_empty() {
            lines.push(Line::from(vec![
                label_span("Blocked: "),
                Span::styled(
                    settings.blocked_reasons.join("; "),
                    Style::default().fg(Theme::WARNING),
                ),
            ]));
        }
        if let Some(paused_until) = settings.paused_until {
            lines.push(Line::from(vec![
                label_span("Paused until: "),
                Span::styled(
                    format_timestamp(Some(paused_until)),
                    Style::default().fg(Theme::TEXT),
                ),
            ]));
        }
        if let Some(snoozed_until) = settings.snoozed_until {
            lines.push(Line::from(vec![
                label_span("Snoozed until: "),
                Span::styled(
                    format_timestamp(Some(snoozed_until)),
                    Style::default().fg(Theme::TEXT),
                ),
            ]));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "Effective settings are unavailable for this loop.",
            Style::default().fg(Theme::MUTED),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Latest Run",
        Style::default()
            .fg(Theme::ACCENT)
            .add_modifier(Modifier::BOLD),
    )));
    if let Some(run) = &item.latest_run {
        lines.push(Line::from(vec![
            label_span("Status: "),
            Span::styled(
                run.status.as_str().to_string(),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Mode: "),
            Span::styled(
                run.mode.as_str().to_string(),
                Style::default().fg(Theme::TEXT),
            ),
        ]));
        lines.push(Line::from(vec![
            label_span("Started: "),
            Span::styled(
                format_timestamp(Some(run.started_at)),
                Style::default().fg(Theme::TEXT),
            ),
        ]));
        if let Some(finished_at) = run.finished_at {
            lines.push(Line::from(vec![
                label_span("Finished: "),
                Span::styled(
                    format_timestamp(Some(finished_at)),
                    Style::default().fg(Theme::TEXT),
                ),
            ]));
        }
        if let Some(summary) = &run.output_summary {
            lines.push(Line::from(vec![
                label_span("Output: "),
                Span::styled(summary.clone(), Style::default().fg(Theme::TEXT)),
            ]));
        }
        if !run.blocked_reasons.is_empty() {
            lines.push(Line::from(vec![
                label_span("Run blocked: "),
                Span::styled(
                    run.blocked_reasons.join("; "),
                    Style::default().fg(Theme::WARNING),
                ),
            ]));
        }
        lines.push(Line::from(vec![
            label_span("Run id: "),
            Span::styled(run.id.to_string(), Style::default().fg(Theme::MUTED)),
        ]));
    } else {
        lines.push(Line::from(Span::styled(
            "No runs recorded for this loop in the current project.",
            Style::default().fg(Theme::MUTED),
        )));
    }

    let approvals = snapshot
        .pending_approvals
        .iter()
        .filter(|approval| approval.loop_id == item.definition.loop_id)
        .collect::<Vec<_>>();
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!("Pending Approvals ({})", approvals.len()),
        Style::default()
            .fg(Theme::ACCENT)
            .add_modifier(Modifier::BOLD),
    )));
    if approvals.is_empty() {
        lines.push(Line::from(Span::styled(
            "No pending approval requests for this loop.",
            Style::default().fg(Theme::MUTED),
        )));
    } else {
        for approval in approvals.into_iter().take(8) {
            lines.push(Line::from(vec![
                label_span("- "),
                Span::styled(
                    format!("{}: {}", approval.action_type, approval.risk_reason),
                    Style::default().fg(Theme::TEXT),
                ),
            ]));
        }
    }

    if !snapshot.warnings.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Warnings",
            Style::default()
                .fg(Theme::WARNING)
                .add_modifier(Modifier::BOLD),
        )));
        for warning in snapshot.warnings.iter().take(8) {
            lines.push(Line::from(Span::styled(
                format!("- {warning}"),
                Style::default().fg(Theme::WARNING),
            )));
        }
    }

    lines
}

fn effective_mode_label(item: &AutomationListItem) -> String {
    item.effective_settings
        .as_ref()
        .map(|settings| settings.mode.as_str().to_string())
        .unwrap_or_else(|| format!("default {}", item.definition.default_mode.as_str()))
}

fn effective_scope_label(item: &AutomationListItem) -> String {
    item.effective_settings
        .as_ref()
        .map(|settings| settings.scope_type.as_str().to_string())
        .unwrap_or_else(|| "default".to_string())
}

fn latest_run_label(item: &AutomationListItem) -> String {
    item.latest_run
        .as_ref()
        .map(|run| run.status.as_str().to_string())
        .unwrap_or_else(|| "none".to_string())
}
