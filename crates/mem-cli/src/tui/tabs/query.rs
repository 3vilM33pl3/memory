use super::super::app::*;
use super::super::theme::{Theme, themed_block, themed_focus_block};
use super::{TabAction, TabContext, TabRenderContext};
use crate::commands::memory_ops::SourceKindString;
use crossterm::event::{Event, KeyCode};
use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Row, Table, Wrap},
};

pub(in crate::tui) fn draw_query_tab(
    frame: &mut ratatui::Frame<'_>,
    ctx: &TabRenderContext<'_>,
    area: Rect,
) {
    let app = ctx.app;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(13),
            Constraint::Min(12),
        ])
        .split(area);
    let query_editing = matches!(app.chrome.input_mode, InputMode::Query(_));
    let query_input_area = chunks[0];
    let query_inner_width = query_input_area.width.saturating_sub(2);
    let query_input = query_input_display(&current_query_display(app), query_inner_width);
    let query_title = if app.query.query_loading {
        "Question (searching)"
    } else if query_editing {
        "Question (editing)"
    } else {
        "Question"
    };
    let query_style = if query_input.placeholder {
        Style::default().fg(Theme::MUTED).bg(Theme::PANEL)
    } else {
        Style::default().fg(Theme::TEXT).bg(Theme::PANEL)
    };
    let query_box = Paragraph::new(Line::from(Span::styled(query_input.text, query_style)))
        .style(Style::default().bg(Theme::PANEL))
        .block(themed_focus_block(
            query_title,
            query_editing || app.query.query_loading,
        ));
    frame.render_widget(query_box, query_input_area);
    if query_editing && query_input_area.width > 2 && query_input_area.height > 2 {
        frame.set_cursor_position(Position::new(
            query_input_area.x + 1 + query_input.cursor_col,
            query_input_area.y + 1,
        ));
    }

    let answer_text = if app.query.query_loading {
        let elapsed = app
            .query
            .query_started_at
            .map(|started| started.elapsed().as_millis() as u64)
            .unwrap_or_default();
        let pending = app
            .query
            .query_pending_question
            .as_deref()
            .unwrap_or(app.query.query_text.as_str());
        let previous = app
            .query
            .query_response
            .as_ref()
            .map(|response| response.results.len())
            .unwrap_or(0);
        vec![
            Line::from(vec![
                label_span("Searching: "),
                Span::styled(pending.to_string(), Style::default().fg(Theme::TEXT)),
            ]),
            Line::from(vec![
                Span::styled("Working ", Style::default().fg(Theme::ACCENT_STRONG)),
                Span::styled(
                    "querying memory and preparing answer/evidence",
                    Style::default().fg(Theme::TEXT),
                ),
            ]),
            Line::from(vec![
                label_span("Elapsed: "),
                Span::styled(format!("{elapsed} ms"), Style::default().fg(Theme::TEXT)),
                Span::raw("   "),
                label_span("Previous results: "),
                Span::styled(previous.to_string(), Style::default().fg(Theme::MUTED)),
            ]),
            Line::from(Span::styled(
                "Previous results remain visible below until the new search finishes.",
                Style::default().fg(Theme::MUTED),
            )),
        ]
    } else if let Some(error) = &app.query.query_error {
        vec![
            Line::from(vec![
                label_span("Question: "),
                Span::styled(
                    display_filter(&current_query_display(app)),
                    Style::default().fg(Theme::TEXT),
                ),
            ]),
            Line::from(vec![
                label_span("Error: "),
                Span::styled(error.clone(), Style::default().fg(Theme::DANGER)),
            ]),
            Line::from(Span::styled(
                "Edit the question with ? and press Enter to try again.",
                Style::default().fg(Theme::MUTED),
            )),
        ]
    } else if let Some(response) = &app.query.query_response {
        let mut lines = vec![
            Line::from(vec![
                label_span("Question: "),
                Span::styled(
                    if current_query_display(app).trim().is_empty() {
                        "<empty>".to_string()
                    } else {
                        current_query_display(app)
                    },
                    Style::default().fg(Theme::TEXT),
                ),
            ]),
            Line::from(vec![
                label_span("Answer: "),
                Span::styled(response.answer.clone(), Style::default().fg(Theme::TEXT)),
            ]),
            Line::from(vec![
                label_span("Method: "),
                query_answer_method_span(&response.answer_generation.method),
                Span::raw("   "),
                label_span("Citations: "),
                Span::styled(
                    format_query_citation_numbers(&response.answer_generation.cited_result_numbers),
                    Style::default().fg(Theme::ACCENT),
                ),
                Span::raw("   "),
                label_span("Answer gen: "),
                Span::styled(
                    format!("{} ms", response.answer_generation.duration_ms),
                    Style::default().fg(Theme::TEXT),
                ),
            ]),
            Line::from(vec![
                label_span("Confidence: "),
                Span::styled(
                    format!("{:.2}", response.confidence),
                    confidence_style(response.confidence),
                ),
                Span::raw("   "),
                label_span("Evidence: "),
                Span::styled(
                    if response.insufficient_evidence {
                        "insufficient"
                    } else {
                        "sufficient"
                    },
                    if response.insufficient_evidence {
                        Style::default().fg(Theme::WARNING)
                    } else {
                        Style::default().fg(Theme::SUCCESS)
                    },
                ),
                Span::raw("   "),
                label_span("Matches: "),
                Span::styled(
                    response.results.len().to_string(),
                    Style::default().fg(Theme::TEXT),
                ),
            ]),
        ];
        lines.extend(query_timing_breakdown_lines(
            response,
            app.query.query_roundtrip_timing,
        ));
        lines.extend([
            if let Some(reason) = &response.answer_generation.fallback_reason {
                Line::from(vec![
                    label_span("Fallback: "),
                    Span::styled(reason.clone(), Style::default().fg(Theme::WARNING)),
                ])
            } else {
                Line::from("")
            },
        ]);
        lines
    } else {
        vec![
            Line::from(vec![
                label_span("Question: "),
                Span::styled(
                    display_filter(&current_query_display(app)),
                    Style::default().fg(Theme::TEXT),
                ),
            ]),
            Line::from(Span::styled(
                "Press ? to enter a question. The result table below shows the memories returned for that query.",
                Style::default().fg(Theme::MUTED),
            )),
        ]
    };

    let answer = Paragraph::new(answer_text)
        .style(Style::default().bg(Theme::PANEL))
        .wrap(Wrap { trim: false })
        .block(themed_block("Query Result"));
    frame.render_widget(answer, chunks[1]);

    let lower = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[2]);

    let header = Row::new(["#", "Summary", "Type", "Match", "Score"]).style(
        Style::default()
            .fg(Theme::ACCENT_STRONG)
            .bg(Theme::PANEL_ALT)
            .add_modifier(Modifier::BOLD),
    );
    let cited_numbers = app
        .query
        .query_response
        .as_ref()
        .map(|response| &response.answer_generation.cited_result_numbers);
    let rows = app
        .query_results()
        .iter()
        .enumerate()
        .map(|(index, result)| {
            query_row(
                index + 1,
                result,
                cited_numbers.is_some_and(|numbers| numbers.contains(&(index + 1))),
            )
        });
    let table = Table::new(
        rows,
        [
            Constraint::Length(4),
            Constraint::Percentage(52),
            Constraint::Length(13),
            Constraint::Length(10),
            Constraint::Length(8),
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
        "Returned Memories ({})",
        app.query_results().len()
    )));
    let mut state = app.query.query_table_state.clone();
    frame.render_stateful_widget(table, lower[0], &mut state);

    let detail_text = if let Some(result) = app.query_results().get(app.query.query_selected_index)
    {
        let result_number = app.query.query_selected_index + 1;
        let cited_in_answer = app.query.query_response.as_ref().is_some_and(|response| {
            response
                .answer_generation
                .cited_result_numbers
                .contains(&result_number)
        });
        let mut lines = vec![
            Line::from(vec![
                label_span("Summary: "),
                Span::styled(result.summary.clone(), Style::default().fg(Theme::TEXT)),
            ]),
            Line::from(vec![
                label_span("Type: "),
                memory_type_span(&result.memory_type),
                Span::raw("   "),
                label_span("Match: "),
                query_match_span(&result.match_kind),
                Span::raw("   "),
                label_span("Score: "),
                Span::styled(
                    format!("{:.2}", result.score),
                    Style::default().fg(Theme::ACCENT_STRONG),
                ),
                Span::raw("   "),
                label_span("Cited: "),
                Span::styled(
                    if cited_in_answer { "yes" } else { "no" },
                    if cited_in_answer {
                        Style::default().fg(Theme::SUCCESS)
                    } else {
                        Style::default().fg(Theme::MUTED)
                    },
                ),
            ]),
            Line::from(""),
            Line::from(vec![section_span("Snippet")]),
            Line::from(Span::styled(
                result.snippet.clone(),
                Style::default().fg(Theme::TEXT),
            )),
        ];

        lines.push(Line::from(""));
        lines.push(Line::from(vec![section_span("Search Diagnostics")]));
        lines.push(Line::from(Span::styled(
            format!(
                "chunk={:.2} | entry={:.2} | semantic={:.2} | overlap={:.0}% | relation={:.2} | graph={:.2}",
                result.debug.chunk_fts,
                result.debug.entry_fts,
                result.debug.semantic_similarity,
                result.debug.term_overlap * 100.0,
                result.debug.relation_boost,
                result.debug.graph_boost
            ),
            Style::default().fg(Theme::TEXT),
        )));
        lines.push(Line::from(Span::styled(
            format!(
                "phrases={} | tags={} | paths={} | graph matches={} edges={} | importance={} | confidence={:.2} | recency={:.2}",
                result.debug.exact_phrase_matches,
                result.debug.tag_match_count,
                result.debug.path_match_count,
                result.debug.graph_match_count,
                result.debug.graph_edge_count,
                result.debug.importance,
                result.debug.memory_confidence,
                result.debug.recency_boost
            ),
            Style::default().fg(Theme::MUTED),
        )));

        if !result.score_explanation.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![section_span("Why It Ranked")]));
            for explanation in &result.score_explanation {
                lines.push(Line::from(Span::styled(
                    format!("- {explanation}"),
                    Style::default().fg(Theme::ACCENT),
                )));
            }
        }

        if !result.graph_connections.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![section_span("Graph Connections")]));
            for connection in &result.graph_connections {
                let mut details = vec![connection.reason.clone(), connection.file_path.clone()];
                if let Some(symbol) = &connection.symbol {
                    details.push(format!("symbol={symbol}"));
                }
                if let Some(edge_kind) = &connection.edge_kind {
                    details.push(format!("edge={edge_kind}"));
                }
                if let Some(neighbor) = &connection.neighbor_symbol {
                    details.push(format!("neighbor={neighbor}"));
                }
                details.push(format!("boost={:.2}", connection.score_boost));
                lines.push(Line::from(Span::styled(
                    format!("- {}", details.join(" | ")),
                    Style::default().fg(Theme::ACCENT),
                )));
            }
        }

        if let Some(detail) = &app.query.query_selected_detail {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![section_span("Canonical Text")]));
            lines.push(Line::from(Span::styled(
                detail.canonical_text.clone(),
                Style::default().fg(Theme::TEXT),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(vec![section_span("Related Memories")]));
            if detail.related_memories.is_empty() {
                lines.push(Line::from(Span::styled(
                    "No related memories recorded.",
                    Style::default().fg(Theme::MUTED),
                )));
            } else {
                for related in &detail.related_memories {
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("{} ", related.relation_type),
                            Style::default().fg(Theme::ACCENT),
                        ),
                        memory_type_span(&related.memory_type),
                        Span::raw(" "),
                        Span::styled(
                            format!("({:.2}) ", related.confidence),
                            confidence_style(related.confidence),
                        ),
                        Span::styled(related.summary.clone(), Style::default().fg(Theme::TEXT)),
                    ]));
                }
            }
        } else if app.query.query_detail_loading {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Loading selected memory detail...",
                Style::default().fg(Theme::MUTED),
            )));
        }

        if !result.sources.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![section_span("Sources")]));
            for source in &result.sources {
                let mut parts = vec![source.source_kind.source_kind_string().to_string()];
                if let Some(path) = &source.file_path {
                    parts.push(path.clone());
                }
                if let Some(excerpt) = &source.excerpt {
                    parts.push(excerpt.clone());
                }
                lines.push(Line::from(Span::styled(
                    parts.join(" | "),
                    Style::default().fg(Theme::TEXT),
                )));
            }
        }

        lines
    } else {
        vec![Line::from(Span::styled(
            "Run a query to inspect the returned memories.",
            Style::default().fg(Theme::MUTED),
        ))]
    };

    let detail = Paragraph::new(detail_text)
        .style(Style::default().bg(Theme::PANEL))
        .wrap(Wrap { trim: false })
        .block(themed_block("Returned Memory Detail"));
    frame.render_widget(detail, lower[1]);
}

pub(in crate::tui) fn update(
    event: &Event,
    state: &mut QueryTabState,
    _ctx: &mut TabContext,
) -> TabAction {
    match event {
        Event::Key(key) => match key.code {
            KeyCode::Down | KeyCode::Char('j') => move_query_selection(state, 1),
            KeyCode::Up | KeyCode::Char('k') => move_query_selection(state, -1),
            _ => TabAction::None,
        },
        _ => TabAction::None,
    }
}

fn move_query_selection(state: &mut QueryTabState, delta: isize) -> TabAction {
    let result_count = state
        .query_response
        .as_ref()
        .map(|response| response.results.len())
        .unwrap_or_default();
    if result_count == 0 {
        return TabAction::None;
    }

    let next = (state.query_selected_index as isize + delta)
        .clamp(0, result_count.saturating_sub(1) as isize) as usize;
    if next == state.query_selected_index {
        return TabAction::None;
    }

    state.query_selected_index = next;
    state.query_table_state.select(Some(next));
    state.query_selected_detail = None;
    state.query_detail_loading = false;
    TabAction::QuerySelectionChanged
}
