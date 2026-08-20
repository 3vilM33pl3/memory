// SPDX-License-Identifier: AGPL-3.0-or-later

// SPDX-License-Identifier: AGPL-3.0-or-later

use mem_config::Profile;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph, Wrap},
};

#[allow(unused_imports)]
use super::*;
use crate::tui::{
    app::*,
    theme::{Theme, themed_block},
};

pub(in crate::tui) fn llm_audit_status_lines(app: &App) -> Vec<Line<'static>> {
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

pub(in crate::tui) fn tui_status_label(app: &App) -> &'static str {
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

pub(in crate::tui) fn tui_status_color(app: &App) -> Color {
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

pub(in crate::tui) fn tui_status_detail(app: &App) -> Option<String> {
    let count = error_count(app);
    (count > 0).then(|| format!("{count} error{}", if count == 1 { "" } else { "s" }))
}

pub(in crate::tui) fn service_status_label(app: &App) -> &'static str {
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

pub(in crate::tui) fn service_status_color(app: &App) -> Color {
    match service_status_label(app) {
        "up" => Theme::SUCCESS,
        "unknown" => Theme::WARNING,
        "degraded" => Theme::WARNING,
        _ => Theme::DANGER,
    }
}

pub(in crate::tui) fn service_status_detail(app: &App) -> Option<String> {
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
    if let Some(count) = app.service.offline_pending_count
        && count > 0
    {
        parts.push(format!("{count} offline pending"));
    }
    (!parts.is_empty()).then_some(parts.join(", "))
}

pub(in crate::tui) fn manager_status_label(app: &App) -> &'static str {
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

pub(in crate::tui) fn manager_status_color(app: &App) -> Color {
    match manager_status_label(app) {
        "active" => Theme::SUCCESS,
        "installed" => Theme::WARNING,
        "off" => Theme::MUTED,
        "error" => Theme::DANGER,
        _ => Theme::WARNING,
    }
}

pub(in crate::tui) fn manager_status_detail(app: &App) -> Option<String> {
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

pub(in crate::tui) fn status_message_style(app: &App) -> Style {
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

pub(in crate::tui) fn footer_height(profile: Profile) -> u16 {
    match profile {
        Profile::Dev => 5,
        Profile::Prod => 4,
    }
}

pub(in crate::tui) fn draw_dev_status_bar(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    frame.render_widget(
        Block::default().style(Style::default().bg(Theme::WARNING)),
        area,
    );
    let commit = app.meta.dev_commit_label.as_deref().unwrap_or("unknown");
    frame.render_widget(
        Paragraph::new(dev_status_line(commit))
            .style(Style::default().fg(Color::Black).bg(Theme::WARNING))
            .alignment(Alignment::Center),
        area,
    );
}

pub(in crate::tui) fn dev_status_line(commit: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            "DEV MODE",
            Style::default()
                .fg(Color::Rgb(120, 0, 0))
                .bg(Theme::WARNING)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "  commit=",
            Style::default()
                .fg(Color::Black)
                .bg(Theme::WARNING)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            commit.to_string(),
            Style::default()
                .fg(Color::Black)
                .bg(Theme::WARNING)
                .add_modifier(Modifier::BOLD),
        ),
    ])
}

pub(in crate::tui) fn draw_bottom_status_bar(
    frame: &mut ratatui::Frame<'_>,
    app: &App,
    area: Rect,
) {
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

pub(in crate::tui) fn component_status_line<'a>(
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

pub(in crate::tui) fn draw_backend_recovery(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
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

pub(in crate::tui) fn draw_backend_connecting(frame: &mut ratatui::Frame<'_>, area: Rect) {
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
