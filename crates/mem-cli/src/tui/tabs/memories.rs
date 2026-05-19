use super::super::app::*;
use super::super::theme::{Theme, themed_focus_block};
use super::{TabAction, TabContext, TabRenderContext};
use crossterm::event::{Event, KeyCode};
use ratatui::{
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    widgets::{Paragraph, Row, Table, Wrap},
};

pub(in crate::tui) fn draw_memories_tab(
    frame: &mut ratatui::Frame<'_>,
    ctx: &TabRenderContext<'_>,
    area: Rect,
) {
    let app = ctx.app;
    let chunks = split_memories_area(area);

    let header = Row::new(["Summary", "Type", "Status", "Conf", "Updated"]).style(
        Style::default()
            .fg(Theme::ACCENT_STRONG)
            .bg(Theme::PANEL_ALT)
            .add_modifier(Modifier::BOLD),
    );
    let rows = app.memories.filtered_memories.iter().map(memory_row);
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(34),
            Constraint::Length(16),
            Constraint::Length(8),
            Constraint::Length(5),
            Constraint::Length(20),
        ],
    )
    .column_spacing(2)
    .header(header)
    .row_highlight_style(
        Style::default()
            .fg(Theme::SELECTION_FG)
            .bg(Theme::SELECTION_BG)
            .add_modifier(Modifier::BOLD),
    )
    .block(themed_focus_block(
        format!(
            "Memories (showing {} / {})",
            app.memories.filtered_memories.len(),
            app.memories.total_memories
        ),
        app.memories.memories_focus == MemoriesFocus::List,
    ));
    let mut state = app.memories.table_state.clone();
    frame.render_stateful_widget(table, chunks[0], &mut state);

    let detail_text = build_memory_detail_lines(app);
    let detail_block = themed_focus_block(
        match app.memories.memories_focus {
            MemoriesFocus::List => "Detail".to_string(),
            MemoriesFocus::Detail => "Detail Reader".to_string(),
        },
        app.memories.memories_focus == MemoriesFocus::Detail,
    );
    let detail_inner = detail_block.inner(chunks[1]);
    let max_scroll = if detail_inner.width == 0 || detail_inner.height == 0 {
        0
    } else {
        wrapped_line_count(&detail_text, detail_inner.width)
            .saturating_sub(detail_inner.height as usize) as u16
    };
    let detail = Paragraph::new(detail_text)
        .style(Style::default().bg(Theme::PANEL))
        .scroll((app.memories.memory_detail_scroll.min(max_scroll), 0))
        .wrap(Wrap { trim: false })
        .block(detail_block);
    frame.render_widget(detail, chunks[1]);
}

pub(in crate::tui) fn update(
    event: &Event,
    state: &mut MemoriesTabState,
    _ctx: &mut TabContext,
) -> TabAction {
    match event {
        Event::Key(key) => match key.code {
            KeyCode::PageDown => {
                state.memory_detail_scroll = state.memory_detail_scroll.saturating_add(8);
                TabAction::Redraw
            }
            KeyCode::PageUp => {
                state.memory_detail_scroll = state.memory_detail_scroll.saturating_sub(8);
                TabAction::Redraw
            }
            KeyCode::Home => {
                state.memory_detail_scroll = 0;
                TabAction::Redraw
            }
            KeyCode::Enter => {
                state.memories_focus = match state.memories_focus {
                    MemoriesFocus::List => MemoriesFocus::Detail,
                    MemoriesFocus::Detail => MemoriesFocus::List,
                };
                TabAction::Redraw
            }
            KeyCode::Esc => {
                state.memories_focus = MemoriesFocus::List;
                TabAction::Redraw
            }
            _ => TabAction::None,
        },
        _ => TabAction::None,
    }
}
