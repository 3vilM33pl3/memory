use super::super::app::*;
use super::super::theme::{Theme, themed_block};
use crate::commands::memory_ops::SourceKindString;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};

pub(in crate::tui) fn draw_project_tab(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(12),
            Constraint::Length(8),
            Constraint::Min(7),
        ])
        .split(area);

    let summary = Paragraph::new(vec![
        metric_line(
            "Project",
            Span::styled(&app.meta.overview.project, Style::default().fg(Theme::TEXT)),
        ),
        metric_line(
            "Latest plan",
            Span::styled(latest_plan_display(app), Style::default().fg(Theme::TEXT)),
        ),
        Line::from(vec![
            label_span("Service: "),
            service_span(&app.meta.overview.service_status),
            Span::raw("   "),
            label_span("Database: "),
            service_span(&app.meta.overview.database_status),
        ]),
        Line::from(vec![
            label_span("Memories: "),
            Span::styled(
                format!(
                    "{} total / {} active / {} archived",
                    app.meta.overview.memory_entries_total,
                    app.meta.overview.active_memories,
                    app.meta.overview.archived_memories
                ),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
        Line::from(vec![
            label_span("Confidence bins: "),
            Span::styled(
                format!(
                    "{} high / {} medium / {} low",
                    app.meta.overview.high_confidence_memories,
                    app.meta.overview.medium_confidence_memories,
                    app.meta.overview.low_confidence_memories
                ),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
        metric_line(
            "Recent 7d",
            Span::styled(
                format!(
                    "{} memories / {} captures",
                    app.meta.overview.recent_memories_7d, app.meta.overview.recent_captures_7d
                ),
                Style::default().fg(Theme::TEXT),
            ),
        ),
        metric_line(
            "Raw captures",
            Span::styled(
                format!(
                    "{} total / {} uncurated",
                    app.meta.overview.raw_captures_total, app.meta.overview.uncurated_raw_captures
                ),
                Style::default().fg(Theme::TEXT),
            ),
        ),
        metric_line(
            "Tasks / Sessions / Runs",
            Span::styled(
                format!(
                    "{} / {} / {}",
                    app.meta.overview.tasks_total,
                    app.meta.overview.sessions_total,
                    app.meta.overview.curation_runs_total
                ),
                Style::default().fg(Theme::TEXT),
            ),
        ),
        metric_line(
            "Last memory / curation",
            Span::styled(
                format!(
                    "{} / {}",
                    format_timestamp(app.meta.overview.last_memory_at),
                    format_timestamp(app.meta.overview.last_curation_at)
                ),
                Style::default().fg(Theme::TEXT),
            ),
        ),
        metric_line(
            "Last capture / oldest uncurated",
            Span::styled(
                format!(
                    "{} / {}",
                    format_timestamp(app.meta.overview.last_capture_at),
                    app.meta
                        .overview
                        .oldest_uncurated_capture_age_hours
                        .map(|hours| format!("{hours}h"))
                        .unwrap_or_else(|| "n/a".to_string())
                ),
                Style::default().fg(Theme::TEXT),
            ),
        ),
        metric_line(
            "Tool versions",
            Span::styled(
                format!(
                    "memory {} / service {} / watcher {}",
                    app.meta.versions.mem_cli,
                    app.meta.versions.mem_service,
                    app.meta.versions.memory_watch
                ),
                Style::default().fg(Theme::TEXT),
            ),
        ),
        metric_line(
            "Skill bundle",
            Span::styled(
                format!(
                    "v{} {} ({})",
                    app.meta.skill_inventory.bundle_version,
                    app.meta.skill_inventory.status.label(),
                    app.meta.skill_inventory.summary
                ),
                Style::default().fg(skill_bundle_status_color(app.meta.skill_inventory.status)),
            ),
        ),
        metric_line(
            "Automation",
            Span::styled(
                app.meta
                    .overview
                    .automation
                    .as_ref()
                    .map(format_automation_status)
                    .unwrap_or_else(|| "not configured".to_string()),
                Style::default().fg(Theme::TEXT),
            ),
        ),
        metric_line(
            "Watchers",
            Span::styled(watcher_summary_text(app), Style::default().fg(Theme::TEXT)),
        ),
        metric_line(
            "Curation policy",
            Span::styled(
                format!(
                    "{} / {} pending (see Review tab)",
                    app.review.replacement_policy, app.meta.overview.pending_replacement_proposals
                ),
                Style::default().fg(Theme::TEXT),
            ),
        ),
    ])
    .scroll((app.project_tab.project_scroll, 0))
    .style(Style::default().bg(Theme::PANEL))
    .block(themed_block(format!(
        "Overview (scroll {})",
        app.project_tab.project_scroll
    )));
    frame.render_widget(summary, chunks[0]);

    let mid = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(34),
        ])
        .split(chunks[1]);

    frame.render_widget(
        Paragraph::new(lines_for_named_counts(
            app.meta
                .overview
                .memory_type_breakdown
                .iter()
                .map(|item| (item.memory_type.to_string(), item.count))
                .collect(),
            "No memory entries yet.",
        ))
        .style(Style::default().bg(Theme::PANEL_ALT))
        .block(themed_block("Memory Types")),
        mid[0],
    );
    frame.render_widget(
        Paragraph::new(lines_for_named_counts(
            app.meta
                .overview
                .source_kind_breakdown
                .iter()
                .map(|item| {
                    (
                        item.source_kind.source_kind_string().to_string(),
                        item.count,
                    )
                })
                .collect(),
            "No sources yet.",
        ))
        .style(Style::default().bg(Theme::PANEL_ALT))
        .block(themed_block("Source Kinds")),
        mid[1],
    );
    frame.render_widget(
        Paragraph::new(lines_for_named_counts(
            app.meta
                .overview
                .top_tags
                .iter()
                .map(|item| (item.name.clone(), item.count))
                .collect(),
            "No tags yet.",
        ))
        .style(Style::default().bg(Theme::PANEL_ALT))
        .block(themed_block("Top Tags")),
        mid[2],
    );

    let bottom = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(8), Constraint::Length(7)])
        .split(chunks[2]);

    let bottom_top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
        .split(bottom[0]);

    frame.render_widget(
        Paragraph::new(lines_for_named_counts(
            app.meta
                .overview
                .top_files
                .iter()
                .map(|item| (item.name.clone(), item.count))
                .collect(),
            "No file provenance yet.",
        ))
        .style(Style::default().bg(Theme::PANEL_ALT))
        .block(themed_block("Top Files")),
        bottom_top[0],
    );
    frame.render_widget(
        Paragraph::new(recent_activity_lines(app))
            .style(Style::default().bg(Theme::PANEL_ALT))
            .block(themed_block("Recent Activity")),
        bottom_top[1],
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "Actions",
                Style::default().fg(Theme::ACCENT_STRONG),
            )),
            Line::from(Span::styled(
                "c curate project",
                Style::default().fg(Theme::TEXT),
            )),
            Line::from(Span::styled(
                "i reindex search chunks",
                Style::default().fg(Theme::TEXT),
            )),
            Line::from(Span::styled(
                "e materialize active-space vectors",
                Style::default().fg(Theme::TEXT),
            )),
            Line::from(Span::styled(
                "a archive low-value memories",
                Style::default().fg(Theme::TEXT),
            )),
            Line::from(Span::styled(
                "Review tab: y approve / n reject / p cycle policy",
                Style::default().fg(Theme::MUTED),
            )),
            Line::from(Span::styled("r refresh", Style::default().fg(Theme::TEXT))),
        ])
        .style(Style::default().bg(Theme::PANEL_ALT))
        .block(themed_block("Operations")),
        bottom[1],
    );
}
