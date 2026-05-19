use super::super::app::*;
use super::super::theme::{Theme, themed_block};
use super::{TabAction, TabContext, TabRenderContext};
use crossterm::event::{Event, KeyCode};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Row, Table, Wrap},
};

pub(in crate::tui) fn draw_activity_tab(
    frame: &mut ratatui::Frame<'_>,
    ctx: &TabRenderContext<'_>,
    area: Rect,
) {
    let app = ctx.app;
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(11), Constraint::Min(8)])
        .split(area);

    let mut briefing_lines = activity_briefing_lines(app);
    briefing_lines.extend(llm_audit_status_lines(app));
    frame.render_widget(
        Paragraph::new(briefing_lines)
            .style(Style::default().bg(Theme::PANEL_ALT))
            .wrap(Wrap { trim: false })
            .block(themed_block("Get Up To Speed")),
        vertical[0],
    );

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(vertical[1]);

    let header = Row::new(["When", "Kind", "Tok", "Ms", "Summary"]).style(
        Style::default()
            .fg(Theme::ACCENT_STRONG)
            .bg(Theme::PANEL_ALT)
            .add_modifier(Modifier::BOLD),
    );
    let rows = app.activity.activity_events.iter().map(activity_row);
    let table = Table::new(
        rows,
        [
            Constraint::Length(16),
            Constraint::Length(11),
            Constraint::Length(7),
            Constraint::Length(6),
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
        "Activity ({})",
        app.activity.activity_events.len()
    )));
    let mut state = app.activity.activity_table_state.clone();
    frame.render_stateful_widget(table, chunks[0], &mut state);

    let detail_lines = if let Some(entry) = app
        .activity
        .activity_events
        .get(app.activity.activity_selected_index)
    {
        activity_detail_lines(entry)
    } else {
        vec![Line::from(Span::styled(
            "No activity yet. Keep the TUI open while queries, captures, curations, reindexing, re-embedding, archiving, or deletions happen for this project.",
            Style::default().fg(Theme::MUTED),
        ))]
    };

    let detail = Paragraph::new(detail_lines)
        .scroll((app.activity.activity_detail_scroll, 0))
        .style(Style::default().bg(Theme::PANEL))
        .wrap(Wrap { trim: false })
        .block(themed_block(format!(
            "Activity Detail (scroll {})",
            app.activity.activity_detail_scroll
        )));
    frame.render_widget(detail, chunks[1]);
}

pub(in crate::tui) fn update(
    event: &Event,
    state: &mut ActivityTabState,
    _ctx: &mut TabContext,
) -> TabAction {
    match event {
        Event::Key(key) => match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                move_activity_selection(state, 1);
                TabAction::Redraw
            }
            KeyCode::Up | KeyCode::Char('k') => {
                move_activity_selection(state, -1);
                TabAction::Redraw
            }
            KeyCode::PageDown => {
                state.activity_detail_scroll = state.activity_detail_scroll.saturating_add(8);
                TabAction::Redraw
            }
            KeyCode::PageUp => {
                state.activity_detail_scroll = state.activity_detail_scroll.saturating_sub(8);
                TabAction::Redraw
            }
            KeyCode::Home => {
                state.activity_detail_scroll = 0;
                TabAction::Redraw
            }
            _ => TabAction::None,
        },
        _ => TabAction::None,
    }
}

fn move_activity_selection(state: &mut ActivityTabState, delta: isize) {
    if state.activity_events.is_empty() {
        return;
    }
    let next = (state.activity_selected_index as isize + delta)
        .clamp(0, state.activity_events.len().saturating_sub(1) as isize) as usize;
    if next != state.activity_selected_index {
        state.activity_selected_index = next;
        state
            .activity_table_state
            .select(Some(state.activity_selected_index));
    }
}
