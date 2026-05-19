use super::super::app::*;
use super::super::theme::{Theme, themed_block};
use super::{TabAction, TabContext, TabRenderContext};
use crossterm::event::{Event, KeyCode};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Paragraph, Row, Table, Wrap},
};

pub(in crate::tui) fn draw_agents_tab(
    frame: &mut ratatui::Frame<'_>,
    ctx: &TabRenderContext<'_>,
    area: Rect,
) {
    let app = ctx.app;
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

pub(in crate::tui) fn update(
    event: &Event,
    state: &mut AgentsTabState,
    _ctx: &mut TabContext,
) -> TabAction {
    match event {
        Event::Key(key) => match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                move_agent_selection(state, 1);
                TabAction::Redraw
            }
            KeyCode::Up | KeyCode::Char('k') => {
                move_agent_selection(state, -1);
                TabAction::Redraw
            }
            KeyCode::PageDown => {
                state.agent_detail_scroll = state.agent_detail_scroll.saturating_add(8);
                TabAction::Redraw
            }
            KeyCode::PageUp => {
                state.agent_detail_scroll = state.agent_detail_scroll.saturating_sub(8);
                TabAction::Redraw
            }
            KeyCode::Home => {
                state.agent_detail_scroll = 0;
                TabAction::Redraw
            }
            _ => TabAction::None,
        },
        _ => TabAction::None,
    }
}

fn move_agent_selection(state: &mut AgentsTabState, delta: isize) {
    let Some(snapshot) = &state.agent_snapshot else {
        state.agent_selected_index = 0;
        state.agent_table_state.select(None);
        return;
    };
    let len = snapshot.sessions.len();
    if len == 0 {
        state.agent_selected_index = 0;
        state.agent_table_state.select(None);
        return;
    }
    let next = (state.agent_selected_index as isize + delta).clamp(0, len as isize - 1);
    state.agent_selected_index = next as usize;
    state
        .agent_table_state
        .select(Some(state.agent_selected_index));
}
