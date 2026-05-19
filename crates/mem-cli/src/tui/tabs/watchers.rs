use super::super::app::*;
use super::super::theme::{themed_block, Theme};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::Span,
    widgets::{Paragraph, Wrap},
};

pub(in crate::tui) fn draw_watchers_tab(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
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
