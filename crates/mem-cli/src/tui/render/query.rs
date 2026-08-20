// SPDX-License-Identifier: AGPL-3.0-or-later

// SPDX-License-Identifier: AGPL-3.0-or-later

use mem_record::{QueryAnswerMethod, QueryFilters, QueryMatchKind, QueryResponse, QueryResult};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Row},
};

#[allow(unused_imports)]
use super::*;
use crate::tui::{app::*, theme::Theme};

pub(in crate::tui) fn current_query_display(app: &App) -> String {
    match &app.chrome.input_mode {
        InputMode::Query(value) => value.clone(),
        _ => app.query.query_text.clone(),
    }
}

pub(in crate::tui) struct QueryInputDisplay {
    pub(in crate::tui) text: String,
    pub(in crate::tui) cursor_col: u16,
    pub(in crate::tui) placeholder: bool,
}

pub(in crate::tui) fn query_input_display(value: &str, inner_width: u16) -> QueryInputDisplay {
    let width = inner_width as usize;
    if width == 0 {
        return QueryInputDisplay {
            text: String::new(),
            cursor_col: 0,
            placeholder: value.is_empty(),
        };
    }
    if value.is_empty() {
        let placeholder = "Ask project memory a question...";
        let text = placeholder.chars().take(width).collect::<String>();
        return QueryInputDisplay {
            text,
            cursor_col: 0,
            placeholder: true,
        };
    }

    let char_count = value.chars().count();
    if char_count <= width {
        return QueryInputDisplay {
            text: value.to_string(),
            cursor_col: char_count.min(width.saturating_sub(1)) as u16,
            placeholder: false,
        };
    }

    let tail_width = width.saturating_sub(1);
    let mut tail = value
        .chars()
        .skip(char_count.saturating_sub(tail_width))
        .collect::<String>();
    tail.insert(0, '<');
    QueryInputDisplay {
        text: tail,
        cursor_col: width.saturating_sub(1) as u16,
        placeholder: false,
    }
}

pub(in crate::tui) fn query_row(
    result_number: usize,
    item: &QueryResult,
    cited: bool,
) -> Row<'static> {
    let number = if cited {
        format!("[{result_number}]")
    } else {
        result_number.to_string()
    };
    let number_style = if cited {
        Style::default()
            .fg(Theme::SUCCESS)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Theme::MUTED)
    };
    Row::new(vec![
        Cell::from(Span::styled(number, number_style)),
        Cell::from(Span::styled(
            item.summary.clone(),
            Style::default().fg(Theme::TEXT),
        )),
        Cell::from(memory_type_span(&item.memory_type)),
        Cell::from(query_match_span(&item.match_kind)),
        Cell::from(Span::styled(
            format!("{:.2}", item.score),
            Style::default().fg(Theme::ACCENT_STRONG),
        )),
    ])
}

pub(in crate::tui) fn format_query_citation_numbers(numbers: &[usize]) -> String {
    if numbers.is_empty() {
        "none".to_string()
    } else {
        numbers
            .iter()
            .map(|number| format!("[{number}]"))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

pub(in crate::tui) fn query_answer_method_span(method: &QueryAnswerMethod) -> Span<'static> {
    let color = match method {
        QueryAnswerMethod::Llm => Theme::SUCCESS,
        QueryAnswerMethod::Deterministic => Theme::ACCENT,
        QueryAnswerMethod::Fallback => Theme::WARNING,
    };
    Span::styled(method.to_string(), Style::default().fg(color))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::tui) struct QueryTimingBreakdown {
    pub(in crate::tui) backend_reported_ms: u64,
    pub(in crate::tui) transport_overhead_ms: u64,
    pub(in crate::tui) retrieval_other_ms: u64,
}

pub(in crate::tui) fn query_timing_breakdown(
    response: &QueryResponse,
    timing: QueryRoundtripTiming,
) -> QueryTimingBreakdown {
    let diagnostics = &response.diagnostics;
    let backend_reported_ms = diagnostics
        .total_duration_ms
        .saturating_add(response.answer_generation.duration_ms);
    let retrieval_known_ms = diagnostics
        .lexical_duration_ms
        .saturating_add(diagnostics.semantic_duration_ms)
        .saturating_add(diagnostics.graph_duration_ms)
        .saturating_add(diagnostics.rerank_duration_ms);
    QueryTimingBreakdown {
        backend_reported_ms,
        transport_overhead_ms: timing.query_api_ms.saturating_sub(backend_reported_ms),
        retrieval_other_ms: diagnostics
            .total_duration_ms
            .saturating_sub(retrieval_known_ms),
    }
}

pub(in crate::tui) fn format_query_timing(value: Option<u64>) -> String {
    value
        .map(|value| format!("{value} ms"))
        .unwrap_or_else(|| "n/a".to_string())
}

pub(in crate::tui) fn format_query_timing_with_percent(value: u64, total: u64) -> String {
    value
        .saturating_mul(100)
        .checked_div(total)
        .map(|percent| format!("{value} ms ({percent}%)"))
        .unwrap_or_else(|| format!("{value} ms"))
}

pub(in crate::tui) fn query_timing_breakdown_lines(
    response: &QueryResponse,
    timing: Option<QueryRoundtripTiming>,
) -> Vec<Line<'static>> {
    let fallback_timing = QueryRoundtripTiming {
        query_api_ms: response
            .diagnostics
            .total_duration_ms
            .saturating_add(response.answer_generation.duration_ms),
        initial_detail_ms: None,
        ui_ready_ms: response
            .diagnostics
            .total_duration_ms
            .saturating_add(response.answer_generation.duration_ms),
    };
    let timing = timing.unwrap_or(fallback_timing);
    let breakdown = query_timing_breakdown(response, timing);
    let retrieval_total = response.diagnostics.total_duration_ms;

    vec![
        Line::from(vec![section_span("Timing Breakdown")]),
        Line::from(vec![
            label_span("UI ready: "),
            Span::styled(
                format_query_timing(Some(timing.ui_ready_ms)),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Query API: "),
            Span::styled(
                format_query_timing(Some(timing.query_api_ms)),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Initial detail: "),
            Span::styled(
                format_query_timing(timing.initial_detail_ms),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
        Line::from(vec![
            label_span("Backend: "),
            Span::styled(
                format_query_timing_with_percent(breakdown.backend_reported_ms, timing.ui_ready_ms),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Retrieval: "),
            Span::styled(
                format_query_timing_with_percent(retrieval_total, timing.ui_ready_ms),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Answer: "),
            Span::styled(
                format_query_timing_with_percent(
                    response.answer_generation.duration_ms,
                    timing.ui_ready_ms,
                ),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Overhead: "),
            Span::styled(
                format_query_timing(Some(breakdown.transport_overhead_ms)),
                Style::default().fg(Theme::MUTED),
            ),
        ]),
        Line::from(vec![
            label_span("Lexical: "),
            Span::styled(
                format!(
                    "{} candidates, {}",
                    response.diagnostics.lexical_candidates,
                    format_query_timing_with_percent(
                        response.diagnostics.lexical_duration_ms,
                        retrieval_total,
                    )
                ),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Semantic: "),
            Span::styled(
                format!(
                    "{} [{}], {}",
                    response.diagnostics.semantic_candidates,
                    response.diagnostics.semantic_status,
                    format_query_timing_with_percent(
                        response.diagnostics.semantic_duration_ms,
                        retrieval_total,
                    )
                ),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
        Line::from(vec![
            label_span("Graph: "),
            Span::styled(
                format!(
                    "{} [{}], {}",
                    response.diagnostics.graph_candidates,
                    response.diagnostics.graph_status,
                    format_query_timing_with_percent(
                        response.diagnostics.graph_duration_ms,
                        retrieval_total,
                    )
                ),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Rerank/relation: "),
            Span::styled(
                format_query_timing_with_percent(
                    response.diagnostics.rerank_duration_ms,
                    retrieval_total,
                ),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Other: "),
            Span::styled(
                format_query_timing_with_percent(breakdown.retrieval_other_ms, retrieval_total),
                Style::default().fg(Theme::MUTED),
            ),
        ]),
    ]
}

pub(in crate::tui) fn format_query_filters(filters: &QueryFilters) -> String {
    let types = if filters.types.is_empty() {
        "types=all".to_string()
    } else {
        format!(
            "types={}",
            filters
                .types
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        )
    };
    let tags = if filters.tags.is_empty() {
        "tags=all".to_string()
    } else {
        format!("tags={}", filters.tags.join(","))
    };
    format!("{types} {tags}")
}

pub(in crate::tui) fn query_match_span(kind: &QueryMatchKind) -> Span<'static> {
    let (label, color) = match kind {
        QueryMatchKind::Lexical => ("lexical", Theme::ACCENT_STRONG),
        QueryMatchKind::Semantic => ("semantic", Theme::SUCCESS),
        QueryMatchKind::Hybrid => ("hybrid", Theme::ACCENT),
    };
    Span::styled(
        label,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}
