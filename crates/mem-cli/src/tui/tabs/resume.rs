use super::super::app::*;
use super::super::theme::{themed_block, Theme};
use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

pub(in crate::tui) fn draw_resume_tab(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let lines = if let Some(response) = &app.resume.resume_response {
        let mut lines = Vec::new();
        if app.resume.resume_loading {
            lines.push(Line::from(Span::styled(
                "Refreshing resume in the background...",
                Style::default().fg(Theme::ACCENT),
            )));
            lines.push(Line::from(""));
        }
        lines.push(Line::from(vec![
            label_span("Project: "),
            Span::styled(response.project.clone(), Style::default().fg(Theme::TEXT)),
        ]));
        if let Some(checkpoint) = &response.checkpoint {
            lines.push(Line::from(vec![
                label_span("Checkpoint: "),
                Span::styled(
                    format_timestamp_medium(checkpoint.marked_at),
                    Style::default().fg(Theme::TEXT),
                ),
            ]));
            if let Some(note) = &checkpoint.note {
                lines.push(Line::from(vec![
                    label_span("Note: "),
                    Span::styled(note.clone(), Style::default().fg(Theme::TEXT)),
                ]));
            }
        } else {
            lines.push(Line::from(Span::styled(
                "No checkpoint stored yet. Use `memory checkpoint save --project <slug>` when you leave a project.",
                Style::default().fg(Theme::MUTED),
            )));
        }
        if let Some(current_thread) = &response.current_thread {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![section_span("Current Thread")]));
            lines.push(Line::from(Span::styled(
                current_thread.clone(),
                Style::default().fg(Theme::TEXT),
            )));
        }
        if let Some(action) = &response.primary_next_step {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![section_span("Next Step")]));
            lines.push(Line::from(Span::styled(
                format!("{}: {}", action.title, action.rationale),
                Style::default().fg(Theme::TEXT),
            )));
            if let Some(command_hint) = &action.command_hint {
                lines.push(Line::from(Span::styled(
                    command_hint.clone(),
                    Style::default().fg(Theme::MUTED),
                )));
            }
        }
        if !response.change_summary.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![section_span("What Changed")]));
            for item in &response.change_summary {
                lines.push(Line::from(Span::styled(
                    format!("- {item}"),
                    Style::default().fg(Theme::TEXT),
                )));
            }
        }
        if !response.attention_items.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![section_span("Needs Attention")]));
            for item in &response.attention_items {
                lines.push(Line::from(Span::styled(
                    format!("- {item}"),
                    Style::default().fg(Theme::WARNING),
                )));
            }
        }
        if !response.context_items.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![section_span("Keep In Mind")]));
            for item in &response.context_items {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("[{}] ", item.memory_type),
                        Style::default().fg(Theme::ACCENT),
                    ),
                    Span::styled(item.summary.clone(), Style::default().fg(Theme::TEXT)),
                ]));
            }
        }
        if !response.secondary_next_steps.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![section_span("Other Useful Follow-Ups")]));
            for action in &response.secondary_next_steps {
                lines.push(Line::from(Span::styled(
                    format!("- {}: {}", action.title, action.rationale),
                    Style::default().fg(Theme::TEXT),
                )));
                if let Some(command_hint) = &action.command_hint {
                    lines.push(Line::from(Span::styled(
                        format!("  {command_hint}"),
                        Style::default().fg(Theme::MUTED),
                    )));
                }
            }
        }
        if !response.warnings.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![section_span("All Warnings")]));
            for warning in &response.warnings {
                lines.push(Line::from(Span::styled(
                    format!("- {warning}"),
                    Style::default().fg(Theme::WARNING),
                )));
            }
        }
        if !response.actions.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![section_span("All Suggested Next Actions")]));
            for action in &response.actions {
                lines.push(Line::from(Span::styled(
                    format!("- {}: {}", action.title, action.rationale),
                    Style::default().fg(Theme::TEXT),
                )));
                if let Some(command_hint) = &action.command_hint {
                    lines.push(Line::from(Span::styled(
                        format!("  {command_hint}"),
                        Style::default().fg(Theme::MUTED),
                    )));
                }
            }
        }
        if response.current_thread.is_none()
            && response.change_summary.is_empty()
            && response.attention_items.is_empty()
            && response.context_items.is_empty()
        {
            lines.push(Line::from(""));
            append_resume_briefing_lines(&mut lines, &response.briefing);
        }
        if !response.timeline.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![section_span("Recent Timeline")]));
            for event in response.timeline.iter().take(8) {
                lines.push(Line::from(Span::styled(
                    format!(
                        "- {}  {}",
                        format_timestamp_timeline(event.recorded_at),
                        event.summary
                    ),
                    Style::default().fg(Theme::TEXT),
                )));
            }
        }
        lines
    } else if app.resume.resume_loading {
        vec![Line::from(Span::styled(
            "Loading resume in the background...",
            Style::default().fg(Theme::ACCENT),
        ))]
    } else if let Some(error) = &app.resume.resume_error {
        vec![Line::from(Span::styled(
            format!("Resume unavailable: {error}"),
            Style::default().fg(Theme::WARNING),
        ))]
    } else {
        vec![Line::from(Span::styled(
            "Resume briefing is unavailable. Press r to refresh.",
            Style::default().fg(Theme::MUTED),
        ))]
    };

    let paragraph = Paragraph::new(lines)
        .scroll((app.resume.resume_scroll, 0))
        .wrap(Wrap { trim: false })
        .style(Style::default().bg(Theme::PANEL_ALT))
        .block(themed_block(format!(
            "Resume (scroll {})",
            app.resume.resume_scroll
        )));
    frame.render_widget(paragraph, area);
}
