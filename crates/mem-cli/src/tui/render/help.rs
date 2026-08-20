// SPDX-License-Identifier: AGPL-3.0-or-later

// SPDX-License-Identifier: AGPL-3.0-or-later

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

#[allow(unused_imports)]
use super::*;
use crate::tui::{
    app::*,
    markdown::render_markdown_lines,
    theme::{Theme, themed_block},
};

pub(in crate::tui) fn append_resume_briefing_lines(lines: &mut Vec<Line<'static>>, briefing: &str) {
    for raw_line in briefing.lines() {
        let trimmed = raw_line.trim_end();
        if trimmed.is_empty() {
            lines.push(Line::from(""));
            continue;
        }

        let line = if let Some(heading) = trimmed.strip_prefix("### ") {
            Line::from(Span::styled(
                heading.to_string(),
                Style::default()
                    .fg(Theme::ACCENT_STRONG)
                    .add_modifier(Modifier::BOLD),
            ))
        } else {
            Line::from(Span::styled(
                trimmed.to_string(),
                Style::default().fg(Theme::TEXT),
            ))
        };
        lines.push(line);
    }
}

pub(in crate::tui) fn help_max_scroll(tab: TabKind, frame_area: Rect) -> u16 {
    let root = split_root_area(frame_area);
    help_max_scroll_in_area(tab, root[2])
}

pub(in crate::tui) fn help_max_scroll_in_area(tab: TabKind, area: Rect) -> u16 {
    let block = themed_block("Help");
    let inner = block.inner(area);
    if inner.width == 0 || inner.height == 0 {
        return 0;
    }
    wrapped_line_count(&tab_help_lines(tab), inner.width).saturating_sub(inner.height as usize)
        as u16
}

pub(in crate::tui) fn tab_help_lines(tab: TabKind) -> Vec<Line<'static>> {
    render_markdown_lines(tab_help_markdown(tab))
}

pub(in crate::tui) fn tab_help_markdown(tab: TabKind) -> &'static str {
    match tab {
        TabKind::Memories => include_str!("../help/memories.md"),
        TabKind::Agents => include_str!("../help/agents.md"),
        TabKind::Query => include_str!("../help/query.md"),
        TabKind::Activity => include_str!("../help/activity.md"),
        TabKind::Errors => include_str!("../help/errors.md"),
        TabKind::Project => include_str!("../help/project.md"),
        TabKind::Review => include_str!("../help/review.md"),
        TabKind::Watchers => include_str!("../help/watchers.md"),
        TabKind::Skills => include_str!("../help/skills.md"),
        TabKind::Automations => include_str!("../help/automations.md"),
        TabKind::Embeddings => include_str!("../help/embeddings.md"),
        TabKind::Resume => include_str!("../help/resume.md"),
    }
}

pub(in crate::tui) fn wrapped_line_count(lines: &[Line<'_>], width: u16) -> usize {
    if width == 0 {
        return 0;
    }
    let width = width as usize;
    lines
        .iter()
        .map(|line| {
            let line_width = line.width();
            if line_width == 0 {
                1
            } else {
                line_width.div_ceil(width)
            }
        })
        .sum()
}

pub(in crate::tui) fn draw_help_tab(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let max_scroll = help_max_scroll_in_area(app.chrome.help.help_tab, area);
    let scroll = app.chrome.help.help_scroll.min(max_scroll);
    let help = Paragraph::new(tab_help_lines(app.chrome.help.help_tab))
        .style(Style::default().bg(Theme::PANEL))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0))
        .block(themed_block(format!(
            "{} Help (scroll {}/{})",
            app.chrome.help.help_tab.label(),
            scroll,
            max_scroll
        )));
    frame.render_widget(help, area);
}
