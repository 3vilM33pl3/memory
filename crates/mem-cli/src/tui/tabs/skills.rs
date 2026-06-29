use super::super::app::*;
use super::super::theme::{Theme, themed_block};
use super::{TabAction, TabContext, TabRenderContext};
use crossterm::event::Event;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Row, Table, Wrap},
};
use std::fs;

pub(in crate::tui) fn draw_skills_tab(
    frame: &mut ratatui::Frame<'_>,
    ctx: &TabRenderContext<'_>,
    area: Rect,
) {
    let app = ctx.app;
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(4), Constraint::Min(0)])
        .split(area);
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(outer[1]);

    let inventory = &app.meta.skill_inventory;
    let visible_skills = filtered_skills(app);
    let selected = visible_skills.get(app.skills.selected_index).copied();
    let message = app
        .skills
        .operation
        .as_deref()
        .map(|operation| format!("{operation}..."))
        .or_else(|| app.skills.message.clone())
        .unwrap_or_else(|| inventory.summary.clone());
    let filter_summary = format!(
        "Filter: {} ({}/{})  f next  F previous  u repair Memory skills",
        app.skills.filter.label(),
        visible_skills.len(),
        inventory.skills.len()
    );
    let filter_line = Line::from(filter_spans(app.skills.filter));
    let filter_bar = Paragraph::new(vec![Line::from(filter_summary), filter_line])
        .block(themed_block("Skill Filters"));
    frame.render_widget(filter_bar, outer[0]);

    let rows = visible_skills.iter().map(|skill| {
        Row::new(vec![
            skill.name.clone(),
            skill.source_kind.label().to_string(),
            skill.status.label().to_string(),
            skill
                .project_version
                .as_deref()
                .unwrap_or("n/a")
                .to_string(),
            format!("{:?}", skill.action),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(30),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(15),
            Constraint::Percentage(15),
        ],
    )
    .header(
        Row::new(vec!["Skill", "Source", "Status", "Version", "Action"]).style(
            Style::default()
                .fg(Theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(themed_block(format!(
        "Skills v{} {} - {}",
        inventory.bundle_version,
        inventory.status.label(),
        message
    )))
    .row_highlight_style(
        Style::default()
            .bg(Theme::SELECTION_BG)
            .fg(Theme::SELECTION_FG)
            .add_modifier(Modifier::BOLD),
    );
    let mut table_state = app.skills.table_state.clone();
    frame.render_stateful_widget(table, chunks[0], &mut table_state);

    let detail = if let Some(skill) = selected {
        let mut lines = vec![
            Line::from(vec![
                label_span("Skill: "),
                Span::styled(
                    skill.name.clone(),
                    Style::default().fg(Theme::ACCENT_STRONG),
                ),
            ]),
            Line::from(vec![
                label_span("Source: "),
                Span::styled(skill.source_kind.label(), Style::default().fg(Theme::TEXT)),
                Span::raw("   "),
                label_span("Repairable: "),
                Span::styled(
                    if skill.repairable { "yes" } else { "no" },
                    Style::default().fg(Theme::TEXT),
                ),
            ]),
            Line::from(vec![
                label_span("Status: "),
                Span::styled(skill.status.label(), Style::default().fg(Theme::TEXT)),
                Span::raw("   "),
                label_span("Action: "),
                Span::styled(
                    format!("{:?}", skill.action),
                    Style::default().fg(Theme::TEXT),
                ),
            ]),
            Line::from(vec![
                label_span("Description: "),
                Span::styled(
                    skill
                        .description
                        .as_deref()
                        .unwrap_or("not provided")
                        .to_string(),
                    Style::default().fg(Theme::TEXT),
                ),
            ]),
            Line::from(vec![
                label_span("Project version: "),
                Span::styled(
                    skill
                        .project_version
                        .as_deref()
                        .unwrap_or("missing")
                        .to_string(),
                    Style::default().fg(Theme::TEXT),
                ),
            ]),
            Line::from(vec![
                label_span("Template version: "),
                Span::styled(
                    skill
                        .template_version
                        .as_deref()
                        .unwrap_or("missing")
                        .to_string(),
                    Style::default().fg(Theme::TEXT),
                ),
            ]),
            Line::from(vec![
                label_span("Skill path: "),
                Span::styled(skill.project_path.clone(), Style::default().fg(Theme::TEXT)),
            ]),
            Line::from(vec![
                label_span("Template path: "),
                Span::styled(
                    skill
                        .template_path
                        .as_deref()
                        .unwrap_or("not found")
                        .to_string(),
                    Style::default().fg(Theme::TEXT),
                ),
            ]),
        ];
        if let Some(detail) = &skill.detail {
            lines.push(Line::from(vec![
                label_span("Detail: "),
                Span::styled(detail.clone(), Style::default().fg(Theme::WARNING)),
            ]));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "SKILL.md",
            Style::default()
                .fg(Theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        )));
        lines.extend(skill_content_lines(&skill.project_path));
        lines
    } else {
        vec![Line::from(format!(
            "No skills match the {} filter.",
            app.skills.filter.label()
        ))]
    };

    let detail = Paragraph::new(detail)
        .wrap(Wrap { trim: false })
        .scroll((app.skills.detail_scroll, 0))
        .block(themed_block("Skill Detail"));
    frame.render_widget(detail, chunks[1]);
}

pub(in crate::tui) fn filtered_skills(app: &App) -> Vec<&mem_skills::SkillVersionInfo> {
    app.meta
        .skill_inventory
        .skills
        .iter()
        .filter(|skill| app.skills.filter.matches(skill))
        .collect()
}

fn filter_spans(active: SkillsFilter) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (index, filter) in SkillsFilter::ALL.iter().copied().enumerate() {
        if index > 0 {
            spans.push(Span::raw("  "));
        }
        let style = if filter == active {
            Style::default()
                .fg(Theme::SELECTION_FG)
                .bg(Theme::SELECTION_BG)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Theme::MUTED)
        };
        spans.push(Span::styled(format!(" {} ", filter.label()), style));
    }
    spans
}

pub(in crate::tui) fn update(
    _event: &Event,
    _state: &mut SkillsTabState,
    _ctx: &mut TabContext,
) -> TabAction {
    TabAction::None
}

fn skill_content_lines(project_path: &str) -> Vec<Line<'static>> {
    let path = std::path::Path::new(project_path).join("SKILL.md");
    match fs::read_to_string(&path) {
        Ok(content) => content
            .lines()
            .take(240)
            .map(|line| {
                Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(Theme::TEXT),
                ))
            })
            .collect(),
        Err(_) => vec![Line::from(Span::styled(
            "No SKILL.md content is available.",
            Style::default().fg(Theme::MUTED),
        ))],
    }
}
