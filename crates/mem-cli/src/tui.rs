use std::{io, time::Duration};

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use mem_api::{
    MemoryEntryResponse, MemoryStatus, ProjectMemoriesResponse, ProjectMemoryListItem,
    ProjectOverviewResponse,
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState, Tabs, Wrap},
};

use crate::{ApiClient, SourceKindString};

pub(crate) async fn run(api: ApiClient, project: String) -> Result<()> {
    let mut terminal = setup_terminal()?;
    let mut app = App::new(project);
    app.refresh(&api).await;

    loop {
        terminal.draw(|frame| draw(frame, &app))?;
        if event::poll(Duration::from_millis(200))? {
            match event::read()? {
                Event::Key(key) if should_quit(key) => break,
                Event::Key(key) => {
                    if app.handle_key(key, &api).await? {
                        break;
                    }
                }
                _ => {}
            }
        }
    }

    restore_terminal(terminal)
}

struct App {
    project: String,
    active_tab: TabKind,
    memories: ProjectMemoriesResponse,
    overview: ProjectOverviewResponse,
    selected_detail: Option<MemoryEntryResponse>,
    selected_index: usize,
    table_state: TableState,
    last_error: Option<String>,
}

impl App {
    fn new(project: String) -> Self {
        let mut table_state = TableState::default();
        table_state.select(Some(0));
        Self {
            project: project.clone(),
            active_tab: TabKind::Memories,
            memories: ProjectMemoriesResponse {
                project: project.clone(),
                total: 0,
                items: Vec::new(),
            },
            overview: ProjectOverviewResponse {
                project,
                service_status: "unknown".to_string(),
                database_status: "unknown".to_string(),
                memory_entries_total: 0,
                active_memories: 0,
                archived_memories: 0,
                raw_captures_total: 0,
                uncurated_raw_captures: 0,
                tasks_total: 0,
                sessions_total: 0,
                curation_runs_total: 0,
                last_memory_at: None,
                last_capture_at: None,
                last_curation_at: None,
                memory_type_breakdown: Vec::new(),
                source_kind_breakdown: Vec::new(),
            },
            selected_detail: None,
            selected_index: 0,
            table_state,
            last_error: None,
        }
    }

    async fn refresh(&mut self, api: &ApiClient) {
        self.last_error = None;

        match api.health().await {
            Ok(_) => {
                self.overview.service_status = "ok".to_string();
                self.overview.database_status = "up".to_string();
            }
            Err(error) => {
                self.overview.service_status = "error".to_string();
                self.overview.database_status = "unknown".to_string();
                self.last_error = Some(error.to_string());
            }
        }

        match api.project_overview(&self.project).await {
            Ok(overview) => self.overview = overview,
            Err(error) => self.last_error = Some(error.to_string()),
        }

        match api.project_memories(&self.project).await {
            Ok(memories) => {
                self.memories = memories;
                self.clamp_selection();
                self.fetch_selected_detail(api).await;
            }
            Err(error) => {
                self.memories.items.clear();
                self.memories.total = 0;
                self.selected_detail = None;
                self.last_error = Some(error.to_string());
            }
        }
    }

    async fn handle_key(&mut self, key: KeyEvent, api: &ApiClient) -> Result<bool> {
        match key.code {
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') if key.modifiers.is_empty() => {
                self.active_tab = self.active_tab.next();
            }
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') if key.modifiers.is_empty() => {
                self.active_tab = self.active_tab.prev();
            }
            KeyCode::Char('r') if key.modifiers.is_empty() => self.refresh(api).await,
            KeyCode::Down | KeyCode::Char('j') if self.active_tab == TabKind::Memories => {
                if self.selected_index + 1 < self.memories.items.len() {
                    self.selected_index += 1;
                    self.table_state.select(Some(self.selected_index));
                    self.fetch_selected_detail(api).await;
                }
            }
            KeyCode::Up | KeyCode::Char('k') if self.active_tab == TabKind::Memories => {
                if self.selected_index > 0 {
                    self.selected_index -= 1;
                    self.table_state.select(Some(self.selected_index));
                    self.fetch_selected_detail(api).await;
                }
            }
            KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => return Ok(true),
            _ => {}
        }
        Ok(false)
    }

    async fn fetch_selected_detail(&mut self, api: &ApiClient) {
        self.selected_detail = None;
        if let Some(item) = self.memories.items.get(self.selected_index) {
            match api.memory_detail(&item.id.to_string()).await {
                Ok(detail) => self.selected_detail = Some(detail),
                Err(error) => self.last_error = Some(error.to_string()),
            }
        }
    }

    fn clamp_selection(&mut self) {
        if self.memories.items.is_empty() {
            self.selected_index = 0;
            self.table_state.select(None);
        } else {
            self.selected_index = self.selected_index.min(self.memories.items.len() - 1);
            self.table_state.select(Some(self.selected_index));
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TabKind {
    Memories,
    Project,
}

impl TabKind {
    fn next(self) -> Self {
        match self {
            Self::Memories => Self::Project,
            Self::Project => Self::Memories,
        }
    }

    fn prev(self) -> Self {
        self.next()
    }

    fn index(self) -> usize {
        match self {
            Self::Memories => 0,
            Self::Project => 1,
        }
    }
}

fn draw(frame: &mut ratatui::Frame<'_>, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(2),
        ])
        .split(frame.area());

    let titles = ["Memories", "Project"]
        .into_iter()
        .map(Line::from)
        .collect::<Vec<_>>();
    let tabs = Tabs::new(titles)
        .select(app.active_tab.index())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Memory Layer TUI - project {}", app.project)),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_widget(tabs, chunks[0]);

    match app.active_tab {
        TabKind::Memories => draw_memories_tab(frame, app, chunks[1]),
        TabKind::Project => draw_project_tab(frame, app, chunks[1]),
    }

    let footer = Paragraph::new("Tab/h/l switch tabs  j/k move  r refresh  q exit")
        .block(Block::default().borders(Borders::ALL));
    frame.render_widget(footer, chunks[2]);
}

fn draw_memories_tab(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    let header = Row::new(["Summary", "Type", "Status", "Conf", "Updated"]).style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
    let rows = app.memories.items.iter().map(memory_row);
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(36),
            Constraint::Length(14),
            Constraint::Length(10),
            Constraint::Length(6),
            Constraint::Length(20),
        ],
    )
    .header(header)
    .row_highlight_style(Style::default().bg(Color::Blue))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Memories ({})", app.memories.total)),
    );
    frame.render_stateful_widget(table, chunks[0], &mut app.table_state.clone());

    let detail_text = if let Some(detail) = &app.selected_detail {
        let mut lines = vec![
            Line::from(vec![
                Span::styled("Summary: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(detail.summary.clone()),
            ]),
            Line::from(vec![
                Span::styled("Type: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(detail.memory_type.to_string()),
                Span::raw("   "),
                Span::styled("Status: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(match detail.status {
                    MemoryStatus::Active => "active",
                    MemoryStatus::Archived => "archived",
                }),
            ]),
            Line::from(vec![
                Span::styled(
                    "Confidence: ",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!("{:.2}", detail.confidence)),
                Span::raw("   "),
                Span::styled(
                    "Importance: ",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(detail.importance.to_string()),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Canonical Text",
                Style::default().add_modifier(Modifier::BOLD),
            )]),
            Line::from(detail.canonical_text.clone()),
            Line::from(""),
            Line::from(vec![
                Span::styled("Tags: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(if detail.tags.is_empty() {
                    "none".to_string()
                } else {
                    detail.tags.join(", ")
                }),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Sources",
                Style::default().add_modifier(Modifier::BOLD),
            )]),
        ];

        if detail.sources.is_empty() {
            lines.push(Line::from("No provenance sources recorded."));
        } else {
            for source in &detail.sources {
                let mut parts = vec![source.source_kind.source_kind_string().to_string()];
                if let Some(path) = &source.file_path {
                    parts.push(path.clone());
                }
                if let Some(excerpt) = &source.excerpt {
                    parts.push(excerpt.clone());
                }
                lines.push(Line::from(parts.join(" | ")));
            }
        }
        lines
    } else if app.memories.items.is_empty() {
        vec![Line::from(format!(
            "No memories found for project {}.",
            app.project
        ))]
    } else {
        vec![Line::from("Select a memory to load its details.")]
    };

    let detail = Paragraph::new(detail_text)
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::ALL).title("Detail"));
    frame.render_widget(detail, chunks[1]);
}

fn draw_project_tab(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),
            Constraint::Min(8),
            Constraint::Length(6),
        ])
        .split(area);

    let summary = Paragraph::new(vec![
        Line::from(format!("Project: {}", app.overview.project)),
        Line::from(format!(
            "Service: {}   Database: {}",
            app.overview.service_status, app.overview.database_status
        )),
        Line::from(format!(
            "Memories: {} total / {} active / {} archived",
            app.overview.memory_entries_total,
            app.overview.active_memories,
            app.overview.archived_memories
        )),
        Line::from(format!(
            "Raw captures: {} total / {} uncurated",
            app.overview.raw_captures_total, app.overview.uncurated_raw_captures
        )),
        Line::from(format!(
            "Tasks: {}   Sessions: {}   Curation runs: {}",
            app.overview.tasks_total, app.overview.sessions_total, app.overview.curation_runs_total
        )),
        Line::from(format!(
            "Last memory: {}",
            format_timestamp(app.overview.last_memory_at)
        )),
        Line::from(format!(
            "Last capture: {}   Last curation: {}",
            format_timestamp(app.overview.last_capture_at),
            format_timestamp(app.overview.last_curation_at)
        )),
    ])
    .block(Block::default().borders(Borders::ALL).title("Overview"));
    frame.render_widget(summary, chunks[0]);

    let mid = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    let memory_type_lines = if app.overview.memory_type_breakdown.is_empty() {
        vec![Line::from("No memory entries yet.")]
    } else {
        app.overview
            .memory_type_breakdown
            .iter()
            .map(|item| Line::from(format!("{}: {}", item.memory_type, item.count)))
            .collect()
    };
    let source_kind_lines = if app.overview.source_kind_breakdown.is_empty() {
        vec![Line::from("No sources yet.")]
    } else {
        app.overview
            .source_kind_breakdown
            .iter()
            .map(|item| {
                Line::from(format!(
                    "{}: {}",
                    item.source_kind.source_kind_string(),
                    item.count
                ))
            })
            .collect()
    };

    frame.render_widget(
        Paragraph::new(memory_type_lines)
            .block(Block::default().borders(Borders::ALL).title("Memory Types")),
        mid[0],
    );
    frame.render_widget(
        Paragraph::new(source_kind_lines)
            .block(Block::default().borders(Borders::ALL).title("Source Kinds")),
        mid[1],
    );

    let status = Paragraph::new(match &app.last_error {
        Some(error) => vec![Line::from(error.clone())],
        None => vec![Line::from("Press r to refresh project data.")],
    })
    .wrap(Wrap { trim: false })
    .block(Block::default().borders(Borders::ALL).title("Status"));
    frame.render_widget(status, chunks[2]);
}

fn memory_row(item: &ProjectMemoryListItem) -> Row<'static> {
    Row::new(vec![
        Cell::from(item.summary.clone()),
        Cell::from(item.memory_type.to_string()),
        Cell::from(match item.status {
            MemoryStatus::Active => "active".to_string(),
            MemoryStatus::Archived => "archived".to_string(),
        }),
        Cell::from(format!("{:.2}", item.confidence)),
        Cell::from(item.updated_at.format("%Y-%m-%d %H:%M").to_string()),
    ])
}

fn format_timestamp(value: Option<chrono::DateTime<chrono::Utc>>) -> String {
    value
        .map(|value| value.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "n/a".to_string())
}

fn should_quit(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('q'))
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn restore_terminal(mut terminal: Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}
