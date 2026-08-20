// SPDX-License-Identifier: AGPL-3.0-or-later

// SPDX-License-Identifier: AGPL-3.0-or-later

use chrono::{DateTime, Local, Utc};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use mem_skills::SkillBundleStatus;

#[allow(unused_imports)]
use super::*;
use crate::tui::theme::Theme;

pub(in crate::tui) fn truncate_for_list(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

pub(in crate::tui) fn active_embedding_backend_index(
    snapshot: &mem_record::EmbeddingBackendsResponse,
) -> Option<usize> {
    snapshot.backends.iter().position(|backend| backend.active)
}

pub(in crate::tui) fn embedding_backend_index_by_name(
    snapshot: &mem_record::EmbeddingBackendsResponse,
    name: &str,
) -> Option<usize> {
    snapshot
        .backends
        .iter()
        .position(|backend| backend.name == name)
}

pub(in crate::tui) fn clamped_embedding_backend_index(
    current: usize,
    snapshot: &mem_record::EmbeddingBackendsResponse,
) -> Option<usize> {
    (!snapshot.backends.is_empty()).then(|| current.min(snapshot.backends.len().saturating_sub(1)))
}

pub(in crate::tui) fn non_empty_or(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

pub(in crate::tui) fn lines_for_named_counts(
    items: Vec<(String, i64)>,
    empty: &str,
) -> Vec<Line<'static>> {
    if items.is_empty() {
        vec![Line::from(empty.to_string())]
    } else {
        items
            .into_iter()
            .map(|(name, count)| {
                Line::from(vec![
                    Span::styled(name, Style::default().fg(Theme::TEXT)),
                    Span::styled(": ", Style::default().fg(Theme::MUTED)),
                    Span::styled(count.to_string(), Style::default().fg(Theme::ACCENT_STRONG)),
                ])
            })
            .collect()
    }
}

pub(in crate::tui) fn embedding_base_url_is_default(provider: &str, base_url: &str) -> bool {
    // Keep in sync with mem_search::embedding_backend::default_base_url.
    let expected = match provider {
        "openai_compatible" | "openai" => "https://api.openai.com/v1",
        "ollama" => "http://127.0.0.1:11434/v1",
        "voyage" => "https://api.voyageai.com",
        "cohere" => "https://api.cohere.com",
        "gemini" => "https://generativelanguage.googleapis.com/v1beta",
        _ => return false,
    };
    base_url.trim_end_matches('/') == expected
}

pub(in crate::tui) fn format_timestamp(value: Option<DateTime<Utc>>) -> String {
    value
        .map(format_timestamp_full)
        .unwrap_or_else(|| "n/a".to_string())
}

pub(in crate::tui) fn format_timestamp_full(value: DateTime<Utc>) -> String {
    value
        .with_timezone(&Local)
        .format("%Y-%m-%d %H:%M:%S %Z")
        .to_string()
}

pub(in crate::tui) fn format_timestamp_medium(value: DateTime<Utc>) -> String {
    value
        .with_timezone(&Local)
        .format("%Y-%m-%d %H:%M %Z")
        .to_string()
}

pub(in crate::tui) fn format_timestamp_short(value: DateTime<Utc>) -> String {
    value
        .with_timezone(&Local)
        .format("%H:%M:%S %Z")
        .to_string()
}

pub(in crate::tui) fn format_timestamp_timeline(value: DateTime<Utc>) -> String {
    value
        .with_timezone(&Local)
        .format("%m-%d %H:%M %Z")
        .to_string()
}

pub(in crate::tui) fn display_filter(value: &str) -> String {
    if value.is_empty() {
        "none".to_string()
    } else {
        value.to_string()
    }
}

pub(in crate::tui) fn split_root_area(area: Rect) -> [Rect; 4] {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(4),
        ])
        .split(area);
    [chunks[0], chunks[1], chunks[2], chunks[3]]
}

pub(in crate::tui) fn split_memories_area(area: Rect) -> [Rect; 2] {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);
    [chunks[0], chunks[1]]
}

pub(in crate::tui) fn current_frame_area() -> Option<Rect> {
    let (width, height) = crossterm::terminal::size().ok()?;
    Some(Rect::new(0, 0, width, height))
}

pub(in crate::tui) fn default_frame_area() -> Rect {
    Rect::new(0, 0, 160, 48)
}

pub(in crate::tui) fn accent_span(value: impl Into<String>) -> Span<'static> {
    Span::styled(
        value.into(),
        Style::default()
            .fg(Theme::ACCENT)
            .add_modifier(Modifier::BOLD),
    )
}

pub(in crate::tui) fn label_span(value: impl Into<String>) -> Span<'static> {
    Span::styled(
        value.into(),
        Style::default()
            .fg(Theme::ACCENT_STRONG)
            .add_modifier(Modifier::BOLD),
    )
}

pub(in crate::tui) fn section_span(value: impl Into<String>) -> Span<'static> {
    Span::styled(
        value.into(),
        Style::default()
            .fg(Theme::TITLE)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
    )
}

pub(in crate::tui) fn status_span(value: &str) -> Span<'static> {
    let color = match value {
        "active" | "ok" | "up" => Theme::SUCCESS,
        "archived" | "unknown" => Theme::WARNING,
        _ => Theme::DANGER,
    };
    Span::styled(
        value.to_string(),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

pub(in crate::tui) fn service_span(value: &str) -> Span<'static> {
    let color = match value {
        "ok" | "up" => Theme::SUCCESS,
        "unknown" => Theme::WARNING,
        _ => Theme::DANGER,
    };
    Span::styled(
        value.to_string(),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

pub(in crate::tui) fn metric_line<'a>(label: &str, value: Span<'a>) -> Line<'a> {
    Line::from(vec![
        Span::styled(
            format!("{label}: "),
            Style::default()
                .fg(Theme::ACCENT_STRONG)
                .add_modifier(Modifier::BOLD),
        ),
        value,
    ])
}

pub(in crate::tui) fn skill_bundle_status_color(status: SkillBundleStatus) -> Color {
    match status {
        SkillBundleStatus::Ok => Theme::SUCCESS,
        SkillBundleStatus::Warn => Theme::WARNING,
        SkillBundleStatus::Error => Theme::DANGER,
    }
}
