use super::super::app::*;
use super::super::theme::{Theme, themed_block};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Row, Table, Wrap},
};

pub(in crate::tui) fn draw_embeddings_tab(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(8)])
        .split(area);

    let snapshot = app.embeddings.embedding_backends_snapshot.as_ref();
    let backends = snapshot.map(|s| s.backends.as_slice()).unwrap_or(&[]);
    let configured = backends.len();
    let ready = backends.iter().filter(|b| b.ready).count();
    let not_ready = configured.saturating_sub(ready);
    let active_display = snapshot
        .and_then(|s| s.active.clone())
        .unwrap_or_else(|| "(none)".to_string());
    let create_display = snapshot
        .and_then(|snapshot| {
            snapshot
                .backends
                .get(app.embeddings.embeddings_selected_index)
        })
        .map(|backend| {
            format!(
                "{} for {}",
                if backend.create_enabled { "on" } else { "off" },
                backend.name
            )
        })
        .unwrap_or_else(|| "unknown".to_string());

    let message_line = if app.embeddings.embeddings_creation_toggling {
        Line::from(vec![
            label_span("Status: "),
            Span::styled(
                "toggling automatic embedding creation...",
                Style::default().fg(Theme::ACCENT),
            ),
        ])
    } else if let Some(operation) = &app.embeddings.embeddings_operation {
        Line::from(vec![
            label_span("Status: "),
            Span::styled(
                format!("{operation}..."),
                Style::default().fg(Theme::ACCENT),
            ),
        ])
    } else if let Some(toggling) = &app.embeddings.embeddings_toggling {
        Line::from(vec![
            label_span("Status: "),
            Span::styled(
                format!("toggling {toggling}..."),
                Style::default().fg(Theme::ACCENT),
            ),
        ])
    } else if let Some(msg) = &app.embeddings.embeddings_toggle_message {
        let color = if msg.starts_with("Toggle failed")
            || msg.starts_with("Creation toggle failed")
            || msg.starts_with("Embedding creation failed")
            || msg.starts_with("Reindex failed")
        {
            Theme::DANGER
        } else {
            Theme::SUCCESS
        };
        Line::from(vec![
            label_span("Status: "),
            Span::styled(msg.clone(), Style::default().fg(color)),
        ])
    } else if let Some(err) = &app.embeddings.embedding_backends_error {
        Line::from(vec![
            label_span("Status: "),
            Span::styled(
                format!("refresh failed: {err}"),
                Style::default().fg(Theme::WARNING),
            ),
        ])
    } else {
        Line::from(vec![
            label_span("Status: "),
            Span::styled("idle", Style::default().fg(Theme::MUTED)),
        ])
    };

    let summary = Paragraph::new(vec![
        Line::from(vec![
            label_span("Active: "),
            Span::styled(active_display, Style::default().fg(Theme::ACCENT_STRONG)),
        ]),
        Line::from(vec![
            label_span("Create: "),
            Span::styled(create_display, Style::default().fg(Theme::ACCENT_STRONG)),
            Span::styled(" automatic embeddings", Style::default().fg(Theme::MUTED)),
        ]),
        Line::from(vec![
            label_span("Backends: "),
            Span::styled(
                format!("{configured} configured · {ready} ready · {not_ready} not ready"),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
        message_line,
    ])
    .style(Style::default().bg(Theme::PANEL))
    .block(themed_block("Embedding Backends"));
    frame.render_widget(summary, chunks[0]);

    if backends.is_empty() {
        let body = if app.embeddings.embedding_backends_snapshot.is_some() {
            "No embedding backends configured. Declare them under [[embeddings.backends]] in your memory-layer.toml."
        } else {
            "Loading embedding backends..."
        };
        frame.render_widget(
            Paragraph::new(body)
                .wrap(Wrap { trim: false })
                .style(Style::default().fg(Theme::MUTED).bg(Theme::PANEL_ALT))
                .block(themed_block("Backends")),
            chunks[1],
        );
        return;
    }

    let header = Row::new([
        " ", "NAME", "PROVIDER", "MODEL", "CREATE", "BASE URL", "CHUNKS", "MEMORIES",
    ])
    .style(
        Style::default()
            .fg(Theme::ACCENT_STRONG)
            .bg(Theme::PANEL_ALT)
            .add_modifier(Modifier::BOLD),
    );
    let rows = backends.iter().map(|backend| {
        let marker = if backend.active {
            Span::styled(
                "*",
                Style::default()
                    .fg(Theme::ACCENT_STRONG)
                    .add_modifier(Modifier::BOLD),
            )
        } else if !backend.ready {
            Span::styled("!", Style::default().fg(Theme::DANGER))
        } else {
            Span::raw(" ")
        };
        let base_url = if backend.base_url.trim().is_empty()
            || embedding_base_url_is_default(&backend.provider, &backend.base_url)
        {
            String::new()
        } else {
            backend.base_url.clone()
        };
        let chunks_cell = backend
            .project_chunk_count
            .map(|n| n.to_string())
            .unwrap_or_else(|| "—".to_string());
        let memories_cell = backend
            .project_memory_count
            .map(|n| n.to_string())
            .unwrap_or_else(|| "—".to_string());
        let name_style = if backend.active {
            Style::default()
                .fg(Theme::ACCENT_STRONG)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Theme::TEXT)
        };
        Row::new(vec![
            Line::from(marker),
            Line::from(Span::styled(backend.name.clone(), name_style)),
            Line::from(Span::styled(
                backend.provider.clone(),
                Style::default().fg(Theme::ACCENT),
            )),
            Line::from(Span::styled(
                backend.model.clone(),
                Style::default().fg(Theme::TEXT),
            )),
            Line::from(Span::styled(
                if backend.create_enabled { "on" } else { "off" },
                if backend.create_enabled {
                    Style::default().fg(Theme::SUCCESS)
                } else {
                    Style::default().fg(Theme::MUTED)
                },
            )),
            Line::from(Span::styled(base_url, Style::default().fg(Theme::MUTED))),
            Line::from(Span::styled(chunks_cell, Style::default().fg(Theme::TEXT))),
            Line::from(Span::styled(
                memories_cell,
                Style::default().fg(Theme::TEXT),
            )),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(1),
            Constraint::Length(24),
            Constraint::Length(20),
            Constraint::Length(28),
            Constraint::Length(8),
            Constraint::Min(18),
            Constraint::Length(8),
            Constraint::Length(10),
        ],
    )
    .header(header)
    .row_highlight_style(
        Style::default()
            .bg(Theme::SELECTION_BG)
            .fg(Theme::SELECTION_FG),
    )
    .style(Style::default().bg(Theme::PANEL_ALT))
    .block(themed_block(format!(
        "Backends ({} for project {})",
        backends.len(),
        app.project
    )));
    let mut state = app.embeddings.embeddings_table_state.clone();
    frame.render_stateful_widget(table, chunks[1], &mut state);
}
