// SPDX-License-Identifier: AGPL-3.0-or-later

// SPDX-License-Identifier: AGPL-3.0-or-later

use mem_config::Profile;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Tabs, Wrap},
};

use super::{
    app::*,
    tabs::{
        TabRenderContext, activity::draw_activity_tab, agents::draw_agents_tab,
        automations::draw_automations_tab, embeddings::draw_embeddings_tab,
        errors::draw_errors_tab, memories::draw_memories_tab, project::draw_project_tab,
        query::draw_query_tab, resume::draw_resume_tab, review::draw_review_tab,
        skills::draw_skills_tab, watchers::draw_watchers_tab,
    },
    theme::{Theme, themed_block},
};

mod activity;
mod agents;
mod common;
mod errors;
mod help;
mod memories;
mod query;
mod status;
mod watchers;

pub(in crate::tui) use activity::*;
pub(in crate::tui) use agents::*;
pub(in crate::tui) use common::*;
pub(in crate::tui) use errors::*;
pub(in crate::tui) use help::*;
pub(in crate::tui) use memories::*;
pub(in crate::tui) use query::*;
pub(in crate::tui) use status::*;
pub(in crate::tui) use watchers::*;

pub(in crate::tui) fn draw(frame: &mut ratatui::Frame<'_>, app: &App) {
    frame.render_widget(
        Block::default().style(Style::default().bg(Theme::BACKGROUND)),
        frame.area(),
    );

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(footer_height(app.meta.profile)),
        ])
        .split(frame.area());

    let titles = VISIBLE_TABS
        .into_iter()
        .map(|tab| Line::from(Span::styled(tab.label(), Style::default().fg(Theme::TEXT))))
        .collect::<Vec<_>>();
    let title = match app.meta.profile {
        Profile::Dev => format!("Memory Layer TUI [dev] - project {}", app.project),
        Profile::Prod => format!("Memory Layer TUI - project {}", app.project),
    };
    let tabs = Tabs::new(titles)
        .select(app.active_tab.index())
        .block(themed_block(title).borders(Borders::ALL))
        .style(Style::default().fg(Theme::MUTED).bg(Theme::PANEL))
        .highlight_style(
            Style::default()
                .fg(Theme::SELECTION_FG)
                .bg(Theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_widget(tabs, chunks[0]);

    let control_line = if app.chrome.help.help_open {
        Line::from(vec![
            accent_span("back "),
            Span::styled("h/Esc  ", Style::default().fg(Theme::TEXT)),
            accent_span("scroll "),
            Span::styled("j/k PgUp/PgDn  ", Style::default().fg(Theme::TEXT)),
            accent_span("jump "),
            Span::styled("Home/End  ", Style::default().fg(Theme::TEXT)),
            Span::styled(
                format!("showing {} help", app.chrome.help.help_tab.label()),
                Style::default().fg(Theme::MUTED),
            ),
        ])
    } else {
        let mut spans = match app.active_tab {
            TabKind::Resume => vec![
                accent_span("refresh "),
                Span::styled("r  ", Style::default().fg(Theme::TEXT)),
                accent_span("scroll "),
                Span::styled("j/k PgUp/PgDn Home", Style::default().fg(Theme::TEXT)),
            ],
            TabKind::Memories => vec![
                accent_span("search=/ "),
                Span::styled(
                    display_filter(&app.filters.text),
                    Style::default().fg(Theme::TEXT),
                ),
                Span::raw("  "),
                accent_span("tag=g "),
                Span::styled(
                    display_filter(&app.filters.tag),
                    Style::default().fg(Theme::TEXT),
                ),
                Span::raw("  "),
                accent_span("status=s "),
                status_span(app.filters.status.label()),
                Span::raw("  "),
                accent_span("type=t "),
                memory_type_span_from_label(app.filters.memory_type.label()),
                Span::raw("  "),
                accent_span("focus "),
                Span::styled(
                    match app.memories.memories_focus {
                        MemoriesFocus::List => "list",
                        MemoriesFocus::Detail => "detail",
                    },
                    Style::default()
                        .fg(match app.memories.memories_focus {
                            MemoriesFocus::List => Theme::ACCENT,
                            MemoriesFocus::Detail => Theme::ACCENT_STRONG,
                        })
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    match app.memories.memories_focus {
                        MemoriesFocus::List => {
                            "Enter=detail  j/k=select  PgUp/PgDn/Home/End=scroll  clear=x curate=c reindex=i reembed=e archive=a delete=D history=H"
                        }
                        MemoriesFocus::Detail => {
                            "Enter/Esc=list  j/k=scroll  PgUp/PgDn/Home/End=scroll  clear=x curate=c reindex=i reembed=e archive=a delete=D history=H"
                        }
                    },
                    Style::default().fg(Theme::MUTED),
                ),
            ],
            TabKind::Agents => vec![
                accent_span("auto-refresh "),
                Span::styled("2s  ", Style::default().fg(Theme::TEXT)),
                accent_span("select "),
                Span::styled("j/k  ", Style::default().fg(Theme::TEXT)),
                accent_span("detail "),
                Span::styled("PgUp/PgDn Home  ", Style::default().fg(Theme::TEXT)),
                Span::styled(
                    "read-only agent/session monitor inspired by abtop",
                    Style::default().fg(Theme::MUTED),
                ),
            ],
            TabKind::Query => vec![
                accent_span("new=Enter/? "),
                Span::styled(
                    display_filter(&current_query_display(app)),
                    Style::default().fg(Theme::TEXT),
                ),
                Span::raw("  "),
                Span::styled("j/k move result", Style::default().fg(Theme::MUTED)),
                Span::raw("  "),
                Span::styled(
                    "Up/Down history while editing",
                    Style::default().fg(Theme::MUTED),
                ),
            ],
            TabKind::Activity => vec![
                accent_span("brief "),
                Span::styled(
                    "g deterministic / L llm  ",
                    Style::default().fg(Theme::TEXT),
                ),
                accent_span("audit "),
                Span::styled("A  ", Style::default().fg(Theme::TEXT)),
                accent_span("refresh "),
                Span::styled("r  ", Style::default().fg(Theme::TEXT)),
                accent_span("move "),
                Span::styled("j/k PgUp/PgDn", Style::default().fg(Theme::TEXT)),
            ],
            TabKind::Errors => vec![
                accent_span("refresh "),
                Span::styled("r  ", Style::default().fg(Theme::TEXT)),
                accent_span("move "),
                Span::styled("j/k  ", Style::default().fg(Theme::TEXT)),
                accent_span("detail "),
                Span::styled("PgUp/PgDn Home  ", Style::default().fg(Theme::TEXT)),
                Span::styled(
                    "persisted backend diagnostics plus session-local TUI errors",
                    Style::default().fg(Theme::MUTED),
                ),
            ],
            TabKind::Project => vec![
                accent_span("scroll "),
                Span::styled("j/k  ", Style::default().fg(Theme::TEXT)),
                accent_span("page "),
                Span::styled("PgUp/PgDn  ", Style::default().fg(Theme::TEXT)),
                accent_span("jump "),
                Span::styled("Home", Style::default().fg(Theme::TEXT)),
            ],
            TabKind::Review => vec![
                accent_span("move "),
                Span::styled("j/k [ ]  ", Style::default().fg(Theme::TEXT)),
                accent_span("approve "),
                Span::styled("y  ", Style::default().fg(Theme::TEXT)),
                accent_span("reject "),
                Span::styled("n  ", Style::default().fg(Theme::TEXT)),
                accent_span("policy "),
                Span::styled("p  ", Style::default().fg(Theme::TEXT)),
                accent_span("refresh "),
                Span::styled("r", Style::default().fg(Theme::TEXT)),
            ],
            TabKind::Watchers => vec![
                accent_span("scroll "),
                Span::styled("j/k  ", Style::default().fg(Theme::TEXT)),
                accent_span("page "),
                Span::styled("PgUp/PgDn  ", Style::default().fg(Theme::TEXT)),
                accent_span("jump "),
                Span::styled("Home", Style::default().fg(Theme::TEXT)),
            ],
            TabKind::Skills => vec![
                accent_span("move "),
                Span::styled("j/k  ", Style::default().fg(Theme::TEXT)),
                accent_span("filter "),
                Span::styled("f/F  ", Style::default().fg(Theme::TEXT)),
                accent_span("detail "),
                Span::styled("PgUp/PgDn Home  ", Style::default().fg(Theme::TEXT)),
                accent_span("repair "),
                Span::styled("u  ", Style::default().fg(Theme::TEXT)),
                accent_span("refresh "),
                Span::styled("r", Style::default().fg(Theme::TEXT)),
            ],
            TabKind::Automations => vec![
                accent_span("move "),
                Span::styled("j/k  ", Style::default().fg(Theme::TEXT)),
                accent_span("detail "),
                Span::styled("PgUp/PgDn Home  ", Style::default().fg(Theme::TEXT)),
                accent_span("refresh "),
                Span::styled("r", Style::default().fg(Theme::TEXT)),
            ],
            TabKind::Embeddings => vec![
                accent_span("move "),
                Span::styled("j/k  ", Style::default().fg(Theme::TEXT)),
                accent_span("toggle "),
                Span::styled("Enter  ", Style::default().fg(Theme::TEXT)),
                accent_span("create "),
                Span::styled("c  ", Style::default().fg(Theme::TEXT)),
                accent_span("embed "),
                Span::styled("e  ", Style::default().fg(Theme::TEXT)),
                accent_span("reindex "),
                Span::styled("I  ", Style::default().fg(Theme::TEXT)),
                accent_span("refresh "),
                Span::styled("r", Style::default().fg(Theme::TEXT)),
            ],
        };
        spans.push(Span::raw("  "));
        spans.push(accent_span("help "));
        spans.push(Span::styled("h", Style::default().fg(Theme::TEXT)));
        Line::from(spans)
    };
    let filter_bar = Paragraph::new(vec![control_line])
        .style(Style::default().bg(Theme::PANEL_ALT))
        .block(themed_block(if app.chrome.help.help_open {
            "Help Controls"
        } else {
            match &app.chrome.input_mode {
                InputMode::Normal => "Controls",
                InputMode::Search(value) => {
                    if value.is_empty() {
                        "Search Input"
                    } else {
                        "Search Input (editing)"
                    }
                }
                InputMode::Tag(value) => {
                    if value.is_empty() {
                        "Tag Filter Input"
                    } else {
                        "Tag Filter Input (editing)"
                    }
                }
                InputMode::Query(value) => {
                    if value.is_empty() {
                        "Query Input"
                    } else {
                        "Query Input (editing)"
                    }
                }
            }
        }));
    frame.render_widget(filter_bar, chunks[1]);

    if app.chrome.help.help_open {
        draw_help_tab(frame, app, chunks[2]);
    } else if app.service.health_ok {
        let tab_ctx = TabRenderContext::new(app);
        match app.active_tab {
            TabKind::Resume => draw_resume_tab(frame, &tab_ctx, chunks[2]),
            TabKind::Memories => draw_memories_tab(frame, &tab_ctx, chunks[2]),
            TabKind::Agents => draw_agents_tab(frame, &tab_ctx, chunks[2]),
            TabKind::Query => draw_query_tab(frame, &tab_ctx, chunks[2]),
            TabKind::Activity => draw_activity_tab(frame, &tab_ctx, chunks[2]),
            TabKind::Errors => draw_errors_tab(frame, &tab_ctx, chunks[2]),
            TabKind::Project => draw_project_tab(frame, &tab_ctx, chunks[2]),
            TabKind::Review => draw_review_tab(frame, &tab_ctx, chunks[2]),
            TabKind::Watchers => draw_watchers_tab(frame, &tab_ctx, chunks[2]),
            TabKind::Skills => draw_skills_tab(frame, &tab_ctx, chunks[2]),
            TabKind::Automations => draw_automations_tab(frame, &tab_ctx, chunks[2]),
            TabKind::Embeddings => draw_embeddings_tab(frame, &tab_ctx, chunks[2]),
        }
    } else {
        draw_backend_recovery(frame, app, chunks[2]);
    }

    let footer_constraints = match app.meta.profile {
        Profile::Dev => vec![
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(1),
        ],
        Profile::Prod => vec![Constraint::Length(3), Constraint::Length(1)],
    };
    let footer_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(footer_constraints)
        .split(chunks[3]);

    let footer = Paragraph::new(app.chrome.status_message.clone())
        .style(status_message_style(app))
        .wrap(Wrap { trim: false })
        .block(themed_block("Status"));
    frame.render_widget(footer, footer_chunks[0]);
    match app.meta.profile {
        Profile::Dev => {
            draw_bottom_status_bar(frame, app, footer_chunks[1]);
            draw_dev_status_bar(frame, app, footer_chunks[2]);
        }
        Profile::Prod => draw_bottom_status_bar(frame, app, footer_chunks[1]),
    }
}
