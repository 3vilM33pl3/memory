use super::super::app::*;
use super::super::theme::{Theme, themed_block};
use super::{TabAction, TabContext, TabRenderContext};
use crossterm::event::{Event, KeyCode};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::Span,
    widgets::{Paragraph, Wrap},
};

pub(in crate::tui) fn draw_watchers_tab(
    frame: &mut ratatui::Frame<'_>,
    ctx: &TabRenderContext<'_>,
    area: Rect,
) {
    let app = ctx.app;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(10)])
        .split(area);

    let summary = Paragraph::new(vec![
        metric_line(
            "Watchers",
            Span::styled(watcher_summary_text(app), Style::default().fg(Theme::TEXT)),
        ),
        metric_line(
            "Guidance",
            Span::styled(
                "Use `memory watcher manager enable` on Linux, or `memory watcher enable --project <slug>` / `memory watcher run --project <slug>` for manual mode.",
                Style::default().fg(Theme::MUTED),
            ),
        ),
    ])
    .style(Style::default().bg(Theme::PANEL))
    .block(themed_block("Watcher Summary"));
    frame.render_widget(summary, chunks[0]);

    let detail = Paragraph::new(watcher_detail_lines(app))
        .scroll((app.watchers.watcher_scroll, 0))
        .wrap(Wrap { trim: false })
        .style(Style::default().bg(Theme::PANEL_ALT))
        .block(themed_block(format!(
            "Watchers (scroll {})",
            app.watchers.watcher_scroll
        )));
    frame.render_widget(detail, chunks[1]);
}

pub(in crate::tui) fn update(
    event: &Event,
    state: &mut WatchersTabState,
    _ctx: &mut TabContext,
) -> TabAction {
    match event {
        Event::Key(key) => match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                scroll_watchers(state, 1);
                TabAction::Redraw
            }
            KeyCode::Up | KeyCode::Char('k') => {
                scroll_watchers(state, -1);
                TabAction::Redraw
            }
            KeyCode::PageDown => {
                scroll_watchers(state, 8);
                TabAction::Redraw
            }
            KeyCode::PageUp => {
                scroll_watchers(state, -8);
                TabAction::Redraw
            }
            KeyCode::Home => {
                state.watcher_scroll = 0;
                TabAction::Redraw
            }
            _ => TabAction::None,
        },
        _ => TabAction::None,
    }
}

fn scroll_watchers(state: &mut WatchersTabState, delta: i16) {
    state.watcher_scroll = if delta.is_negative() {
        state.watcher_scroll.saturating_sub(delta.unsigned_abs())
    } else {
        state
            .watcher_scroll
            .saturating_add(u16::try_from(delta).unwrap_or(0))
    };
}
