use super::super::app::*;
use super::super::theme::{themed_block, Theme};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Paragraph, Row, Table, Wrap},
};

pub(in crate::tui) fn draw_agents_tab(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
        .split(area);

    if app.agents.agent_loading && app.agents.agent_snapshot.is_none() {
        frame.render_widget(
            Paragraph::new("Loading agent sessions...")
                .style(Style::default().fg(Theme::ACCENT).bg(Theme::PANEL_ALT))
                .block(themed_block("Agents")),
            area,
        );
        return;
    }

    if let Some(error) = &app.agents.agent_error
        && app.agents.agent_snapshot.is_none()
    {
        frame.render_widget(
            Paragraph::new(format!("Agents unavailable: {error}"))
                .style(Style::default().fg(Theme::WARNING).bg(Theme::PANEL_ALT))
                .wrap(Wrap { trim: false })
                .block(themed_block("Agents")),
            area,
        );
        return;
    }

    let Some(snapshot) = &app.agents.agent_snapshot else {
        frame.render_widget(
            Paragraph::new("No agent data available yet.")
                .style(Style::default().fg(Theme::MUTED).bg(Theme::PANEL_ALT))
                .block(themed_block("Agents")),
            area,
        );
        return;
    };

    let header = Row::new(["Project", "Agent", "Status", "Tok", "Ctx", "Task"]).style(
        Style::default()
            .fg(Theme::ACCENT_STRONG)
            .bg(Theme::PANEL_ALT)
            .add_modifier(Modifier::BOLD),
    );
    let rows = snapshot.sessions.iter().map(agent_row);
    let table = Table::new(
        rows,
        [
            Constraint::Length(20),
            Constraint::Length(7),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(7),
            Constraint::Percentage(100),
        ],
    )
    .column_spacing(1)
    .header(header)
    .row_highlight_style(
        Style::default()
            .fg(Theme::SELECTION_FG)
            .bg(Theme::SELECTION_BG)
            .add_modifier(Modifier::BOLD),
    )
    .block(themed_block(format!(
        "Agents ({} sessions, {} orphan ports)",
        snapshot.sessions.len(),
        snapshot.orphan_ports.len()
    )));
    let mut state = app.agents.agent_table_state.clone();
    frame.render_stateful_widget(table, chunks[0], &mut state);

    let detail = Paragraph::new(agent_detail_lines(app, snapshot))
        .scroll((app.agents.agent_detail_scroll, 0))
        .wrap(Wrap { trim: false })
        .style(Style::default().bg(Theme::PANEL))
        .block(themed_block(format!(
            "Agent Detail (scroll {})",
            app.agents.agent_detail_scroll
        )));
    frame.render_widget(detail, chunks[1]);
}
