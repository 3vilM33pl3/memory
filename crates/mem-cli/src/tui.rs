use std::{io, time::Duration};

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use mem_api::{
    MemoryEntryResponse, MemoryStatus, MemoryType, NamedCount, ProjectMemoriesResponse,
    ProjectMemoryListItem, ProjectOverviewResponse,
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
                Event::Key(key) if should_quit(key, &app) => break,
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
    all_memories: Vec<ProjectMemoryListItem>,
    filtered_memories: Vec<ProjectMemoryListItem>,
    total_memories: i64,
    overview: ProjectOverviewResponse,
    selected_detail: Option<MemoryEntryResponse>,
    selected_index: usize,
    table_state: TableState,
    status_message: String,
    health_ok: bool,
    filters: Filters,
    input_mode: InputMode,
}

impl App {
    fn new(project: String) -> Self {
        let mut table_state = TableState::default();
        table_state.select(Some(0));
        Self {
            project: project.clone(),
            active_tab: TabKind::Memories,
            all_memories: Vec::new(),
            filtered_memories: Vec::new(),
            total_memories: 0,
            overview: empty_overview(project),
            selected_detail: None,
            selected_index: 0,
            table_state,
            status_message: "Press r to refresh, q to exit.".to_string(),
            health_ok: false,
            filters: Filters::default(),
            input_mode: InputMode::Normal,
        }
    }

    async fn refresh(&mut self, api: &ApiClient) {
        self.status_message = "Refreshing...".to_string();
        self.selected_detail = None;

        self.health_ok = api.health().await.is_ok();
        if !self.health_ok {
            self.overview.service_status = "error".to_string();
            self.overview.database_status = "unknown".to_string();
        }

        match api.project_overview(&self.project).await {
            Ok(overview) => self.overview = overview,
            Err(error) => self.status_message = error.to_string(),
        }

        match api.project_memories(&self.project).await {
            Ok(ProjectMemoriesResponse {
                project: _,
                total,
                items,
            }) => {
                self.total_memories = total;
                self.all_memories = items;
                self.apply_filters();
                self.fetch_selected_detail(api).await;
                self.status_message = format!(
                    "Loaded {} visible memories ({} total).",
                    self.filtered_memories.len(),
                    self.total_memories
                );
            }
            Err(error) => {
                self.all_memories.clear();
                self.filtered_memories.clear();
                self.total_memories = 0;
                self.selected_detail = None;
                self.table_state.select(None);
                self.status_message = error.to_string();
            }
        }
    }

    async fn handle_key(&mut self, key: KeyEvent, api: &ApiClient) -> Result<bool> {
        let current_input = std::mem::take(&mut self.input_mode);
        match current_input {
            InputMode::Normal => {}
            InputMode::Search(mut buffer) => {
                self.handle_text_input(key, api, TextInputKind::Search, &mut buffer)
                    .await?;
                return Ok(false);
            }
            InputMode::Tag(mut buffer) => {
                self.handle_text_input(key, api, TextInputKind::Tag, &mut buffer)
                    .await?;
                return Ok(false);
            }
        }

        match key.code {
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') if key.modifiers.is_empty() => {
                self.active_tab = self.active_tab.next();
            }
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') if key.modifiers.is_empty() => {
                self.active_tab = self.active_tab.prev();
            }
            KeyCode::Char('r') if key.modifiers.is_empty() => self.refresh(api).await,
            KeyCode::Down | KeyCode::Char('j') if self.active_tab == TabKind::Memories => {
                self.move_selection(1, api).await;
            }
            KeyCode::Up | KeyCode::Char('k') if self.active_tab == TabKind::Memories => {
                self.move_selection(-1, api).await;
            }
            KeyCode::Char('/') if key.modifiers.is_empty() => {
                self.input_mode = InputMode::Search(self.filters.text.clone());
                self.status_message =
                    "Type search text, Enter to apply, Esc to cancel.".to_string();
            }
            KeyCode::Char('g') if key.modifiers.is_empty() => {
                self.input_mode = InputMode::Tag(self.filters.tag.clone());
                self.status_message =
                    "Type tag filter text, Enter to apply, Esc to cancel.".to_string();
            }
            KeyCode::Char('t') if key.modifiers.is_empty() => {
                self.filters.memory_type = self.filters.memory_type.next();
                self.apply_filters();
                self.fetch_selected_detail(api).await;
            }
            KeyCode::Char('s') if key.modifiers.is_empty() => {
                self.filters.status = self.filters.status.next();
                self.apply_filters();
                self.fetch_selected_detail(api).await;
            }
            KeyCode::Char('x') if key.modifiers.is_empty() => {
                self.filters = Filters::default();
                self.input_mode = InputMode::Normal;
                self.apply_filters();
                self.fetch_selected_detail(api).await;
                self.status_message = "Cleared filters.".to_string();
            }
            KeyCode::Char('c') if key.modifiers.is_empty() => {
                let response = api.curate(&self.project).await?;
                self.status_message = format!(
                    "Curated {} captures into {} memories.",
                    response.input_count, response.output_count
                );
                self.refresh(api).await;
            }
            KeyCode::Char('i') if key.modifiers.is_empty() => {
                let response = api.reindex(&self.project).await?;
                self.status_message =
                    format!("Reindexed {} memory entries.", response.reindexed_entries);
                self.refresh(api).await;
            }
            KeyCode::Char('a') if key.modifiers.is_empty() => {
                let response = api.archive_low_value(&self.project).await?;
                self.status_message = format!(
                    "Archived {} low-value memories using default thresholds.",
                    response.archived_count
                );
                self.refresh(api).await;
            }
            KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => return Ok(true),
            _ => {}
        }
        Ok(false)
    }

    async fn handle_text_input(
        &mut self,
        key: KeyEvent,
        api: &ApiClient,
        kind: TextInputKind,
        buffer: &mut String,
    ) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.status_message = "Cancelled input mode.".to_string();
            }
            KeyCode::Enter => {
                match kind {
                    TextInputKind::Search => self.filters.text = buffer.clone(),
                    TextInputKind::Tag => self.filters.tag = buffer.clone(),
                }
                self.input_mode = InputMode::Normal;
                self.apply_filters();
                self.fetch_selected_detail(api).await;
                self.status_message = "Applied filter.".to_string();
            }
            KeyCode::Backspace => {
                buffer.pop();
                self.input_mode = kind.wrap(buffer.clone());
            }
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                buffer.push(ch);
                self.input_mode = kind.wrap(buffer.clone());
            }
            _ => {
                self.input_mode = kind.wrap(buffer.clone());
            }
        }
        Ok(())
    }

    async fn move_selection(&mut self, delta: isize, api: &ApiClient) {
        if self.filtered_memories.is_empty() {
            return;
        }
        let next = (self.selected_index as isize + delta)
            .clamp(0, self.filtered_memories.len().saturating_sub(1) as isize)
            as usize;
        if next != self.selected_index {
            self.selected_index = next;
            self.table_state.select(Some(self.selected_index));
            self.fetch_selected_detail(api).await;
        }
    }

    async fn fetch_selected_detail(&mut self, api: &ApiClient) {
        self.selected_detail = None;
        if let Some(item) = self.filtered_memories.get(self.selected_index) {
            match api.memory_detail(&item.id.to_string()).await {
                Ok(detail) => self.selected_detail = Some(detail),
                Err(error) => self.status_message = error.to_string(),
            }
        }
    }

    fn apply_filters(&mut self) {
        self.filtered_memories = self
            .all_memories
            .iter()
            .filter(|item| self.filters.matches(item))
            .cloned()
            .collect();

        if self.filtered_memories.is_empty() {
            self.selected_index = 0;
            self.table_state.select(None);
            self.selected_detail = None;
        } else {
            self.selected_index = self.selected_index.min(self.filtered_memories.len() - 1);
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

#[derive(Clone, Default)]
struct Filters {
    text: String,
    tag: String,
    status: StatusFilter,
    memory_type: TypeFilter,
}

impl Filters {
    fn matches(&self, item: &ProjectMemoryListItem) -> bool {
        if !self.text.is_empty() {
            let text = self.text.to_lowercase();
            let haystack = format!("{} {}", item.summary, item.preview).to_lowercase();
            if !haystack.contains(&text) {
                return false;
            }
        }

        if !self.tag.is_empty() {
            let wanted = self.tag.to_lowercase();
            if !item
                .tags
                .iter()
                .any(|tag| tag.to_lowercase().contains(&wanted))
            {
                return false;
            }
        }

        if !self.status.matches(item.status.clone()) {
            return false;
        }

        self.memory_type.matches(&item.memory_type)
    }
}

#[derive(Clone, Default)]
enum InputMode {
    #[default]
    Normal,
    Search(String),
    Tag(String),
}

#[derive(Clone, Copy)]
enum TextInputKind {
    Search,
    Tag,
}

impl TextInputKind {
    fn wrap(self, value: String) -> InputMode {
        match self {
            Self::Search => InputMode::Search(value),
            Self::Tag => InputMode::Tag(value),
        }
    }
}

#[derive(Clone, Default)]
enum StatusFilter {
    #[default]
    All,
    Active,
    Archived,
}

impl StatusFilter {
    fn next(&self) -> Self {
        match self {
            Self::All => Self::Active,
            Self::Active => Self::Archived,
            Self::Archived => Self::All,
        }
    }

    fn matches(&self, status: MemoryStatus) -> bool {
        matches!(
            (self, status),
            (Self::All, _)
                | (Self::Active, MemoryStatus::Active)
                | (Self::Archived, MemoryStatus::Archived)
        )
    }

    fn label(&self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Active => "active",
            Self::Archived => "archived",
        }
    }
}

#[derive(Clone, Default)]
enum TypeFilter {
    #[default]
    All,
    Architecture,
    Convention,
    Decision,
    Incident,
    Debugging,
    Environment,
    DomainFact,
}

impl TypeFilter {
    fn next(&self) -> Self {
        match self {
            Self::All => Self::Architecture,
            Self::Architecture => Self::Convention,
            Self::Convention => Self::Decision,
            Self::Decision => Self::Incident,
            Self::Incident => Self::Debugging,
            Self::Debugging => Self::Environment,
            Self::Environment => Self::DomainFact,
            Self::DomainFact => Self::All,
        }
    }

    fn matches(&self, memory_type: &MemoryType) -> bool {
        matches!(
            (self, memory_type),
            (Self::All, _)
                | (Self::Architecture, MemoryType::Architecture)
                | (Self::Convention, MemoryType::Convention)
                | (Self::Decision, MemoryType::Decision)
                | (Self::Incident, MemoryType::Incident)
                | (Self::Debugging, MemoryType::Debugging)
                | (Self::Environment, MemoryType::Environment)
                | (Self::DomainFact, MemoryType::DomainFact)
        )
    }

    fn label(&self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Architecture => "architecture",
            Self::Convention => "convention",
            Self::Decision => "decision",
            Self::Incident => "incident",
            Self::Debugging => "debugging",
            Self::Environment => "environment",
            Self::DomainFact => "domain_fact",
        }
    }
}

fn draw(frame: &mut ratatui::Frame<'_>, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(3),
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

    let filter_bar = Paragraph::new(vec![Line::from(format!(
        "search=/ {}  tag=g {}  status=s {}  type=t {}  clear=x  curate=c  reindex=i  archive=a",
        display_filter(&app.filters.text),
        display_filter(&app.filters.tag),
        app.filters.status.label(),
        app.filters.memory_type.label(),
    ))])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(match &app.input_mode {
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
            }),
    );
    frame.render_widget(filter_bar, chunks[1]);

    match app.active_tab {
        TabKind::Memories => draw_memories_tab(frame, app, chunks[2]),
        TabKind::Project => draw_project_tab(frame, app, chunks[2]),
    }

    let footer = Paragraph::new(app.status_message.clone())
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::ALL).title("Status"));
    frame.render_widget(footer, chunks[3]);
}

fn draw_memories_tab(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let header = Row::new(["Summary", "Type", "Status", "Conf", "Updated"]).style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
    let rows = app.filtered_memories.iter().map(memory_row);
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(34),
            Constraint::Length(14),
            Constraint::Length(10),
            Constraint::Length(6),
            Constraint::Length(20),
        ],
    )
    .header(header)
    .row_highlight_style(Style::default().bg(Color::Blue))
    .block(Block::default().borders(Borders::ALL).title(format!(
        "Memories (showing {} / {})",
        app.filtered_memories.len(),
        app.total_memories
    )));
    let mut state = app.table_state.clone();
    frame.render_stateful_widget(table, chunks[0], &mut state);

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
            Line::from(vec![
                Span::styled("Updated: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(detail.updated_at.format("%Y-%m-%d %H:%M UTC").to_string()),
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
    } else if app.filtered_memories.is_empty() {
        vec![Line::from(format!(
            "No memories match the current filters for project {}.",
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
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Min(8),
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
            "Confidence bins: {} high / {} medium / {} low",
            app.overview.high_confidence_memories,
            app.overview.medium_confidence_memories,
            app.overview.low_confidence_memories
        )),
        Line::from(format!(
            "Recent 7d: {} memories / {} captures",
            app.overview.recent_memories_7d, app.overview.recent_captures_7d
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
            "Last memory: {}   Last curation: {}",
            format_timestamp(app.overview.last_memory_at),
            format_timestamp(app.overview.last_curation_at)
        )),
        Line::from(format!(
            "Last capture: {}   Oldest uncurated age: {}",
            format_timestamp(app.overview.last_capture_at),
            app.overview
                .oldest_uncurated_capture_age_hours
                .map(|hours| format!("{hours}h"))
                .unwrap_or_else(|| "n/a".to_string())
        )),
        Line::from(format!(
            "Automation: {}",
            app.overview
                .automation
                .as_ref()
                .map(format_automation_status)
                .unwrap_or_else(|| "not configured".to_string())
        )),
    ])
    .block(Block::default().borders(Borders::ALL).title("Overview"));
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
            app.overview
                .memory_type_breakdown
                .iter()
                .map(|item| (item.memory_type.to_string(), item.count))
                .collect(),
            "No memory entries yet.",
        ))
        .block(Block::default().borders(Borders::ALL).title("Memory Types")),
        mid[0],
    );
    frame.render_widget(
        Paragraph::new(lines_for_named_counts(
            app.overview
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
        .block(Block::default().borders(Borders::ALL).title("Source Kinds")),
        mid[1],
    );
    frame.render_widget(
        Paragraph::new(lines_for_named_counts(
            app.overview
                .top_tags
                .iter()
                .map(|item| (item.name.clone(), item.count))
                .collect(),
            "No tags yet.",
        ))
        .block(Block::default().borders(Borders::ALL).title("Top Tags")),
        mid[2],
    );

    let bottom = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[2]);
    frame.render_widget(
        Paragraph::new(lines_for_named_counts(
            app.overview
                .top_files
                .iter()
                .map(|item| (item.name.clone(), item.count))
                .collect(),
            "No file provenance yet.",
        ))
        .block(Block::default().borders(Borders::ALL).title("Top Files")),
        bottom[0],
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from("Actions"),
            Line::from("c curate project"),
            Line::from("i reindex search chunks"),
            Line::from("a archive low-value memories"),
            Line::from("r refresh"),
        ])
        .block(Block::default().borders(Borders::ALL).title("Operations")),
        bottom[1],
    );
}

fn lines_for_named_counts(items: Vec<(String, i64)>, empty: &str) -> Vec<Line<'static>> {
    if items.is_empty() {
        vec![Line::from(empty.to_string())]
    } else {
        items
            .into_iter()
            .map(|(name, count)| Line::from(format!("{name}: {count}")))
            .collect()
    }
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

fn display_filter(value: &str) -> String {
    if value.is_empty() {
        "none".to_string()
    } else {
        value.to_string()
    }
}

fn format_automation_status(status: &mem_api::AutomationStatus) -> String {
    format!(
        "enabled={} mode={} dirty_files={} last_decision={}",
        status.enabled,
        match status.mode {
            mem_api::AutomationMode::Suggest => "suggest",
            mem_api::AutomationMode::Auto => "auto",
        },
        status.dirty_file_count.unwrap_or(0),
        status
            .last_decision
            .clone()
            .unwrap_or_else(|| "none".to_string())
    )
}

fn should_quit(key: KeyEvent, app: &App) -> bool {
    matches!(app.input_mode, InputMode::Normal) && matches!(key.code, KeyCode::Char('q'))
}

fn empty_overview(project: String) -> ProjectOverviewResponse {
    ProjectOverviewResponse {
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
        recent_memories_7d: 0,
        recent_captures_7d: 0,
        high_confidence_memories: 0,
        medium_confidence_memories: 0,
        low_confidence_memories: 0,
        last_memory_at: None,
        last_capture_at: None,
        last_curation_at: None,
        oldest_uncurated_capture_age_hours: None,
        memory_type_breakdown: Vec::new(),
        source_kind_breakdown: Vec::new(),
        top_tags: Vec::<NamedCount>::new(),
        top_files: Vec::<NamedCount>::new(),
        automation: None,
    }
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
