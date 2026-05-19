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

pub(in crate::tui) fn draw_review_tab(
    frame: &mut ratatui::Frame<'_>,
    ctx: &TabRenderContext<'_>,
    area: Rect,
) {
    let app = ctx.app;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(area);

    let pending = app.review.replacement_proposals.len();
    let selected_label = if pending == 0 {
        "—".to_string()
    } else {
        format!("{}/{}", app.review.replacement_selected_index + 1, pending)
    };
    let header = Paragraph::new(vec![
        Line::from(vec![
            label_span("Policy: "),
            Span::styled(
                app.review.replacement_policy.to_string(),
                Style::default()
                    .fg(Theme::ACCENT_STRONG)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("   "),
            label_span("Pending: "),
            Span::styled(pending.to_string(), Style::default().fg(Theme::TEXT)),
            Span::raw("   "),
            label_span("Selected: "),
            Span::styled(selected_label, Style::default().fg(Theme::TEXT)),
        ]),
        Line::from(Span::styled(
            "Clear updates replace automatically; ambiguous ones queue here for your approval.",
            Style::default().fg(Theme::MUTED),
        )),
    ])
    .style(Style::default().bg(Theme::PANEL))
    .block(themed_block("Curation Review"));
    frame.render_widget(header, chunks[0]);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    if app.review.replacement_proposals.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "No pending replacement proposals. New ambiguous curation candidates will appear here.",
                Style::default().fg(Theme::MUTED),
            )))
            .wrap(Wrap { trim: false })
            .style(Style::default().bg(Theme::PANEL_ALT))
            .block(themed_block("Proposals")),
            body[0],
        );
    } else {
        let header_row = Row::new(["#", "TARGET", "CANDIDATE", "SCORE"]).style(
            Style::default()
                .fg(Theme::ACCENT_STRONG)
                .bg(Theme::PANEL_ALT)
                .add_modifier(Modifier::BOLD),
        );
        let rows = app
            .review
            .replacement_proposals
            .iter()
            .enumerate()
            .map(|(idx, proposal)| {
                Row::new(vec![
                    Line::from(Span::styled(
                        (idx + 1).to_string(),
                        Style::default().fg(Theme::MUTED),
                    )),
                    Line::from(Span::styled(
                        truncate_for_list(&proposal.target_summary, 48),
                        Style::default().fg(Theme::TEXT),
                    )),
                    Line::from(Span::styled(
                        truncate_for_list(&proposal.candidate_summary, 48),
                        Style::default().fg(Theme::ACCENT),
                    )),
                    Line::from(Span::styled(
                        proposal.score.to_string(),
                        Style::default().fg(Theme::TEXT),
                    )),
                ])
            });
        let table = Table::new(
            rows,
            [
                Constraint::Length(4),
                Constraint::Percentage(45),
                Constraint::Percentage(45),
                Constraint::Length(6),
            ],
        )
        .header(header_row)
        .row_highlight_style(
            Style::default()
                .bg(Theme::SELECTION_BG)
                .fg(Theme::SELECTION_FG),
        )
        .style(Style::default().bg(Theme::PANEL_ALT))
        .block(themed_block(format!("Proposals ({pending})")));
        let mut state = app.review.review_table_state.clone();
        frame.render_stateful_widget(table, body[0], &mut state);
    }

    frame.render_widget(
        Paragraph::new(review_detail_lines(app))
            .wrap(Wrap { trim: false })
            .style(Style::default().bg(Theme::PANEL_ALT))
            .block(themed_block("Detail")),
        body[1],
    );

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            accent_span("j/k [ ] "),
            Span::styled("select  ", Style::default().fg(Theme::TEXT)),
            accent_span("y "),
            Span::styled("approve  ", Style::default().fg(Theme::TEXT)),
            accent_span("n "),
            Span::styled("reject  ", Style::default().fg(Theme::TEXT)),
            accent_span("p "),
            Span::styled("cycle policy  ", Style::default().fg(Theme::TEXT)),
            accent_span("r "),
            Span::styled("refresh", Style::default().fg(Theme::TEXT)),
        ]))
        .style(Style::default().bg(Theme::PANEL))
        .block(themed_block("Actions")),
        chunks[2],
    );
}

pub(in crate::tui) fn update(
    event: &Event,
    state: &mut ReviewTabState,
    _ctx: &mut TabContext,
) -> TabAction {
    match event {
        Event::Key(key) => match key.code {
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Char(']') => {
                select_replacement_proposal(state, 1);
                TabAction::Redraw
            }
            KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('[') => {
                select_replacement_proposal(state, -1);
                TabAction::Redraw
            }
            KeyCode::PageDown => {
                select_replacement_proposal(state, 8);
                TabAction::Redraw
            }
            KeyCode::PageUp => {
                select_replacement_proposal(state, -8);
                TabAction::Redraw
            }
            KeyCode::Home => {
                jump_replacement_proposal(state, 0);
                TabAction::Redraw
            }
            KeyCode::End => {
                let len = state.replacement_proposals.len();
                jump_replacement_proposal(state, len.saturating_sub(1));
                TabAction::Redraw
            }
            _ => TabAction::None,
        },
        _ => TabAction::None,
    }
}

fn select_replacement_proposal(state: &mut ReviewTabState, delta: isize) {
    let len = state.replacement_proposals.len();
    if len == 0 {
        state.replacement_selected_index = 0;
        state.review_table_state.select(None);
        return;
    }
    let cur = state.replacement_selected_index as isize;
    let next = ((cur + delta) % len as isize + len as isize) % len as isize;
    state.replacement_selected_index = next as usize;
    state
        .review_table_state
        .select(Some(state.replacement_selected_index));
}

fn jump_replacement_proposal(state: &mut ReviewTabState, index: usize) {
    let len = state.replacement_proposals.len();
    if len == 0 {
        state.replacement_selected_index = 0;
        state.review_table_state.select(None);
        return;
    }
    state.replacement_selected_index = index.min(len - 1);
    state
        .review_table_state
        .select(Some(state.replacement_selected_index));
}
