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

pub(in crate::tui) fn draw_errors_tab(
    frame: &mut ratatui::Frame<'_>,
    ctx: &TabRenderContext<'_>,
    area: Rect,
) {
    let app = ctx.app;
    let items = collect_error_items(app);
    let selected_index = app
        .errors
        .errors_selected_index
        .min(items.len().saturating_sub(1));
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(44), Constraint::Percentage(56)])
        .split(area);

    let header = Row::new(["When", "Sev", "Source", "Component", "Summary"]).style(
        Style::default()
            .fg(Theme::ACCENT_STRONG)
            .bg(Theme::PANEL_ALT)
            .add_modifier(Modifier::BOLD),
    );
    let rows = items.iter().map(error_row);
    let table = Table::new(
        rows,
        [
            Constraint::Length(16),
            Constraint::Length(7),
            Constraint::Length(12),
            Constraint::Length(13),
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
    .block(themed_block(format!("Errors ({})", items.len())));
    let mut state = app.errors.errors_table_state.clone();
    if items.is_empty() {
        state.select(None);
    } else {
        state.select(Some(selected_index));
    }
    frame.render_stateful_widget(table, chunks[0], &mut state);

    let lines = if let Some(item) = items.get(selected_index) {
        error_detail_lines(item)
    } else {
        vec![
            Line::from(Span::styled(
                "No diagnostics recorded for this project or TUI session.",
                Style::default().fg(Theme::SUCCESS),
            )),
            Line::from(Span::styled(
                "Provider errors, query failures, watcher failures, and TUI connection errors will appear here with fix hints.",
                Style::default().fg(Theme::MUTED),
            )),
        ]
    };
    let detail = Paragraph::new(lines)
        .scroll((app.errors.errors_detail_scroll, 0))
        .style(Style::default().bg(Theme::PANEL))
        .wrap(Wrap { trim: false })
        .block(themed_block(format!(
            "Error Detail (scroll {})",
            app.errors.errors_detail_scroll
        )));
    frame.render_widget(detail, chunks[1]);
}

pub(in crate::tui) fn update(
    event: &Event,
    state: &mut ErrorsTabState,
    ctx: &mut TabContext,
) -> TabAction {
    match event {
        Event::Key(key) => match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                move_error_selection(state, ctx.error_count, 1);
                TabAction::Redraw
            }
            KeyCode::Up | KeyCode::Char('k') => {
                move_error_selection(state, ctx.error_count, -1);
                TabAction::Redraw
            }
            KeyCode::PageDown => {
                state.errors_detail_scroll = state.errors_detail_scroll.saturating_add(8);
                TabAction::Redraw
            }
            KeyCode::PageUp => {
                state.errors_detail_scroll = state.errors_detail_scroll.saturating_sub(8);
                TabAction::Redraw
            }
            KeyCode::Home => {
                state.errors_detail_scroll = 0;
                TabAction::Redraw
            }
            _ => TabAction::None,
        },
        _ => TabAction::None,
    }
}

fn move_error_selection(state: &mut ErrorsTabState, len: usize, delta: isize) {
    if len == 0 {
        state.errors_selected_index = 0;
        state.errors_table_state.select(None);
        return;
    }
    let next = (state.errors_selected_index as isize + delta)
        .clamp(0, len.saturating_sub(1) as isize) as usize;
    if next != state.errors_selected_index {
        state.errors_selected_index = next;
        state.errors_table_state.select(Some(next));
        state.errors_detail_scroll = 0;
    }
}
