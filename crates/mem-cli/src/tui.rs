use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use chrono::{DateTime, Local, Utc};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use mem_api::{
    ActivityDetails, ActivityEvent, ActivityKind, MemoryEntryResponse, MemoryStatus, MemoryType,
    NamedCount, PlanActivityAction, ProjectMemoriesResponse, ProjectMemoryListItem,
    ProjectOverviewResponse, QueryFilters, QueryMatchKind, QueryRequest, QueryResponse,
    QueryResult, ReplacementPolicy, ReplacementProposalRecord, ResumeCheckpoint, ResumeRequest,
    ResumeResponse, StreamRequest, StreamResponse, WatcherHealth, load_repo_replacement_policy,
    read_capnp_text_frame, repo_agent_settings_path, write_capnp_text_frame,
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState, Tabs, Wrap},
};
use tokio::{
    net::{TcpStream, UnixStream},
    sync::mpsc,
};

use crate::{ApiClient, SourceKindString, resume};

struct Theme;

impl Theme {
    const BACKGROUND: Color = Color::Rgb(12, 18, 28);
    const PANEL: Color = Color::Rgb(22, 31, 46);
    const PANEL_ALT: Color = Color::Rgb(28, 39, 58);
    const BORDER: Color = Color::Rgb(74, 94, 122);
    const TITLE: Color = Color::Rgb(146, 195, 255);
    const TEXT: Color = Color::Rgb(230, 236, 245);
    const MUTED: Color = Color::Rgb(150, 165, 186);
    const ACCENT: Color = Color::Rgb(92, 194, 255);
    const ACCENT_STRONG: Color = Color::Rgb(255, 196, 85);
    const SUCCESS: Color = Color::Rgb(104, 211, 145);
    const WARNING: Color = Color::Rgb(255, 187, 92);
    const DANGER: Color = Color::Rgb(255, 122, 122);
    const SELECTION_BG: Color = Color::Rgb(61, 96, 153);
    const SELECTION_FG: Color = Color::Rgb(250, 251, 255);
}

pub(crate) async fn run(api: ApiClient, project: String, repo_root: PathBuf) -> Result<()> {
    let (background_tx, mut background_rx) = mpsc::unbounded_channel();
    let mut terminal = setup_terminal()?;
    let mut app = App::new(project, repo_root, detect_tool_versions(), background_tx);
    terminal.draw(|frame| draw(frame, &app))?;
    app.refresh(&api, RefreshMode::Startup).await;
    let mut stream = StreamSession::connect(&api).await.ok();
    let mut last_stream_connect_attempt = Instant::now();
    if let Some(stream_session) = stream.as_mut() {
        subscribe_stream(stream_session, &app).await?;
        app.status_message =
            "Streaming updates enabled. Press r to force resync, q to exit.".to_string();
    }

    loop {
        let mut stream_failed = false;
        if let Some(current_stream) = stream.as_mut() {
            match current_stream.try_recv() {
                Ok(Some(response)) => {
                    app.apply_stream_response(response);
                    while let Ok(Some(response)) = current_stream.try_recv() {
                        app.apply_stream_response(response);
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    app.status_message =
                        format!("Streaming disconnected: {error}. Falling back to manual refresh.");
                    app.mark_service_unavailable();
                    app.ui_status = UiStatus::Error;
                    stream_failed = true;
                }
            }
        }
        if stream_failed {
            stream = None;
            last_stream_connect_attempt = Instant::now();
        }
        while let Ok(event) = background_rx.try_recv() {
            app.apply_background_event(event);
        }
        if should_attempt_stream_reconnect(stream.is_some(), last_stream_connect_attempt) {
            last_stream_connect_attempt = Instant::now();
            match StreamSession::connect(&api).await {
                Ok(mut stream_session) => match subscribe_stream(&mut stream_session, &app).await {
                    Ok(()) => {
                        stream = Some(stream_session);
                        app.status_message =
                            "Backend reconnected. Refreshing project data...".to_string();
                        app.refresh(&api, RefreshMode::Full).await;
                    }
                    Err(error) => {
                        app.status_message =
                            format!("Backend reachable, but stream subscription failed: {error}");
                        app.ui_status = UiStatus::Error;
                    }
                },
                Err(_) => {}
            }
        }
        terminal.draw(|frame| draw(frame, &app))?;
        if event::poll(Duration::from_millis(200))? {
            match event::read()? {
                Event::Key(key) if should_quit(key, &app) => break,
                Event::Key(key) => {
                    if app.handle_key(key, &api, stream.as_mut()).await? {
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
    repo_root: PathBuf,
    active_tab: TabKind,
    all_memories: Vec<ProjectMemoryListItem>,
    filtered_memories: Vec<ProjectMemoryListItem>,
    total_memories: i64,
    overview: ProjectOverviewResponse,
    selected_detail: Option<MemoryEntryResponse>,
    selected_index: usize,
    table_state: TableState,
    query_text: String,
    query_response: Option<QueryResponse>,
    query_last_duration_ms: Option<u64>,
    query_selected_detail: Option<MemoryEntryResponse>,
    query_selected_index: usize,
    query_table_state: TableState,
    resume_response: Option<ResumeResponse>,
    resume_loading: bool,
    resume_loaded: bool,
    resume_error: Option<String>,
    resume_scroll: u16,
    activity_events: Vec<ActivityEntry>,
    activity_selected_index: usize,
    activity_table_state: TableState,
    memory_detail_scroll: u16,
    project_scroll: u16,
    watcher_scroll: u16,
    replacement_policy: ReplacementPolicy,
    replacement_proposals: Vec<ReplacementProposalRecord>,
    replacement_selected_index: usize,
    versions: ToolVersions,
    ui_status: UiStatus,
    status_message: String,
    health_ok: bool,
    filters: Filters,
    input_mode: InputMode,
    startup_resume_autoselect_pending: bool,
    background_tx: mpsc::UnboundedSender<BackgroundEvent>,
}

struct ToolVersions {
    mem_cli: String,
    mem_service: String,
    memory_watch: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum UiStatus {
    Loading,
    Busy,
    Ready,
    Error,
}

enum ActivityEntry {
    Backend(ActivityEvent),
    Query(QueryActivityEntry),
}

struct QueryActivityEntry {
    recorded_at: DateTime<Utc>,
    project: String,
    request: QueryRequest,
    duration_ms: u64,
    outcome: QueryLogOutcome,
}

#[derive(Clone)]
enum QueryLogOutcome {
    Success(QueryResponse),
    Error(String),
}

enum BackgroundEvent {
    ResumeLoaded {
        response: Result<ResumeResponse, String>,
        checkpoint_present: bool,
        has_changes: bool,
        allow_autoselect: bool,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RefreshMode {
    Startup,
    Full,
}

impl App {
    fn new(
        project: String,
        repo_root: PathBuf,
        versions: ToolVersions,
        background_tx: mpsc::UnboundedSender<BackgroundEvent>,
    ) -> Self {
        let mut table_state = TableState::default();
        table_state.select(Some(0));
        let mut query_table_state = TableState::default();
        query_table_state.select(Some(0));
        let mut activity_table_state = TableState::default();
        activity_table_state.select(Some(0));
        Self {
            project: project.clone(),
            repo_root: repo_root.clone(),
            active_tab: TabKind::Memories,
            all_memories: Vec::new(),
            filtered_memories: Vec::new(),
            total_memories: 0,
            overview: empty_overview(project),
            selected_detail: None,
            selected_index: 0,
            table_state,
            query_text: String::new(),
            query_response: None,
            query_last_duration_ms: None,
            query_selected_detail: None,
            query_selected_index: 0,
            query_table_state,
            resume_response: None,
            resume_loading: false,
            resume_loaded: false,
            resume_error: None,
            resume_scroll: 0,
            activity_events: Vec::new(),
            activity_selected_index: 0,
            activity_table_state,
            memory_detail_scroll: 0,
            project_scroll: 0,
            watcher_scroll: 0,
            replacement_policy: load_repo_replacement_policy(&repo_root).unwrap_or_default(),
            replacement_proposals: Vec::new(),
            replacement_selected_index: 0,
            versions,
            ui_status: UiStatus::Loading,
            status_message: "Loading project data...".to_string(),
            health_ok: false,
            filters: Filters::default(),
            input_mode: InputMode::Normal,
            startup_resume_autoselect_pending: true,
            background_tx,
        }
    }

    async fn refresh(&mut self, api: &ApiClient, mode: RefreshMode) {
        self.status_message = "Refreshing...".to_string();
        self.ui_status = if mode == RefreshMode::Startup {
            UiStatus::Loading
        } else {
            UiStatus::Busy
        };
        self.selected_detail = None;
        self.replacement_policy = load_repo_replacement_policy(&self.repo_root).unwrap_or_default();

        let health_fut = api.health();
        let overview_fut = api.project_overview(&self.project);
        let memories_fut = api.project_memories(&self.project);
        let proposals_fut = api.replacement_proposals(&self.project);
        let (health_result, overview_result, memories_result, proposals_result) =
            tokio::join!(health_fut, overview_fut, memories_fut, proposals_fut);
        let mut had_error = false;

        match health_result {
            Ok(health) => {
                self.health_ok = true;
                if let Some(version) = health.get("version").and_then(|value| value.as_str()) {
                    self.versions.mem_service = version.to_string();
                }
            }
            Err(_) => {
                had_error = true;
                self.mark_service_unavailable();
            }
        }

        match overview_result {
            Ok(overview) => self.overview = overview,
            Err(error) => {
                had_error = true;
                self.status_message = error.to_string();
            }
        }

        match memories_result {
            Ok(ProjectMemoriesResponse {
                project: _,
                total,
                items,
            }) => {
                self.total_memories = total;
                self.all_memories = items;
                self.apply_filters();
                self.fetch_selected_detail(api, None).await;
                self.status_message = format!(
                    "Loaded {} visible memories ({} total).",
                    self.filtered_memories.len(),
                    self.total_memories
                );
            }
            Err(error) => {
                had_error = true;
                self.all_memories.clear();
                self.filtered_memories.clear();
                self.total_memories = 0;
                self.selected_detail = None;
                self.table_state.select(None);
                self.status_message = error.to_string();
            }
        }

        match proposals_result {
            Ok(response) => {
                self.replacement_proposals = response.proposals;
                if self.replacement_proposals.is_empty() {
                    self.replacement_selected_index = 0;
                } else {
                    self.replacement_selected_index = self
                        .replacement_selected_index
                        .min(self.replacement_proposals.len() - 1);
                }
            }
            Err(error) => {
                had_error = true;
                self.replacement_proposals.clear();
                self.replacement_selected_index = 0;
                self.status_message = error.to_string();
            }
        }

        self.ui_status = if had_error {
            UiStatus::Error
        } else if self.resume_loading {
            UiStatus::Busy
        } else {
            UiStatus::Ready
        };

        if mode == RefreshMode::Startup {
            if self.resume_checkpoint().is_some() {
                self.request_resume_refresh(api, true);
            }
        } else if mode == RefreshMode::Full || self.active_tab == TabKind::Resume {
            self.request_resume_refresh(api, false);
        }
    }

    fn resume_checkpoint(&self) -> Option<ResumeCheckpoint> {
        resume::load_checkpoint(&self.project, &self.repo_root)
            .ok()
            .flatten()
    }

    fn mark_service_unavailable(&mut self) {
        self.health_ok = false;
        self.overview.service_status = "error".to_string();
        self.overview.database_status = "unknown".to_string();
        self.overview.watchers = None;
    }

    fn request_resume_refresh(&mut self, api: &ApiClient, allow_autoselect: bool) {
        if self.resume_loading {
            return;
        }
        let checkpoint = self.resume_checkpoint();
        self.resume_loading = true;
        self.resume_error = None;
        if self.resume_response.is_some() {
            self.status_message = "Refreshing resume...".to_string();
        } else {
            self.status_message = "Loading resume...".to_string();
        }
        if !matches!(self.ui_status, UiStatus::Loading) {
            self.ui_status = UiStatus::Busy;
        }
        let request = ResumeRequest {
            project: self.project.clone(),
            checkpoint: checkpoint.clone(),
            since: None,
            include_llm_summary: false,
            limit: 12,
        };
        let api = api.clone();
        let tx = self.background_tx.clone();
        tokio::spawn(async move {
            let response = api
                .resume(&request)
                .await
                .map_err(|error| error.to_string());
            let has_changes = match &response {
                Ok(response) => {
                    !response.timeline.is_empty()
                        || !response.commits.is_empty()
                        || !response.changed_memories.is_empty()
                }
                Err(_) => false,
            };
            let _ = tx.send(BackgroundEvent::ResumeLoaded {
                response,
                checkpoint_present: checkpoint.is_some(),
                has_changes,
                allow_autoselect,
            });
        });
    }

    fn apply_background_event(&mut self, event: BackgroundEvent) {
        match event {
            BackgroundEvent::ResumeLoaded {
                response,
                checkpoint_present,
                has_changes,
                allow_autoselect,
            } => {
                self.resume_loading = false;
                match response {
                    Ok(response) => {
                        self.resume_response = Some(response);
                        self.resume_loaded = true;
                        self.resume_error = None;
                        if allow_autoselect
                            && self.startup_resume_autoselect_pending
                            && checkpoint_present
                            && has_changes
                        {
                            self.active_tab = TabKind::Resume;
                        }
                        self.status_message = if self.active_tab == TabKind::Resume {
                            "Resume loaded.".to_string()
                        } else {
                            "Resume updated in the background.".to_string()
                        };
                        self.ui_status = UiStatus::Ready;
                    }
                    Err(error) => {
                        self.resume_error = Some(error.clone());
                        if self.resume_response.is_none() {
                            self.resume_loaded = false;
                        }
                        self.status_message = format!("Resume unavailable: {error}");
                        self.ui_status = UiStatus::Error;
                    }
                }
            }
        }
    }

    async fn handle_key(
        &mut self,
        key: KeyEvent,
        api: &ApiClient,
        stream: Option<&mut StreamSession>,
    ) -> Result<bool> {
        let current_input = std::mem::take(&mut self.input_mode);
        match current_input {
            InputMode::Normal => {}
            InputMode::Search(mut buffer) => {
                self.handle_text_input(key, api, stream, TextInputKind::Search, &mut buffer)
                    .await?;
                return Ok(false);
            }
            InputMode::Tag(mut buffer) => {
                self.handle_text_input(key, api, stream, TextInputKind::Tag, &mut buffer)
                    .await?;
                return Ok(false);
            }
            InputMode::Query(mut buffer) => {
                self.handle_text_input(key, api, stream, TextInputKind::Query, &mut buffer)
                    .await?;
                return Ok(false);
            }
        }

        self.startup_resume_autoselect_pending = false;

        match key.code {
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') if key.modifiers.is_empty() => {
                self.active_tab = self.active_tab.next();
                if self.active_tab == TabKind::Resume && !self.resume_loaded {
                    self.request_resume_refresh(api, false);
                }
            }
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') if key.modifiers.is_empty() => {
                self.active_tab = self.active_tab.prev();
                if self.active_tab == TabKind::Resume && !self.resume_loaded {
                    self.request_resume_refresh(api, false);
                }
            }
            KeyCode::Char('r') if key.modifiers.is_empty() => {
                self.refresh(api, RefreshMode::Full).await
            }
            KeyCode::Down | KeyCode::Char('j') if self.active_tab == TabKind::Resume => {
                self.scroll_resume(1);
            }
            KeyCode::Up | KeyCode::Char('k') if self.active_tab == TabKind::Resume => {
                self.scroll_resume(-1);
            }
            KeyCode::Down | KeyCode::Char('j') if self.active_tab == TabKind::Memories => {
                self.move_selection(1, api, stream).await;
            }
            KeyCode::Up | KeyCode::Char('k') if self.active_tab == TabKind::Memories => {
                self.move_selection(-1, api, stream).await;
            }
            KeyCode::Down | KeyCode::Char('j') if self.active_tab == TabKind::Query => {
                self.move_query_selection(1, api).await;
            }
            KeyCode::Up | KeyCode::Char('k') if self.active_tab == TabKind::Query => {
                self.move_query_selection(-1, api).await;
            }
            KeyCode::Down | KeyCode::Char('j') if self.active_tab == TabKind::Activity => {
                self.move_activity_selection(1);
            }
            KeyCode::Up | KeyCode::Char('k') if self.active_tab == TabKind::Activity => {
                self.move_activity_selection(-1);
            }
            KeyCode::Down | KeyCode::Char('j') if self.active_tab == TabKind::Project => {
                self.scroll_project(1);
            }
            KeyCode::Up | KeyCode::Char('k') if self.active_tab == TabKind::Project => {
                self.scroll_project(-1);
            }
            KeyCode::Down | KeyCode::Char('j') if self.active_tab == TabKind::Watchers => {
                self.scroll_watchers(1);
            }
            KeyCode::Up | KeyCode::Char('k') if self.active_tab == TabKind::Watchers => {
                self.scroll_watchers(-1);
            }
            KeyCode::PageDown if self.active_tab == TabKind::Memories => {
                self.scroll_memory_detail(8);
            }
            KeyCode::PageUp if self.active_tab == TabKind::Memories => {
                self.scroll_memory_detail(-8);
            }
            KeyCode::Home if self.active_tab == TabKind::Memories => {
                self.memory_detail_scroll = 0;
            }
            KeyCode::PageDown if self.active_tab == TabKind::Resume => {
                self.scroll_resume(8);
            }
            KeyCode::PageUp if self.active_tab == TabKind::Resume => {
                self.scroll_resume(-8);
            }
            KeyCode::Home if self.active_tab == TabKind::Resume => {
                self.resume_scroll = 0;
            }
            KeyCode::PageDown if self.active_tab == TabKind::Project => {
                self.scroll_project(8);
            }
            KeyCode::PageUp if self.active_tab == TabKind::Project => {
                self.scroll_project(-8);
            }
            KeyCode::Home if self.active_tab == TabKind::Project => {
                self.project_scroll = 0;
            }
            KeyCode::Char('p')
                if self.active_tab == TabKind::Project && key.modifiers.is_empty() =>
            {
                self.cycle_replacement_policy().await?;
            }
            KeyCode::Char('[')
                if self.active_tab == TabKind::Project && key.modifiers.is_empty() =>
            {
                self.select_replacement_proposal(-1);
            }
            KeyCode::Char(']')
                if self.active_tab == TabKind::Project && key.modifiers.is_empty() =>
            {
                self.select_replacement_proposal(1);
            }
            KeyCode::Char('y')
                if self.active_tab == TabKind::Project && key.modifiers.is_empty() =>
            {
                self.approve_selected_replacement_proposal(api).await?;
            }
            KeyCode::Char('n')
                if self.active_tab == TabKind::Project && key.modifiers.is_empty() =>
            {
                self.reject_selected_replacement_proposal(api).await?;
            }
            KeyCode::PageDown if self.active_tab == TabKind::Watchers => {
                self.scroll_watchers(8);
            }
            KeyCode::PageUp if self.active_tab == TabKind::Watchers => {
                self.scroll_watchers(-8);
            }
            KeyCode::Home if self.active_tab == TabKind::Watchers => {
                self.watcher_scroll = 0;
            }
            KeyCode::Char('/') if key.modifiers.is_empty() => {
                self.input_mode = InputMode::Search(self.filters.text.clone());
                self.status_message =
                    "Type search text, Enter to apply, Esc to cancel.".to_string();
            }
            KeyCode::Char('?') if key.modifiers.is_empty() => {
                self.active_tab = TabKind::Query;
                self.input_mode = InputMode::Query(self.query_text.clone());
                self.status_message = "Type a question, Enter to run, Esc to cancel.".to_string();
            }
            KeyCode::Char(ch)
                if self.active_tab == TabKind::Query
                    && (key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT) =>
            {
                let mut buffer = self.query_text.clone();
                buffer.push(ch);
                self.input_mode = InputMode::Query(buffer);
                self.status_message = "Type a question, Enter to run, Esc to cancel.".to_string();
            }
            KeyCode::Char('g') if key.modifiers.is_empty() => {
                self.input_mode = InputMode::Tag(self.filters.tag.clone());
                self.status_message =
                    "Type tag filter text, Enter to apply, Esc to cancel.".to_string();
            }
            KeyCode::Char('t') if key.modifiers.is_empty() => {
                self.filters.memory_type = self.filters.memory_type.next();
                self.apply_filters();
                self.fetch_selected_detail(api, stream).await;
            }
            KeyCode::Char('s') if key.modifiers.is_empty() => {
                self.filters.status = self.filters.status.next();
                self.apply_filters();
                self.fetch_selected_detail(api, stream).await;
            }
            KeyCode::Char('x') if key.modifiers.is_empty() => {
                self.filters = Filters::default();
                self.input_mode = InputMode::Normal;
                self.apply_filters();
                self.fetch_selected_detail(api, stream).await;
                self.status_message = "Cleared filters.".to_string();
            }
            KeyCode::Char('c') if key.modifiers.is_empty() => {
                let response = api.curate(&self.project, self.replacement_policy).await?;
                self.status_message = format!(
                    "Curated {} captures into {} memories with {} replacement(s) and {} queued proposal(s).",
                    response.input_count,
                    response.output_count,
                    response.replaced_count,
                    response.proposal_count
                );
                self.refresh(api, RefreshMode::Full).await;
            }
            KeyCode::Char('i') if key.modifiers.is_empty() => {
                let response = api.reindex(&self.project).await?;
                self.status_message =
                    format!("Reindexed {} memory entries.", response.reindexed_entries);
                self.refresh(api, RefreshMode::Full).await;
            }
            KeyCode::Char('e') if key.modifiers.is_empty() => {
                let response = api.reembed(&self.project).await?;
                self.status_message = format!(
                    "Materialized {} chunk embeddings for the active space.",
                    response.reembedded_chunks
                );
                self.refresh(api, RefreshMode::Full).await;
            }
            KeyCode::Char('a') if key.modifiers.is_empty() => {
                let response = api.archive_low_value(&self.project).await?;
                self.status_message = format!(
                    "Archived {} low-value memories using default thresholds.",
                    response.archived_count
                );
                self.refresh(api, RefreshMode::Full).await;
            }
            KeyCode::Char('D') if key.modifiers == KeyModifiers::SHIFT => {
                if self.active_tab == TabKind::Memories {
                    self.delete_selected_memory(api).await?;
                } else if self.active_tab == TabKind::Query {
                    self.delete_selected_query_memory(api).await?;
                }
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
        stream: Option<&mut StreamSession>,
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
                    TextInputKind::Query => self.query_text = buffer.clone(),
                }
                self.input_mode = InputMode::Normal;
                match kind {
                    TextInputKind::Query => {
                        self.run_query(api).await;
                    }
                    _ => {
                        self.apply_filters();
                        self.fetch_selected_detail(api, stream).await;
                        self.status_message = "Applied filter.".to_string();
                    }
                }
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

    async fn move_selection(
        &mut self,
        delta: isize,
        api: &ApiClient,
        stream: Option<&mut StreamSession>,
    ) {
        if self.filtered_memories.is_empty() {
            return;
        }
        let next = (self.selected_index as isize + delta)
            .clamp(0, self.filtered_memories.len().saturating_sub(1) as isize)
            as usize;
        if next != self.selected_index {
            self.selected_index = next;
            self.table_state.select(Some(self.selected_index));
            self.fetch_selected_detail(api, stream).await;
        }
    }

    async fn fetch_selected_detail(
        &mut self,
        api: &ApiClient,
        mut stream: Option<&mut StreamSession>,
    ) {
        self.selected_detail = None;
        self.memory_detail_scroll = 0;
        if let Some(item) = self.filtered_memories.get(self.selected_index) {
            if let Some(stream) = stream.as_mut() {
                if let Err(error) = stream
                    .send(StreamRequest::SubscribeMemory { memory_id: item.id })
                    .await
                {
                    self.status_message = error.to_string();
                }
            } else {
                match api.memory_detail(&item.id.to_string()).await {
                    Ok(detail) => self.selected_detail = Some(detail),
                    Err(error) => self.status_message = error.to_string(),
                }
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

    fn apply_stream_response(&mut self, response: StreamResponse) {
        match response {
            StreamResponse::ProjectSnapshot { overview, memories }
            | StreamResponse::ProjectChanged { overview, memories } => {
                self.overview = overview;
                self.total_memories = memories.total;
                self.all_memories = memories.items;
                self.apply_filters();
                self.resume_loaded = false;
                self.status_message = format!(
                    "Streaming update: {} visible memories ({} total).",
                    self.filtered_memories.len(),
                    self.total_memories
                );
                self.ui_status = UiStatus::Ready;
            }
            StreamResponse::MemorySnapshot { detail }
            | StreamResponse::MemoryChanged { detail } => {
                self.selected_detail = detail;
                self.memory_detail_scroll = 0;
            }
            StreamResponse::Activity { event } => {
                self.record_backend_activity(event);
            }
            StreamResponse::Error { message } => {
                self.status_message = format!("Stream error: {message}");
                self.ui_status = UiStatus::Error;
            }
            _ => {}
        }
    }

    async fn run_query(&mut self, api: &ApiClient) {
        let question = self.query_text.trim();
        if question.is_empty() {
            self.query_response = None;
            self.query_last_duration_ms = None;
            self.query_selected_detail = None;
            self.query_selected_index = 0;
            self.query_table_state.select(None);
            self.status_message = "Enter a query before running search.".to_string();
            return;
        }

        self.status_message = format!("Running query for \"{question}\"...");
        self.ui_status = UiStatus::Busy;
        let request = QueryRequest {
            project: self.project.clone(),
            query: question.to_string(),
            filters: QueryFilters::default(),
            top_k: 8,
            min_confidence: None,
        };
        let started = Instant::now();
        match api.query(&request).await {
            Ok(response) => {
                let elapsed_ms = started.elapsed().as_millis() as u64;
                self.record_query_activity(
                    request.clone(),
                    elapsed_ms,
                    QueryLogOutcome::Success(response.clone()),
                );
                self.resume_loaded = false;
                self.query_last_duration_ms = Some(elapsed_ms);
                self.query_response = Some(response);
                self.query_selected_index = 0;
                if self.query_results().is_empty() {
                    self.query_selected_detail = None;
                    self.query_table_state.select(None);
                } else {
                    self.query_table_state.select(Some(0));
                    self.fetch_selected_query_detail(api).await;
                }
                self.status_message = format!(
                    "Query returned {} memories in {} ms.",
                    self.query_results().len(),
                    elapsed_ms
                );
                self.ui_status = UiStatus::Ready;
            }
            Err(error) => {
                let elapsed_ms = started.elapsed().as_millis() as u64;
                self.record_query_activity(
                    request,
                    elapsed_ms,
                    QueryLogOutcome::Error(error.to_string()),
                );
                self.resume_loaded = false;
                self.query_response = None;
                self.query_last_duration_ms = Some(elapsed_ms);
                self.query_selected_detail = None;
                self.query_table_state.select(None);
                self.status_message = error.to_string();
                self.ui_status = UiStatus::Error;
            }
        }
    }

    async fn move_query_selection(&mut self, delta: isize, api: &ApiClient) {
        if self.query_results().is_empty() {
            return;
        }
        let next = (self.query_selected_index as isize + delta)
            .clamp(0, self.query_results().len().saturating_sub(1) as isize)
            as usize;
        if next != self.query_selected_index {
            self.query_selected_index = next;
            self.query_table_state
                .select(Some(self.query_selected_index));
            self.fetch_selected_query_detail(api).await;
        }
    }

    async fn fetch_selected_query_detail(&mut self, api: &ApiClient) {
        self.query_selected_detail = None;
        if let Some(result) = self.query_results().get(self.query_selected_index) {
            match api.memory_detail(&result.memory_id.to_string()).await {
                Ok(detail) => self.query_selected_detail = Some(detail),
                Err(error) => self.status_message = error.to_string(),
            }
        }
    }

    fn query_results(&self) -> &[QueryResult] {
        self.query_response
            .as_ref()
            .map(|response| response.results.as_slice())
            .unwrap_or(&[])
    }

    fn record_query_activity(
        &mut self,
        request: QueryRequest,
        duration_ms: u64,
        outcome: QueryLogOutcome,
    ) {
        self.activity_events.insert(
            0,
            ActivityEntry::Query(QueryActivityEntry {
                recorded_at: Utc::now(),
                project: request.project.clone(),
                request,
                duration_ms,
                outcome,
            }),
        );
        self.finish_activity_insert();
    }

    fn record_backend_activity(&mut self, event: ActivityEvent) {
        if let Some(ActivityDetails::WatcherHealth {
            health,
            previous_health,
            message,
            ..
        }) = event.details.as_ref()
        {
            self.status_message = watcher_transition_status_message(
                &event.summary,
                health,
                previous_health.as_ref(),
                message.as_deref(),
            );
        }
        self.activity_events
            .insert(0, ActivityEntry::Backend(event));
        self.finish_activity_insert();
    }

    fn finish_activity_insert(&mut self) {
        if self.activity_events.len() > 200 {
            self.activity_events.truncate(200);
        }
        self.activity_selected_index = 0;
        self.activity_table_state.select(Some(0));
    }

    fn move_activity_selection(&mut self, delta: isize) {
        if self.activity_events.is_empty() {
            return;
        }
        let next = (self.activity_selected_index as isize + delta)
            .clamp(0, self.activity_events.len().saturating_sub(1) as isize)
            as usize;
        if next != self.activity_selected_index {
            self.activity_selected_index = next;
            self.activity_table_state
                .select(Some(self.activity_selected_index));
        }
    }

    fn select_replacement_proposal(&mut self, delta: isize) {
        if self.replacement_proposals.is_empty() {
            return;
        }
        let next = (self.replacement_selected_index as isize + delta).clamp(
            0,
            self.replacement_proposals.len().saturating_sub(1) as isize,
        ) as usize;
        self.replacement_selected_index = next;
    }

    async fn cycle_replacement_policy(&mut self) -> Result<()> {
        self.replacement_policy = match self.replacement_policy {
            ReplacementPolicy::Conservative => ReplacementPolicy::Balanced,
            ReplacementPolicy::Balanced => ReplacementPolicy::Aggressive,
            ReplacementPolicy::Aggressive => ReplacementPolicy::Conservative,
        };
        write_replacement_policy(&self.repo_root, self.replacement_policy)?;
        self.status_message = format!(
            "Curation replacement policy set to {}.",
            self.replacement_policy
        );
        Ok(())
    }

    async fn approve_selected_replacement_proposal(&mut self, api: &ApiClient) -> Result<()> {
        let Some(proposal) = self
            .replacement_proposals
            .get(self.replacement_selected_index)
            .cloned()
        else {
            self.status_message = "No pending replacement proposal selected.".to_string();
            return Ok(());
        };
        let response = api
            .approve_replacement_proposal(&self.project, proposal.id)
            .await?;
        self.status_message = format!(
            "Approved replacement: {} -> {}",
            response.target_summary, response.candidate_summary
        );
        self.refresh(api, RefreshMode::Full).await;
        Ok(())
    }

    async fn reject_selected_replacement_proposal(&mut self, api: &ApiClient) -> Result<()> {
        let Some(proposal) = self
            .replacement_proposals
            .get(self.replacement_selected_index)
            .cloned()
        else {
            self.status_message = "No pending replacement proposal selected.".to_string();
            return Ok(());
        };
        let response = api
            .reject_replacement_proposal(&self.project, proposal.id)
            .await?;
        self.status_message = format!(
            "Rejected replacement proposal for {}.",
            response.target_summary
        );
        self.refresh(api, RefreshMode::Full).await;
        Ok(())
    }

    async fn delete_selected_memory(&mut self, api: &ApiClient) -> Result<()> {
        let Some(item) = self.filtered_memories.get(self.selected_index) else {
            self.status_message = "No selected memory to delete.".to_string();
            return Ok(());
        };
        let response = api.delete_memory(item.id).await?;
        self.status_message = format!("Deleted memory: {}", response.summary);
        self.refresh(api, RefreshMode::Full).await;
        Ok(())
    }

    async fn delete_selected_query_memory(&mut self, api: &ApiClient) -> Result<()> {
        let Some(result) = self.query_results().get(self.query_selected_index) else {
            self.status_message = "No selected query result to delete.".to_string();
            return Ok(());
        };
        let response = api.delete_memory(result.memory_id).await?;
        self.status_message = format!("Deleted memory: {}", response.summary);
        self.query_selected_detail = None;
        self.run_query(api).await;
        self.refresh(api, RefreshMode::Full).await;
        Ok(())
    }

    fn scroll_project(&mut self, delta: i16) {
        self.project_scroll = if delta.is_negative() {
            self.project_scroll.saturating_sub(delta.unsigned_abs())
        } else {
            self.project_scroll
                .saturating_add(u16::try_from(delta).unwrap_or(0))
        };
    }

    fn scroll_resume(&mut self, delta: i16) {
        self.resume_scroll = if delta.is_negative() {
            self.resume_scroll.saturating_sub(delta.unsigned_abs())
        } else {
            self.resume_scroll
                .saturating_add(u16::try_from(delta).unwrap_or(0))
        };
    }

    fn scroll_watchers(&mut self, delta: i16) {
        self.watcher_scroll = if delta.is_negative() {
            self.watcher_scroll.saturating_sub(delta.unsigned_abs())
        } else {
            self.watcher_scroll
                .saturating_add(u16::try_from(delta).unwrap_or(0))
        };
    }

    fn scroll_memory_detail(&mut self, delta: i16) {
        self.memory_detail_scroll = if delta.is_negative() {
            self.memory_detail_scroll
                .saturating_sub(delta.unsigned_abs())
        } else {
            self.memory_detail_scroll
                .saturating_add(u16::try_from(delta).unwrap_or(0))
        };
    }
}

struct StreamSession {
    writer: tokio::io::WriteHalf<StreamTransport>,
    rx: mpsc::UnboundedReceiver<StreamResponse>,
}

enum StreamTransport {
    Unix(UnixStream),
    Tcp(TcpStream),
}

impl tokio::io::AsyncRead for StreamTransport {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            StreamTransport::Unix(stream) => std::pin::Pin::new(stream).poll_read(cx, buf),
            StreamTransport::Tcp(stream) => std::pin::Pin::new(stream).poll_read(cx, buf),
        }
    }
}

impl tokio::io::AsyncWrite for StreamTransport {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match self.get_mut() {
            StreamTransport::Unix(stream) => std::pin::Pin::new(stream).poll_write(cx, buf),
            StreamTransport::Tcp(stream) => std::pin::Pin::new(stream).poll_write(cx, buf),
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            StreamTransport::Unix(stream) => std::pin::Pin::new(stream).poll_flush(cx),
            StreamTransport::Tcp(stream) => std::pin::Pin::new(stream).poll_flush(cx),
        }
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            StreamTransport::Unix(stream) => std::pin::Pin::new(stream).poll_shutdown(cx),
            StreamTransport::Tcp(stream) => std::pin::Pin::new(stream).poll_shutdown(cx),
        }
    }
}

impl StreamSession {
    async fn connect(api: &ApiClient) -> Result<Self> {
        let transport = if std::path::Path::new(&api.config.service.capnp_unix_socket).exists() {
            match UnixStream::connect(&api.config.service.capnp_unix_socket).await {
                Ok(stream) => StreamTransport::Unix(stream),
                Err(_) => StreamTransport::Tcp(
                    TcpStream::connect(&api.config.service.capnp_tcp_addr).await?,
                ),
            }
        } else {
            StreamTransport::Tcp(TcpStream::connect(&api.config.service.capnp_tcp_addr).await?)
        };
        let (mut reader, writer) = tokio::io::split(transport);
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            loop {
                match read_capnp_text_frame(&mut reader).await {
                    Ok(Some(text)) => {
                        if let Ok(response) = serde_json::from_str::<StreamResponse>(&text) {
                            let _ = tx.send(response);
                        }
                    }
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
        });
        Ok(Self { writer, rx })
    }

    async fn send(&mut self, request: StreamRequest) -> Result<()> {
        let text = serde_json::to_string(&request)?;
        write_capnp_text_frame(&mut self.writer, &text).await?;
        Ok(())
    }

    fn try_recv(&mut self) -> Result<Option<StreamResponse>> {
        match self.rx.try_recv() {
            Ok(response) => Ok(Some(response)),
            Err(mpsc::error::TryRecvError::Empty) => Ok(None),
            Err(mpsc::error::TryRecvError::Disconnected) => {
                anyhow::bail!("stream connection closed")
            }
        }
    }
}

async fn subscribe_stream(stream: &mut StreamSession, app: &App) -> Result<()> {
    stream
        .send(StreamRequest::SubscribeProject {
            project: app.project.clone(),
        })
        .await?;
    if let Some(item) = app.filtered_memories.get(app.selected_index) {
        stream
            .send(StreamRequest::SubscribeMemory { memory_id: item.id })
            .await?;
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TabKind {
    Resume,
    Memories,
    Query,
    Activity,
    Project,
    Watchers,
}

impl TabKind {
    fn next(self) -> Self {
        match self {
            Self::Resume => Self::Memories,
            Self::Memories => Self::Query,
            Self::Query => Self::Activity,
            Self::Activity => Self::Project,
            Self::Project => Self::Watchers,
            Self::Watchers => Self::Resume,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Resume => Self::Watchers,
            Self::Memories => Self::Resume,
            Self::Query => Self::Memories,
            Self::Activity => Self::Query,
            Self::Project => Self::Activity,
            Self::Watchers => Self::Project,
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Resume => 0,
            Self::Memories => 1,
            Self::Query => 2,
            Self::Activity => 3,
            Self::Project => 4,
            Self::Watchers => 5,
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
    Query(String),
}

#[derive(Clone, Copy)]
enum TextInputKind {
    Search,
    Tag,
    Query,
}

impl TextInputKind {
    fn wrap(self, value: String) -> InputMode {
        match self {
            Self::Search => InputMode::Search(value),
            Self::Tag => InputMode::Tag(value),
            Self::Query => InputMode::Query(value),
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
    Plan,
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
            Self::DomainFact => Self::Plan,
            Self::Plan => Self::All,
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
                | (Self::Plan, MemoryType::Plan)
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
            Self::Plan => "plan",
        }
    }
}

fn draw(frame: &mut ratatui::Frame<'_>, app: &App) {
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
            Constraint::Length(4),
        ])
        .split(frame.area());

    let titles = [
        "Resume", "Memories", "Query", "Activity", "Project", "Watchers",
    ]
    .into_iter()
    .map(|title| Line::from(Span::styled(title, Style::default().fg(Theme::TEXT))))
    .collect::<Vec<_>>();
    let tabs = Tabs::new(titles)
        .select(app.active_tab.index())
        .block(
            themed_block(format!("Memory Layer TUI - project {}", app.project))
                .borders(Borders::ALL),
        )
        .style(Style::default().fg(Theme::MUTED).bg(Theme::PANEL))
        .highlight_style(
            Style::default()
                .fg(Theme::SELECTION_FG)
                .bg(Theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_widget(tabs, chunks[0]);

    let filter_bar = Paragraph::new(vec![Line::from(match app.active_tab {
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
            Span::styled(
                "detail=PgUp/PgDn Home  clear=x curate=c reindex=i reembed=e archive=a delete=D",
                Style::default().fg(Theme::MUTED),
            ),
        ],
        TabKind::Query => vec![
            accent_span("query=? "),
            Span::styled(
                display_filter(&current_query_display(app)),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("  "),
            Span::styled("j/k move result", Style::default().fg(Theme::MUTED)),
            Span::raw("  "),
            Span::styled(
                "Enter runs while editing",
                Style::default().fg(Theme::MUTED),
            ),
        ],
        TabKind::Activity => vec![
            accent_span("activity "),
            Span::styled("j/k move  ", Style::default().fg(Theme::TEXT)),
            Span::styled(
                "shows queries and backend capture/curate/reindex/reembed/archive/delete events",
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
        TabKind::Watchers => vec![
            accent_span("scroll "),
            Span::styled("j/k  ", Style::default().fg(Theme::TEXT)),
            accent_span("page "),
            Span::styled("PgUp/PgDn  ", Style::default().fg(Theme::TEXT)),
            accent_span("jump "),
            Span::styled("Home", Style::default().fg(Theme::TEXT)),
        ],
    })])
    .style(Style::default().bg(Theme::PANEL_ALT))
    .block(themed_block(match &app.input_mode {
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
    }));
    frame.render_widget(filter_bar, chunks[1]);

    match app.active_tab {
        TabKind::Resume => draw_resume_tab(frame, app, chunks[2]),
        TabKind::Memories => draw_memories_tab(frame, app, chunks[2]),
        TabKind::Query => draw_query_tab(frame, app, chunks[2]),
        TabKind::Activity => draw_activity_tab(frame, app, chunks[2]),
        TabKind::Project => draw_project_tab(frame, app, chunks[2]),
        TabKind::Watchers => draw_watchers_tab(frame, app, chunks[2]),
    }

    let footer_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Length(1)])
        .split(chunks[3]);

    let footer = Paragraph::new(app.status_message.clone())
        .style(status_message_style(app))
        .wrap(Wrap { trim: false })
        .block(themed_block("Status"));
    frame.render_widget(footer, footer_chunks[0]);
    draw_bottom_status_bar(frame, app, footer_chunks[1]);
}

fn draw_bottom_status_bar(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    frame.render_widget(
        Block::default().style(Style::default().bg(Theme::PANEL_ALT)),
        area,
    );

    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(34),
            Constraint::Percentage(33),
        ])
        .split(area);

    frame.render_widget(
        Paragraph::new(component_status_line(
            "TUI",
            &app.versions.mem_cli,
            tui_status_label(app),
            tui_status_color(app),
            None,
        ))
        .style(Style::default().bg(Theme::PANEL_ALT)),
        sections[0],
    );
    frame.render_widget(
        Paragraph::new(component_status_line(
            "Service",
            &app.versions.mem_service,
            service_status_label(app),
            service_status_color(app),
            service_status_detail(app),
        ))
        .style(Style::default().bg(Theme::PANEL_ALT)),
        sections[1],
    );
    frame.render_widget(
        Paragraph::new(component_status_line(
            "Watchers",
            &app.versions.memory_watch,
            watcher_bar_status_label(app),
            watcher_bar_status_color(app),
            watcher_bar_status_detail(app),
        ))
        .style(Style::default().bg(Theme::PANEL_ALT)),
        sections[2],
    );
}

fn component_status_line<'a>(
    label: &'a str,
    version: &'a str,
    status: &'a str,
    status_color: Color,
    detail: Option<String>,
) -> Line<'a> {
    let mut spans = vec![
        Span::styled(
            format!("{label} "),
            Style::default()
                .fg(Theme::ACCENT_STRONG)
                .bg(Theme::PANEL_ALT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("v{version} "),
            Style::default().fg(Theme::TEXT).bg(Theme::PANEL_ALT),
        ),
        Span::styled(
            status.to_string(),
            Style::default()
                .fg(status_color)
                .bg(Theme::PANEL_ALT)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    if let Some(detail) = detail {
        spans.push(Span::styled(
            format!(" {detail}"),
            Style::default().fg(Theme::MUTED).bg(Theme::PANEL_ALT),
        ));
    }
    Line::from(spans)
}

fn draw_memories_tab(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    let header = Row::new(["Summary", "Type", "Status", "Conf", "Updated"]).style(
        Style::default()
            .fg(Theme::ACCENT_STRONG)
            .bg(Theme::PANEL_ALT)
            .add_modifier(Modifier::BOLD),
    );
    let rows = app.filtered_memories.iter().map(memory_row);
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(34),
            Constraint::Length(12),
            Constraint::Length(8),
            Constraint::Length(5),
            Constraint::Length(20),
        ],
    )
    .column_spacing(2)
    .header(header)
    .row_highlight_style(
        Style::default()
            .fg(Theme::SELECTION_FG)
            .bg(Theme::SELECTION_BG)
            .add_modifier(Modifier::BOLD),
    )
    .block(themed_block(format!(
        "Memories (showing {} / {})",
        app.filtered_memories.len(),
        app.total_memories
    )));
    let mut state = app.table_state.clone();
    frame.render_stateful_widget(table, chunks[0], &mut state);

    let detail_text = if let Some(detail) = &app.selected_detail {
        let mut lines = vec![
            Line::from(vec![
                label_span("Summary: "),
                Span::styled(detail.summary.clone(), Style::default().fg(Theme::TEXT)),
            ]),
            Line::from(vec![
                label_span("Type: "),
                memory_type_span(&detail.memory_type),
                Span::raw("   "),
                label_span("Status: "),
                status_span(match detail.status {
                    MemoryStatus::Active => "active",
                    MemoryStatus::Archived => "archived",
                }),
            ]),
            Line::from(vec![
                label_span("Confidence: "),
                Span::styled(
                    format!("{:.2}", detail.confidence),
                    confidence_style(detail.confidence),
                ),
                Span::raw("   "),
                label_span("Importance: "),
                Span::styled(
                    detail.importance.to_string(),
                    Style::default().fg(Theme::TEXT),
                ),
            ]),
            Line::from(vec![
                label_span("Updated: "),
                Span::styled(
                    format_timestamp_medium(detail.updated_at),
                    Style::default().fg(Theme::TEXT),
                ),
            ]),
            Line::from(""),
            Line::from(vec![section_span("Canonical Text")]),
            Line::from(Span::styled(
                detail.canonical_text.clone(),
                Style::default().fg(Theme::TEXT),
            )),
            Line::from(""),
            Line::from(vec![
                label_span("Tags: "),
                Span::styled(
                    if detail.tags.is_empty() {
                        "none".to_string()
                    } else {
                        detail.tags.join(", ")
                    },
                    Style::default().fg(Theme::TEXT),
                ),
            ]),
            Line::from(""),
            Line::from(vec![section_span("Sources")]),
        ];

        if detail.sources.is_empty() {
            lines.push(Line::from(Span::styled(
                "No provenance sources recorded.",
                Style::default().fg(Theme::MUTED),
            )));
        } else {
            for source in &detail.sources {
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
        lines
    } else if app.filtered_memories.is_empty() {
        vec![Line::from(Span::styled(
            format!(
                "No memories match the current filters for project {}.",
                app.project
            ),
            Style::default().fg(Theme::MUTED),
        ))]
    } else {
        vec![Line::from(Span::styled(
            "Select a memory to load its details.",
            Style::default().fg(Theme::MUTED),
        ))]
    };

    let detail = Paragraph::new(detail_text)
        .style(Style::default().bg(Theme::PANEL))
        .scroll((app.memory_detail_scroll, 0))
        .wrap(Wrap { trim: false })
        .block(themed_block("Detail"));
    frame.render_widget(detail, chunks[1]);
}

fn draw_project_tab(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
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
            Span::styled(&app.overview.project, Style::default().fg(Theme::TEXT)),
        ),
        Line::from(vec![
            label_span("Service: "),
            service_span(&app.overview.service_status),
            Span::raw("   "),
            label_span("Database: "),
            service_span(&app.overview.database_status),
        ]),
        Line::from(vec![
            label_span("Memories: "),
            Span::styled(
                format!(
                    "{} total / {} active / {} archived",
                    app.overview.memory_entries_total,
                    app.overview.active_memories,
                    app.overview.archived_memories
                ),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
        Line::from(vec![
            label_span("Confidence bins: "),
            Span::styled(
                format!(
                    "{} high / {} medium / {} low",
                    app.overview.high_confidence_memories,
                    app.overview.medium_confidence_memories,
                    app.overview.low_confidence_memories
                ),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
        metric_line(
            "Recent 7d",
            Span::styled(
                format!(
                    "{} memories / {} captures",
                    app.overview.recent_memories_7d, app.overview.recent_captures_7d
                ),
                Style::default().fg(Theme::TEXT),
            ),
        ),
        metric_line(
            "Raw captures",
            Span::styled(
                format!(
                    "{} total / {} uncurated",
                    app.overview.raw_captures_total, app.overview.uncurated_raw_captures
                ),
                Style::default().fg(Theme::TEXT),
            ),
        ),
        metric_line(
            "Tasks / Sessions / Runs",
            Span::styled(
                format!(
                    "{} / {} / {}",
                    app.overview.tasks_total,
                    app.overview.sessions_total,
                    app.overview.curation_runs_total
                ),
                Style::default().fg(Theme::TEXT),
            ),
        ),
        metric_line(
            "Last memory / curation",
            Span::styled(
                format!(
                    "{} / {}",
                    format_timestamp(app.overview.last_memory_at),
                    format_timestamp(app.overview.last_curation_at)
                ),
                Style::default().fg(Theme::TEXT),
            ),
        ),
        metric_line(
            "Last capture / oldest uncurated",
            Span::styled(
                format!(
                    "{} / {}",
                    format_timestamp(app.overview.last_capture_at),
                    app.overview
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
                    app.versions.mem_cli, app.versions.mem_service, app.versions.memory_watch
                ),
                Style::default().fg(Theme::TEXT),
            ),
        ),
        metric_line(
            "Automation",
            Span::styled(
                app.overview
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
                    "{} / {} pending proposal(s)",
                    app.replacement_policy, app.overview.pending_replacement_proposals
                ),
                Style::default().fg(Theme::TEXT),
            ),
        ),
    ])
    .scroll((app.project_scroll, 0))
    .style(Style::default().bg(Theme::PANEL))
    .block(themed_block(format!(
        "Overview (scroll {})",
        app.project_scroll
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
            app.overview
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
        .style(Style::default().bg(Theme::PANEL_ALT))
        .block(themed_block("Source Kinds")),
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
        .style(Style::default().bg(Theme::PANEL_ALT))
        .block(themed_block("Top Tags")),
        mid[2],
    );

    let bottom = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),
            Constraint::Min(8),
            Constraint::Length(7),
        ])
        .split(chunks[2]);

    frame.render_widget(
        Paragraph::new(replacement_proposal_lines(app))
            .style(Style::default().bg(Theme::PANEL_ALT))
            .wrap(Wrap { trim: false })
            .block(themed_block("Curation Review")),
        bottom[0],
    );
    frame.render_widget(
        Paragraph::new(lines_for_named_counts(
            app.overview
                .top_files
                .iter()
                .map(|item| (item.name.clone(), item.count))
                .collect(),
            "No file provenance yet.",
        ))
        .style(Style::default().bg(Theme::PANEL_ALT))
        .block(themed_block("Top Files")),
        bottom[1],
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
                "p cycle policy / [ ] select proposal",
                Style::default().fg(Theme::TEXT),
            )),
            Line::from(Span::styled(
                "y approve / n reject selected proposal",
                Style::default().fg(Theme::TEXT),
            )),
            Line::from(Span::styled("r refresh", Style::default().fg(Theme::TEXT))),
        ])
        .style(Style::default().bg(Theme::PANEL_ALT))
        .block(themed_block("Operations")),
        bottom[2],
    );
}

fn draw_watchers_tab(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(10)])
        .split(area);

    let summary = Paragraph::new(vec![
        metric_line(
            "Watchers",
            Span::styled(watcher_summary_text(app), Style::default().fg(Theme::TEXT)),
        ),
        metric_line(
            "Guidance",
            Span::styled(
                "Use `memory watcher enable --project <slug>` or `memory watcher run --project <slug>`.",
                Style::default().fg(Theme::MUTED),
            ),
        ),
    ])
    .style(Style::default().bg(Theme::PANEL))
    .block(themed_block("Watcher Summary"));
    frame.render_widget(summary, chunks[0]);

    let detail = Paragraph::new(watcher_detail_lines(app))
        .scroll((app.watcher_scroll, 0))
        .wrap(Wrap { trim: false })
        .style(Style::default().bg(Theme::PANEL_ALT))
        .block(themed_block(format!(
            "Watchers (scroll {})",
            app.watcher_scroll
        )));
    frame.render_widget(detail, chunks[1]);
}

fn draw_query_tab(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(11), Constraint::Min(12)])
        .split(area);

    let answer_text = if let Some(response) = &app.query_response {
        vec![
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
            Line::from(vec![
                label_span("Roundtrip: "),
                Span::styled(
                    app.query_last_duration_ms
                        .map(|value| format!("{value} ms"))
                        .unwrap_or_else(|| "n/a".to_string()),
                    Style::default().fg(Theme::TEXT),
                ),
                Span::raw("   "),
                label_span("Server: "),
                Span::styled(
                    format!("{} ms", response.diagnostics.total_duration_ms),
                    Style::default().fg(Theme::TEXT),
                ),
                Span::raw("   "),
                label_span("Merged: "),
                Span::styled(
                    response.diagnostics.merged_candidates.to_string(),
                    Style::default().fg(Theme::TEXT),
                ),
            ]),
            Line::from(vec![
                label_span("Lexical: "),
                Span::styled(
                    format!(
                        "{} in {} ms",
                        response.diagnostics.lexical_candidates,
                        response.diagnostics.lexical_duration_ms
                    ),
                    Style::default().fg(Theme::TEXT),
                ),
                Span::raw("   "),
                label_span("Semantic: "),
                Span::styled(
                    format!(
                        "{} in {} ms",
                        response.diagnostics.semantic_candidates,
                        response.diagnostics.semantic_duration_ms
                    ),
                    Style::default().fg(Theme::TEXT),
                ),
                Span::raw("   "),
                label_span("Rerank: "),
                Span::styled(
                    format!("{} ms", response.diagnostics.rerank_duration_ms),
                    Style::default().fg(Theme::TEXT),
                ),
            ]),
        ]
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
    frame.render_widget(answer, chunks[0]);

    let lower = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    let header = Row::new(["Summary", "Type", "Match", "Score"]).style(
        Style::default()
            .fg(Theme::ACCENT_STRONG)
            .bg(Theme::PANEL_ALT)
            .add_modifier(Modifier::BOLD),
    );
    let rows = app.query_results().iter().map(query_row);
    let table = Table::new(
        rows,
        [
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
    let mut state = app.query_table_state.clone();
    frame.render_stateful_widget(table, lower[0], &mut state);

    let detail_text = if let Some(result) = app.query_results().get(app.query_selected_index) {
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
                "chunk={:.2} | entry={:.2} | semantic={:.2} | overlap={:.0}% | relation={:.2}",
                result.debug.chunk_fts,
                result.debug.entry_fts,
                result.debug.semantic_similarity,
                result.debug.term_overlap * 100.0,
                result.debug.relation_boost
            ),
            Style::default().fg(Theme::TEXT),
        )));
        lines.push(Line::from(Span::styled(
            format!(
                "phrases={} | tags={} | paths={} | importance={} | confidence={:.2} | recency={:.2}",
                result.debug.exact_phrase_matches,
                result.debug.tag_match_count,
                result.debug.path_match_count,
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

        if let Some(detail) = &app.query_selected_detail {
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

fn current_query_display(app: &App) -> String {
    match &app.input_mode {
        InputMode::Query(value) => value.clone(),
        _ => app.query_text.clone(),
    }
}

fn draw_resume_tab(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let lines = if let Some(response) = &app.resume_response {
        let mut lines = Vec::new();
        if app.resume_loading {
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
    } else if app.resume_loading {
        vec![Line::from(Span::styled(
            "Loading resume in the background...",
            Style::default().fg(Theme::ACCENT),
        ))]
    } else if let Some(error) = &app.resume_error {
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
        .scroll((app.resume_scroll, 0))
        .wrap(Wrap { trim: false })
        .style(Style::default().bg(Theme::PANEL_ALT))
        .block(themed_block(format!(
            "Resume (scroll {})",
            app.resume_scroll
        )));
    frame.render_widget(paragraph, area);
}

fn append_resume_briefing_lines(lines: &mut Vec<Line<'static>>, briefing: &str) {
    for raw_line in briefing.lines() {
        let trimmed = raw_line.trim_end();
        if trimmed.is_empty() {
            lines.push(Line::from(""));
            continue;
        }

        let line = if let Some(heading) = trimmed.strip_prefix("### ") {
            Line::from(Span::styled(
                heading.to_string(),
                Style::default()
                    .fg(Theme::ACCENT_STRONG)
                    .add_modifier(Modifier::BOLD),
            ))
        } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            Line::from(Span::styled(
                trimmed.to_string(),
                Style::default().fg(Theme::TEXT),
            ))
        } else {
            Line::from(Span::styled(
                trimmed.to_string(),
                Style::default().fg(Theme::TEXT),
            ))
        };
        lines.push(line);
    }
}

fn draw_activity_tab(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(area);

    let header = Row::new(["When", "Kind", "Summary"]).style(
        Style::default()
            .fg(Theme::ACCENT_STRONG)
            .bg(Theme::PANEL_ALT)
            .add_modifier(Modifier::BOLD),
    );
    let rows = app.activity_events.iter().map(activity_row);
    let table = Table::new(
        rows,
        [
            Constraint::Length(16),
            Constraint::Length(11),
            Constraint::Percentage(100),
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
        "Activity ({})",
        app.activity_events.len()
    )));
    let mut state = app.activity_table_state.clone();
    frame.render_stateful_widget(table, chunks[0], &mut state);

    let detail_lines = if let Some(entry) = app.activity_events.get(app.activity_selected_index) {
        activity_detail_lines(entry)
    } else {
        vec![Line::from(Span::styled(
            "No activity yet. Keep the TUI open while queries, captures, curations, reindexing, re-embedding, archiving, or deletions happen for this project.",
            Style::default().fg(Theme::MUTED),
        ))]
    };

    let detail = Paragraph::new(detail_lines)
        .style(Style::default().bg(Theme::PANEL))
        .wrap(Wrap { trim: false })
        .block(themed_block("Activity Detail"));
    frame.render_widget(detail, chunks[1]);
}

fn lines_for_named_counts(items: Vec<(String, i64)>, empty: &str) -> Vec<Line<'static>> {
    if items.is_empty() {
        vec![Line::from(empty.to_string())]
    } else {
        items
            .into_iter()
            .map(|(name, count)| {
                Line::from(vec![
                    Span::styled(name, Style::default().fg(Theme::TEXT)),
                    Span::styled(": ", Style::default().fg(Theme::MUTED)),
                    Span::styled(count.to_string(), Style::default().fg(Theme::ACCENT_STRONG)),
                ])
            })
            .collect()
    }
}

fn watcher_summary_text(app: &App) -> String {
    let Some(summary) = &app.overview.watchers else {
        return "no watcher presence reported".to_string();
    };

    format!(
        "{} healthy / {} unhealthy / stale after {}s / last {}",
        summary.active_count,
        summary.unhealthy_count,
        summary.stale_after_seconds,
        summary
            .last_heartbeat_at
            .map(format_timestamp_short)
            .unwrap_or_else(|| "n/a".to_string())
    )
}

fn watcher_detail_lines(app: &App) -> Vec<Line<'static>> {
    let Some(summary) = &app.overview.watchers else {
        return vec![
            Line::from(Span::styled(
                "No watcher presence reported.",
                Style::default().fg(Theme::MUTED),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Start a watcher with `memory watcher enable --project <slug>` or `memory watcher run --project <slug>`.",
                Style::default().fg(Theme::MUTED),
            )),
        ];
    };
    if summary.watchers.is_empty() {
        return vec![
            Line::from(Span::styled(
                format!(
                    "0 healthy watcher(s), {} unhealthy. Stale after {}s.",
                    summary.unhealthy_count, summary.stale_after_seconds
                ),
                Style::default().fg(Theme::MUTED),
            )),
            Line::from(Span::styled(
                "Start a watcher with `memory watcher enable --project <slug>` or `memory watcher run --project <slug>`.",
                Style::default().fg(Theme::MUTED),
            )),
        ];
    }

    let mut lines = vec![Line::from(Span::styled(
        format!(
            "{} active watcher(s), stale after {}s.",
            summary.active_count, summary.stale_after_seconds
        ),
        Style::default().fg(Theme::TEXT),
    ))];
    if summary.unhealthy_count > 0 {
        lines.push(Line::from(Span::styled(
            format!(
                "{} watcher(s) currently unhealthy.",
                summary.unhealthy_count
            ),
            Style::default().fg(Theme::WARNING),
        )));
    }
    if let Some(last_heartbeat) = summary.last_heartbeat_at {
        lines.push(Line::from(vec![
            label_span("Last heartbeat: "),
            Span::styled(
                format_timestamp_full(last_heartbeat),
                Style::default().fg(Theme::TEXT),
            ),
        ]));
    }
    lines.push(Line::from(""));
    for watcher in &summary.watchers {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} ", watcher.hostname),
                Style::default().fg(Theme::ACCENT),
            ),
            Span::styled(
                format!("pid={} ", watcher.pid),
                Style::default().fg(Theme::ACCENT_STRONG),
            ),
            Span::styled(
                format!("{} ", watcher.mode),
                Style::default().fg(Theme::TEXT),
            ),
            Span::styled(
                format_timestamp_short(watcher.last_heartbeat_at),
                Style::default().fg(Theme::MUTED),
            ),
        ]));
        lines.push(Line::from(vec![
            label_span("  status: "),
            watcher_health_span(&watcher.health),
            Span::styled(
                if watcher.managed_by_service {
                    " managed".to_string()
                } else {
                    " manual".to_string()
                },
                Style::default().fg(Theme::MUTED),
            ),
        ]));
        lines.push(Line::from(Span::styled(
            format!("  repo: {}", watcher.repo_root),
            Style::default().fg(Theme::MUTED),
        )));
        lines.push(Line::from(Span::styled(
            format!("  watcher: {}", watcher.watcher_id),
            Style::default().fg(Theme::MUTED),
        )));
        lines.push(Line::from(Span::styled(
            format!("  host service: {}", watcher.host_service_id),
            Style::default().fg(Theme::MUTED),
        )));
        lines.push(Line::from(Span::styled(
            format!("  restart attempts: {}", watcher.restart_attempt_count),
            Style::default().fg(Theme::MUTED),
        )));
        if let Some(last_restart) = watcher.last_restart_attempt_at {
            lines.push(Line::from(Span::styled(
                format!(
                    "  last restart attempt: {}",
                    format_timestamp_full(last_restart)
                ),
                Style::default().fg(Theme::MUTED),
            )));
        }
        lines.push(Line::from(""));
    }
    lines
}

fn replacement_proposal_lines(app: &App) -> Vec<Line<'static>> {
    if app.replacement_proposals.is_empty() {
        return vec![
            Line::from(Span::styled(
                format!(
                    "Policy: {}. No pending replacement proposals.",
                    app.replacement_policy
                ),
                Style::default().fg(Theme::TEXT),
            )),
            Line::from(Span::styled(
                "Use `p` to cycle policy. Clear updates replace automatically; ambiguous ones queue here.",
                Style::default().fg(Theme::MUTED),
            )),
        ];
    }

    let proposal = &app.replacement_proposals[app.replacement_selected_index];
    let mut lines = vec![
        Line::from(Span::styled(
            format!(
                "{} pending proposal(s) / selected {}/{}",
                app.replacement_proposals.len(),
                app.replacement_selected_index + 1,
                app.replacement_proposals.len()
            ),
            Style::default().fg(Theme::TEXT),
        )),
        Line::from(vec![
            label_span("Target: "),
            Span::styled(
                proposal.target_summary.clone(),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
        Line::from(vec![
            label_span("Candidate: "),
            Span::styled(
                proposal.candidate_summary.clone(),
                Style::default().fg(Theme::ACCENT),
            ),
        ]),
        Line::from(vec![
            label_span("Type / Score / Policy: "),
            Span::styled(
                format!(
                    "{} / {} / {}",
                    proposal.candidate_memory_type, proposal.score, proposal.policy
                ),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
    ];
    if !proposal.reasons.is_empty() {
        lines.push(Line::from(vec![
            label_span("Why: "),
            Span::styled(
                proposal.reasons.join(", "),
                Style::default().fg(Theme::MUTED),
            ),
        ]));
    }
    lines.push(Line::from(Span::styled(
        proposal.candidate_canonical_text.clone(),
        Style::default().fg(Theme::MUTED),
    )));
    lines
}

fn write_replacement_policy(repo_root: &Path, policy: ReplacementPolicy) -> Result<()> {
    let path = repo_agent_settings_path(repo_root);
    let mut value = if path.exists() {
        fs::read_to_string(&path)?
            .parse::<toml::Value>()
            .context("parse .agents/memory-layer.toml")?
    } else {
        toml::Value::Table(toml::map::Map::new())
    };
    let table = value
        .as_table_mut()
        .context(".agents/memory-layer.toml must be a top-level table")?;
    let curation = table
        .entry("curation".to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    let curation_table = curation
        .as_table_mut()
        .context("[curation] must be a table")?;
    curation_table.insert(
        "replacement_policy".to_string(),
        toml::Value::String(policy.to_string()),
    );
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, toml::to_string_pretty(&value)?)?;
    Ok(())
}

fn memory_row(item: &ProjectMemoryListItem) -> Row<'static> {
    let row_style = match item.status {
        MemoryStatus::Active => Style::default().fg(Theme::TEXT).bg(Theme::PANEL),
        MemoryStatus::Archived => Style::default().fg(Theme::MUTED).bg(Theme::PANEL),
    };
    Row::new(vec![
        Cell::from(Span::styled(
            item.summary.clone(),
            Style::default().fg(Theme::TEXT),
        )),
        Cell::from(memory_type_span(&item.memory_type)),
        Cell::from(status_span(match item.status {
            MemoryStatus::Active => "active",
            MemoryStatus::Archived => "archived",
        })),
        Cell::from(Span::styled(
            format!("{:.2}", item.confidence),
            confidence_style(item.confidence),
        )),
        Cell::from(Span::styled(
            format_timestamp_medium(item.updated_at),
            Style::default().fg(Theme::MUTED),
        )),
    ])
    .style(row_style)
}

fn query_row(item: &QueryResult) -> Row<'static> {
    Row::new(vec![
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

fn activity_row(item: &ActivityEntry) -> Row<'static> {
    Row::new(vec![
        Cell::from(Span::styled(
            format_timestamp_short(activity_recorded_at(item)),
            Style::default().fg(Theme::TEXT),
        )),
        Cell::from(activity_entry_kind_span(item)),
        Cell::from(Span::styled(
            activity_summary(item),
            Style::default().fg(Theme::TEXT),
        )),
    ])
}

fn activity_detail_lines(entry: &ActivityEntry) -> Vec<Line<'static>> {
    match entry {
        ActivityEntry::Backend(event) => backend_activity_detail_lines(event),
        ActivityEntry::Query(entry) => {
            let mut lines = vec![
                Line::from(vec![
                    label_span("When: "),
                    Span::styled(
                        format_timestamp_full(entry.recorded_at),
                        Style::default().fg(Theme::TEXT),
                    ),
                ]),
                Line::from(vec![
                    label_span("Project: "),
                    Span::styled(entry.project.clone(), Style::default().fg(Theme::TEXT)),
                ]),
                Line::from(vec![
                    label_span("Kind: "),
                    activity_entry_kind_span(&ActivityEntry::Query(QueryActivityEntry {
                        recorded_at: entry.recorded_at,
                        project: entry.project.clone(),
                        request: entry.request.clone(),
                        duration_ms: entry.duration_ms,
                        outcome: entry.outcome.clone(),
                    })),
                ]),
                Line::from(vec![
                    label_span("Duration: "),
                    Span::styled(
                        format!("{} ms", entry.duration_ms),
                        Style::default().fg(Theme::TEXT),
                    ),
                    Span::raw("   "),
                    label_span("Top K: "),
                    Span::styled(
                        entry.request.top_k.to_string(),
                        Style::default().fg(Theme::TEXT),
                    ),
                    Span::raw("   "),
                    label_span("Min confidence: "),
                    Span::styled(
                        entry
                            .request
                            .min_confidence
                            .map(|value| format!("{value:.2}"))
                            .unwrap_or_else(|| "none".to_string()),
                        Style::default().fg(Theme::TEXT),
                    ),
                ]),
                Line::from(vec![
                    label_span("Filters: "),
                    Span::styled(
                        format_query_filters(&entry.request.filters),
                        Style::default().fg(Theme::TEXT),
                    ),
                ]),
                Line::from(vec![
                    label_span("Roundtrip: "),
                    Span::styled(
                        format!("{} ms", entry.duration_ms),
                        Style::default().fg(Theme::TEXT),
                    ),
                ]),
                Line::from(""),
                Line::from(vec![section_span("Question")]),
                Line::from(Span::styled(
                    entry.request.query.clone(),
                    Style::default().fg(Theme::TEXT),
                )),
                Line::from(""),
            ];

            match &entry.outcome {
                QueryLogOutcome::Success(response) => {
                    lines.push(Line::from(vec![section_span("Answer")]));
                    lines.push(Line::from(Span::styled(
                        response.answer.clone(),
                        Style::default().fg(Theme::TEXT),
                    )));
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
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
                        label_span("Results: "),
                        Span::styled(
                            response.results.len().to_string(),
                            Style::default().fg(Theme::TEXT),
                        ),
                    ]));
                    lines.push(Line::from(vec![
                        label_span("Server timings: "),
                        Span::styled(
                            format!(
                                "lexical {} ms | semantic {} ms | rerank {} ms | total {} ms",
                                response.diagnostics.lexical_duration_ms,
                                response.diagnostics.semantic_duration_ms,
                                response.diagnostics.rerank_duration_ms,
                                response.diagnostics.total_duration_ms
                            ),
                            Style::default().fg(Theme::TEXT),
                        ),
                    ]));
                    lines.push(Line::from(vec![
                        label_span("Candidate counts: "),
                        Span::styled(
                            format!(
                                "lexical {} | semantic {} | merged {} | returned {} | relation {}",
                                response.diagnostics.lexical_candidates,
                                response.diagnostics.semantic_candidates,
                                response.diagnostics.merged_candidates,
                                response.diagnostics.returned_results,
                                response.diagnostics.relation_augmented_candidates
                            ),
                            Style::default().fg(Theme::TEXT),
                        ),
                    ]));
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![section_span("Returned Memories")]));
                    if response.results.is_empty() {
                        lines.push(Line::from(Span::styled(
                            "No memories returned.",
                            Style::default().fg(Theme::MUTED),
                        )));
                    } else {
                        for result in &response.results {
                            lines.push(Line::from(Span::styled(
                                format!(
                                    "{} | {} [{} / {}] score={:.2}",
                                    result.memory_id,
                                    result.summary,
                                    result.memory_type,
                                    result.match_kind,
                                    result.score
                                ),
                                Style::default().fg(Theme::TEXT),
                            )));
                            lines.push(Line::from(Span::styled(
                                format!("  snippet: {}", result.snippet),
                                Style::default().fg(Theme::MUTED),
                            )));
                            lines.push(Line::from(Span::styled(
                                format!(
                                    "  debug: chunk {:.2} | entry {:.2} | semantic {:.2} | relation {:.2}",
                                    result.debug.chunk_fts,
                                    result.debug.entry_fts,
                                    result.debug.semantic_similarity,
                                    result.debug.relation_boost
                                ),
                                Style::default().fg(Theme::MUTED),
                            )));
                            if !result.score_explanation.is_empty() {
                                lines.push(Line::from(Span::styled(
                                    format!("  why: {}", result.score_explanation.join(" | ")),
                                    Style::default().fg(Theme::ACCENT),
                                )));
                            }
                            if !result.tags.is_empty() {
                                lines.push(Line::from(Span::styled(
                                    format!("  tags: {}", result.tags.join(", ")),
                                    Style::default().fg(Theme::MUTED),
                                )));
                            }
                        }
                    }
                }
                QueryLogOutcome::Error(error) => {
                    lines.push(Line::from(vec![section_span("Error")]));
                    lines.push(Line::from(Span::styled(
                        error.clone(),
                        Style::default().fg(Theme::DANGER),
                    )));
                }
            }

            lines
        }
    }
}

fn backend_activity_detail_lines(event: &ActivityEvent) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(vec![
            label_span("When: "),
            Span::styled(
                format_timestamp_full(event.recorded_at),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
        Line::from(vec![
            label_span("Project: "),
            Span::styled(event.project.clone(), Style::default().fg(Theme::TEXT)),
        ]),
        Line::from(vec![label_span("Kind: "), activity_kind_span(&event.kind)]),
        Line::from(vec![
            label_span("Memory Id: "),
            Span::styled(
                event
                    .memory_id
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "n/a".to_string()),
                Style::default().fg(Theme::MUTED),
            ),
        ]),
        Line::from(""),
        Line::from(vec![section_span("Summary")]),
        Line::from(Span::styled(
            event.summary.clone(),
            Style::default().fg(Theme::TEXT),
        )),
    ];

    if let Some(details) = &event.details {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![section_span("Operation Detail")]));
        match details {
            ActivityDetails::Plan {
                action,
                title,
                thread_key,
                total_items,
                completed_items,
                remaining_items,
                source_path,
                verified_complete,
            } => {
                lines.push(Line::from(vec![
                    label_span("Action: "),
                    plan_activity_action_span(action),
                ]));
                lines.push(activity_kv_line("Title", title.clone()));
                lines.push(activity_kv_line("Thread", thread_key.clone()));
                lines.push(activity_kv_line("Total items", total_items.to_string()));
                lines.push(activity_kv_line("Completed", completed_items.to_string()));
                lines.push(activity_kv_line(
                    "Remaining",
                    remaining_items.len().to_string(),
                ));
                lines.push(activity_kv_line(
                    "Verified complete",
                    verified_complete.to_string(),
                ));
                if let Some(source_path) = source_path {
                    lines.push(activity_kv_line("Source path", source_path.clone()));
                }
                if !remaining_items.is_empty() {
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![section_span("Remaining Items")]));
                    for item in remaining_items {
                        lines.push(Line::from(Span::styled(
                            format!("- {item}"),
                            Style::default().fg(Theme::TEXT),
                        )));
                    }
                }
            }
            ActivityDetails::Scan {
                dry_run,
                candidate_count,
                files_considered,
                commits_considered,
                index_reused,
                report_path,
                capture_id,
                curate_run_id,
            } => {
                lines.push(activity_kv_line("Dry run", dry_run.to_string()));
                lines.push(activity_kv_line("Candidates", candidate_count.to_string()));
                lines.push(activity_kv_line("Files", files_considered.to_string()));
                lines.push(activity_kv_line("Commits", commits_considered.to_string()));
                lines.push(activity_kv_line("Index reused", index_reused.to_string()));
                lines.push(activity_kv_line("Report", report_path.clone()));
                if let Some(capture_id) = capture_id {
                    lines.push(activity_kv_line("Capture", capture_id.clone()));
                }
                if let Some(curate_run_id) = curate_run_id {
                    lines.push(activity_kv_line("Curate run", curate_run_id.clone()));
                }
            }
            ActivityDetails::Checkpoint {
                repo_root,
                marked_at,
                note,
                git_branch,
                git_head,
            } => {
                lines.push(activity_kv_line(
                    "Marked at",
                    format_timestamp(Some(*marked_at)),
                ));
                lines.push(activity_kv_line("Repo root", repo_root.clone()));
                lines.push(activity_kv_line(
                    "Note",
                    note.clone().unwrap_or_else(|| "n/a".to_string()),
                ));
                lines.push(activity_kv_line(
                    "Branch",
                    git_branch.clone().unwrap_or_else(|| "n/a".to_string()),
                ));
                lines.push(activity_kv_line(
                    "HEAD",
                    git_head.clone().unwrap_or_else(|| "n/a".to_string()),
                ));
            }
            ActivityDetails::CommitSync {
                imported_count,
                updated_count,
                total_received,
                newest_commit,
                oldest_commit,
            } => {
                lines.push(activity_kv_line("Imported", imported_count.to_string()));
                lines.push(activity_kv_line("Updated", updated_count.to_string()));
                lines.push(activity_kv_line("Received", total_received.to_string()));
                if let Some(newest_commit) = newest_commit {
                    lines.push(activity_kv_line("Newest", newest_commit.clone()));
                }
                if let Some(oldest_commit) = oldest_commit {
                    lines.push(activity_kv_line("Oldest", oldest_commit.clone()));
                }
            }
            ActivityDetails::BundleTransfer {
                bundle_id,
                item_count,
                source_project,
            } => {
                lines.push(activity_kv_line("Bundle", bundle_id.clone()));
                lines.push(activity_kv_line("Items", item_count.to_string()));
                if let Some(source_project) = source_project {
                    lines.push(activity_kv_line("Source project", source_project.clone()));
                }
            }
            ActivityDetails::Query {
                query,
                top_k,
                result_count,
                confidence,
                insufficient_evidence,
                total_duration_ms,
                answer,
                error,
            } => {
                lines.push(activity_kv_line("Query", query.clone()));
                lines.push(activity_kv_line("Top K", top_k.to_string()));
                lines.push(activity_kv_line("Results", result_count.to_string()));
                lines.push(activity_kv_line("Confidence", format!("{confidence:.2}")));
                lines.push(activity_kv_line(
                    "Insufficient evidence",
                    insufficient_evidence.to_string(),
                ));
                lines.push(activity_kv_line(
                    "Duration",
                    format!("{total_duration_ms} ms"),
                ));
                if let Some(answer) = answer {
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![section_span("Answer")]));
                    lines.push(Line::from(Span::styled(
                        answer.clone(),
                        Style::default().fg(Theme::TEXT),
                    )));
                }
                if let Some(error) = error {
                    lines.push(activity_kv_line("Error", error.clone()));
                }
            }
            ActivityDetails::CaptureTask {
                session_id,
                task_id,
                raw_capture_id,
                idempotency_key,
                task_title,
                writer_id,
            } => {
                lines.push(activity_kv_line("Session", session_id.to_string()));
                lines.push(activity_kv_line("Task", task_id.to_string()));
                lines.push(activity_kv_line("Raw capture", raw_capture_id.to_string()));
                lines.push(activity_kv_line("Idempotency", idempotency_key.clone()));
                if let Some(task_title) = task_title {
                    lines.push(activity_kv_line("Task title", task_title.clone()));
                }
                lines.push(activity_kv_line("Writer", writer_id.clone()));
            }
            ActivityDetails::Curate {
                run_id,
                input_count,
                output_count,
                replaced_count,
                proposal_count,
            } => {
                lines.push(activity_kv_line("Run", run_id.to_string()));
                lines.push(activity_kv_line("Input captures", input_count.to_string()));
                lines.push(activity_kv_line(
                    "Output memories",
                    output_count.to_string(),
                ));
                lines.push(activity_kv_line("Replacements", replaced_count.to_string()));
                lines.push(activity_kv_line(
                    "Queued proposals",
                    proposal_count.to_string(),
                ));
            }
            ActivityDetails::MemoryReplacement {
                old_memory_id,
                old_summary,
                new_memory_id,
                new_summary,
                automatic,
                policy,
            } => {
                lines.push(activity_kv_line("Old memory", old_memory_id.to_string()));
                lines.push(activity_kv_line("Old summary", old_summary.clone()));
                lines.push(activity_kv_line("New memory", new_memory_id.to_string()));
                lines.push(activity_kv_line("New summary", new_summary.clone()));
                lines.push(activity_kv_line("Automatic", automatic.to_string()));
                lines.push(activity_kv_line("Policy", policy.to_string()));
            }
            ActivityDetails::Reindex { reindexed_entries } => {
                lines.push(activity_kv_line(
                    "Reindexed entries",
                    reindexed_entries.to_string(),
                ));
            }
            ActivityDetails::Reembed { reembedded_chunks } => {
                lines.push(activity_kv_line(
                    "Re-embedded chunks",
                    reembedded_chunks.to_string(),
                ));
            }
            ActivityDetails::Archive {
                archived_count,
                max_confidence,
                max_importance,
            } => {
                lines.push(activity_kv_line(
                    "Archived count",
                    archived_count.to_string(),
                ));
                lines.push(activity_kv_line(
                    "Max confidence",
                    format!("{max_confidence:.2}"),
                ));
                lines.push(activity_kv_line(
                    "Max importance",
                    max_importance.to_string(),
                ));
            }
            ActivityDetails::DeleteMemory { deleted, summary } => {
                lines.push(activity_kv_line("Deleted", deleted.to_string()));
                lines.push(activity_kv_line("Deleted summary", summary.clone()));
            }
            ActivityDetails::WatcherHealth {
                watcher_id,
                hostname,
                health,
                managed_by_service,
                restart_attempt_count,
                previous_health,
                recovered_after_restart_attempts,
                message,
            } => {
                lines.push(activity_kv_line("Watcher", watcher_id.clone()));
                lines.push(activity_kv_line("Hostname", hostname.clone()));
                lines.push(Line::from(vec![
                    label_span("Health: "),
                    watcher_health_span(health),
                ]));
                if let Some(previous_health) = previous_health {
                    lines.push(Line::from(vec![
                        label_span("Previous health: "),
                        watcher_health_span(previous_health),
                    ]));
                }
                lines.push(activity_kv_line(
                    "Managed by service",
                    managed_by_service.to_string(),
                ));
                lines.push(activity_kv_line(
                    "Restart attempts",
                    restart_attempt_count.to_string(),
                ));
                if let Some(attempts) = recovered_after_restart_attempts {
                    lines.push(activity_kv_line(
                        "Recovered after attempts",
                        attempts.to_string(),
                    ));
                }
                lines.push(activity_kv_line(
                    "Message",
                    message.clone().unwrap_or_else(|| "n/a".to_string()),
                ));
            }
        }
    }

    lines
}

fn activity_kv_line(label: &str, value: String) -> Line<'static> {
    Line::from(vec![
        label_span(format!("{label}: ")),
        Span::styled(value, Style::default().fg(Theme::TEXT)),
    ])
}

fn format_query_filters(filters: &QueryFilters) -> String {
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

fn truncate_activity_text(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn activity_recorded_at(item: &ActivityEntry) -> DateTime<Utc> {
    match item {
        ActivityEntry::Backend(event) => event.recorded_at,
        ActivityEntry::Query(entry) => entry.recorded_at,
    }
}

fn activity_summary(item: &ActivityEntry) -> String {
    match item {
        ActivityEntry::Backend(event) => event.summary.clone(),
        ActivityEntry::Query(entry) => {
            let preview = truncate_activity_text(&entry.request.query, 52);
            match &entry.outcome {
                QueryLogOutcome::Success(response) => format!(
                    "{} | {} results | {} ms | conf {:.2}",
                    preview,
                    response.results.len(),
                    entry.duration_ms,
                    response.confidence
                ),
                QueryLogOutcome::Error(_) => {
                    format!("{preview} | error | {} ms", entry.duration_ms)
                }
            }
        }
    }
}

fn watcher_transition_status_message(
    summary: &str,
    health: &WatcherHealth,
    previous_health: Option<&WatcherHealth>,
    message: Option<&str>,
) -> String {
    if matches!(health, WatcherHealth::Healthy)
        && previous_health.is_some_and(|value| !matches!(value, WatcherHealth::Healthy))
    {
        format!("Watcher recovered: {summary}")
    } else if let Some(message) = message {
        format!("Watcher status: {summary} ({message})")
    } else {
        format!("Watcher status: {summary}")
    }
}

fn format_timestamp(value: Option<DateTime<Utc>>) -> String {
    value
        .map(format_timestamp_full)
        .unwrap_or_else(|| "n/a".to_string())
}

fn format_timestamp_full(value: DateTime<Utc>) -> String {
    value
        .with_timezone(&Local)
        .format("%Y-%m-%d %H:%M:%S %Z")
        .to_string()
}

fn format_timestamp_medium(value: DateTime<Utc>) -> String {
    value
        .with_timezone(&Local)
        .format("%Y-%m-%d %H:%M %Z")
        .to_string()
}

fn format_timestamp_short(value: DateTime<Utc>) -> String {
    value
        .with_timezone(&Local)
        .format("%H:%M:%S %Z")
        .to_string()
}

fn format_timestamp_timeline(value: DateTime<Utc>) -> String {
    value
        .with_timezone(&Local)
        .format("%m-%d %H:%M %Z")
        .to_string()
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

fn themed_block<'a>(title: impl Into<Line<'a>>) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Theme::BORDER))
        .title(title)
        .title_style(
            Style::default()
                .fg(Theme::TITLE)
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().bg(Theme::PANEL))
}

fn accent_span(value: impl Into<String>) -> Span<'static> {
    Span::styled(
        value.into(),
        Style::default()
            .fg(Theme::ACCENT)
            .add_modifier(Modifier::BOLD),
    )
}

fn label_span(value: impl Into<String>) -> Span<'static> {
    Span::styled(
        value.into(),
        Style::default()
            .fg(Theme::ACCENT_STRONG)
            .add_modifier(Modifier::BOLD),
    )
}

fn section_span(value: impl Into<String>) -> Span<'static> {
    Span::styled(
        value.into(),
        Style::default()
            .fg(Theme::TITLE)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
    )
}

fn activity_kind_span(kind: &ActivityKind) -> Span<'static> {
    let (label, color) = match kind {
        ActivityKind::Checkpoint => ("checkpoint", Theme::ACCENT_STRONG),
        ActivityKind::Scan => ("scan", Theme::ACCENT_STRONG),
        ActivityKind::Plan => ("plan", Theme::ACCENT_STRONG),
        ActivityKind::CommitSync => ("commit-sync", Theme::ACCENT_STRONG),
        ActivityKind::BundleExport => ("bundle-export", Theme::ACCENT_STRONG),
        ActivityKind::BundleImport => ("bundle-import", Theme::ACCENT_STRONG),
        ActivityKind::Query => ("query", Theme::ACCENT),
        ActivityKind::QueryError => ("query-error", Theme::DANGER),
        ActivityKind::MemoryReplacement => ("replacement", Theme::WARNING),
        ActivityKind::CaptureTask => ("capture", Theme::ACCENT),
        ActivityKind::Curate => ("curate", Theme::SUCCESS),
        ActivityKind::Reindex => ("reindex", Theme::ACCENT_STRONG),
        ActivityKind::Reembed => ("reembed", Theme::ACCENT_STRONG),
        ActivityKind::Archive => ("archive", Theme::WARNING),
        ActivityKind::DeleteMemory => ("delete", Theme::DANGER),
        ActivityKind::WatcherHealth => ("watcher-health", Theme::WARNING),
    };
    Span::styled(
        label,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

fn plan_activity_action_span(action: &PlanActivityAction) -> Span<'static> {
    let (label, color) = match action {
        PlanActivityAction::Started => ("started", Theme::ACCENT_STRONG),
        PlanActivityAction::Synced => ("synced", Theme::ACCENT),
        PlanActivityAction::FinishBlocked => ("finish-blocked", Theme::WARNING),
        PlanActivityAction::FinishVerified => ("finish-verified", Theme::SUCCESS),
    };
    Span::styled(
        label,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

fn watcher_health_span(health: &WatcherHealth) -> Span<'static> {
    let (label, color) = match health {
        WatcherHealth::Healthy => ("healthy", Theme::SUCCESS),
        WatcherHealth::Stale => ("stale", Theme::WARNING),
        WatcherHealth::Restarting => ("restarting", Theme::ACCENT_STRONG),
        WatcherHealth::Failed => ("failed", Theme::DANGER),
    };
    Span::styled(
        label,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

fn watcher_health_label(health: &WatcherHealth) -> &'static str {
    match health {
        WatcherHealth::Healthy => "healthy",
        WatcherHealth::Stale => "stale",
        WatcherHealth::Restarting => "restarting",
        WatcherHealth::Failed => "failed",
    }
}

fn query_match_span(kind: &QueryMatchKind) -> Span<'static> {
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

fn activity_entry_kind_span(item: &ActivityEntry) -> Span<'static> {
    match item {
        ActivityEntry::Backend(event) => {
            if let Some(ActivityDetails::Plan { action, .. }) = event.details.as_ref() {
                return plan_activity_action_span(action);
            }
            if let Some(ActivityDetails::WatcherHealth {
                health: WatcherHealth::Healthy,
                previous_health: Some(previous_health),
                ..
            }) = event.details.as_ref()
            {
                return Span::styled(
                    format!("watcher-{}", watcher_health_label(previous_health)),
                    Style::default()
                        .fg(Theme::SUCCESS)
                        .add_modifier(Modifier::BOLD),
                );
            }
            activity_kind_span(&event.kind)
        }
        ActivityEntry::Query(entry) => match &entry.outcome {
            QueryLogOutcome::Success(response) => {
                if response.insufficient_evidence {
                    Span::styled(
                        "query-weak",
                        Style::default()
                            .fg(Theme::WARNING)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    Span::styled(
                        "query",
                        Style::default()
                            .fg(Theme::ACCENT)
                            .add_modifier(Modifier::BOLD),
                    )
                }
            }
            QueryLogOutcome::Error(_) => Span::styled(
                "query-error",
                Style::default()
                    .fg(Theme::DANGER)
                    .add_modifier(Modifier::BOLD),
            ),
        },
    }
}

fn status_span(value: &str) -> Span<'static> {
    let color = match value {
        "active" | "ok" | "up" => Theme::SUCCESS,
        "archived" | "unknown" => Theme::WARNING,
        _ => Theme::DANGER,
    };
    Span::styled(
        value.to_string(),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

fn service_span(value: &str) -> Span<'static> {
    let color = match value {
        "ok" | "up" => Theme::SUCCESS,
        "unknown" => Theme::WARNING,
        _ => Theme::DANGER,
    };
    Span::styled(
        value.to_string(),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

fn tui_status_label(app: &App) -> &'static str {
    match app.ui_status {
        UiStatus::Loading => "loading",
        UiStatus::Busy => "busy",
        UiStatus::Ready => "ready",
        UiStatus::Error => "error",
    }
}

fn tui_status_color(app: &App) -> Color {
    match app.ui_status {
        UiStatus::Loading => Theme::ACCENT,
        UiStatus::Busy => Theme::ACCENT_STRONG,
        UiStatus::Ready => Theme::SUCCESS,
        UiStatus::Error => Theme::DANGER,
    }
}

fn service_status_label(app: &App) -> &'static str {
    if !app.health_ok {
        "down"
    } else if !matches!(app.overview.database_status.as_str(), "ok" | "up") {
        "degraded"
    } else {
        match app.overview.service_status.as_str() {
            "ok" | "up" => "up",
            "unknown" => "unknown",
            _ => "degraded",
        }
    }
}

fn service_status_color(app: &App) -> Color {
    match service_status_label(app) {
        "up" => Theme::SUCCESS,
        "unknown" => Theme::WARNING,
        "degraded" => Theme::WARNING,
        _ => Theme::DANGER,
    }
}

fn service_status_detail(app: &App) -> Option<String> {
    if !app.health_ok {
        return Some("db unknown".to_string());
    }
    if !matches!(app.overview.database_status.as_str(), "ok" | "up") {
        Some(format!("db {}", app.overview.database_status))
    } else {
        None
    }
}

fn watcher_bar_status_label(app: &App) -> &'static str {
    if !app.health_ok {
        return "unknown";
    }

    let Some(summary) = &app.overview.watchers else {
        return "none";
    };

    if summary.unhealthy_count > 0 {
        "degraded"
    } else if summary.active_count > 0 {
        "ok"
    } else {
        "none"
    }
}

fn watcher_bar_status_color(app: &App) -> Color {
    match watcher_bar_status_label(app) {
        "ok" => Theme::SUCCESS,
        "none" => Theme::MUTED,
        "unknown" => Theme::WARNING,
        "degraded" => Theme::WARNING,
        _ => Theme::TEXT,
    }
}

fn watcher_bar_status_detail(app: &App) -> Option<String> {
    let summary = app.overview.watchers.as_ref()?;
    if summary.unhealthy_count > 0 {
        Some(format!("{} unhealthy", summary.unhealthy_count))
    } else if summary.active_count > 0 {
        Some(format!("{} active", summary.active_count))
    } else {
        None
    }
}

fn memory_type_span(memory_type: &MemoryType) -> Span<'static> {
    let label = memory_type.to_string();
    memory_type_span_from_label(&label)
}

fn memory_type_span_from_label(label: &str) -> Span<'static> {
    let color = match label {
        "architecture" => Color::Rgb(120, 190, 255),
        "convention" => Color::Rgb(149, 220, 180),
        "decision" => Color::Rgb(255, 205, 120),
        "incident" => Color::Rgb(255, 140, 140),
        "debugging" => Color::Rgb(255, 170, 110),
        "environment" => Color::Rgb(190, 170, 255),
        "domain_fact" => Color::Rgb(130, 225, 220),
        "plan" => Color::Rgb(255, 120, 200),
        "all" => Theme::TEXT,
        _ => Theme::TEXT,
    };
    Span::styled(
        label.to_string(),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

fn confidence_style(confidence: f32) -> Style {
    let color = if confidence >= 0.8 {
        Theme::SUCCESS
    } else if confidence >= 0.5 {
        Theme::WARNING
    } else {
        Theme::DANGER
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

fn metric_line<'a>(label: &str, value: Span<'a>) -> Line<'a> {
    Line::from(vec![
        Span::styled(
            format!("{label}: "),
            Style::default()
                .fg(Theme::ACCENT_STRONG)
                .add_modifier(Modifier::BOLD),
        ),
        value,
    ])
}

fn status_message_style(app: &App) -> Style {
    let lowered = app.status_message.to_lowercase();
    let color = if lowered.contains("error") || lowered.contains("failed") {
        Theme::DANGER
    } else if lowered.contains("refresh")
        || lowered.contains("loaded")
        || lowered.contains("curated")
    {
        Theme::ACCENT
    } else {
        Theme::TEXT
    };
    Style::default().fg(color).bg(Theme::PANEL_ALT)
}

fn should_quit(key: KeyEvent, app: &App) -> bool {
    matches!(app.input_mode, InputMode::Normal) && matches!(key.code, KeyCode::Char('q'))
}

fn should_attempt_stream_reconnect(stream_connected: bool, last_attempt: Instant) -> bool {
    !stream_connected && last_attempt.elapsed() >= Duration::from_secs(1)
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
        embedding_chunks_total: 0,
        fresh_embedding_chunks: 0,
        stale_embedding_chunks: 0,
        missing_embedding_chunks: 0,
        embedding_spaces_total: 0,
        active_embedding_provider: None,
        active_embedding_model: None,
        last_memory_at: None,
        last_capture_at: None,
        last_curation_at: None,
        oldest_uncurated_capture_age_hours: None,
        memory_type_breakdown: Vec::new(),
        source_kind_breakdown: Vec::new(),
        top_tags: Vec::<NamedCount>::new(),
        top_files: Vec::<NamedCount>::new(),
        pending_replacement_proposals: 0,
        automation: None,
        watchers: None,
    }
}

fn detect_tool_versions() -> ToolVersions {
    ToolVersions {
        mem_cli: env!("CARGO_PKG_VERSION").to_string(),
        mem_service: env!("CARGO_PKG_VERSION").to_string(),
        memory_watch: env!("CARGO_PKG_VERSION").to_string(),
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

#[cfg(test)]
mod tests {
    use chrono::{Local, TimeZone, Utc};

    use super::{
        App, ToolVersions, UiStatus, empty_overview, format_timestamp, format_timestamp_full,
        format_timestamp_medium, format_timestamp_short, format_timestamp_timeline,
        service_status_label, should_attempt_stream_reconnect, watcher_bar_status_label,
    };
    use mem_api::WatcherPresenceSummary;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};
    use tokio::sync::mpsc;

    #[test]
    fn format_timestamp_returns_na_for_missing_value() {
        assert_eq!(format_timestamp(None), "n/a");
    }

    #[test]
    fn local_timestamp_formatters_match_local_timezone_rendering() {
        let timestamp = Utc.with_ymd_and_hms(2026, 4, 1, 12, 34, 56).unwrap();
        let local = timestamp.with_timezone(&Local);

        let full = format_timestamp_full(timestamp);
        let medium = format_timestamp_medium(timestamp);
        let short = format_timestamp_short(timestamp);
        let timeline = format_timestamp_timeline(timestamp);

        assert_eq!(full, local.format("%Y-%m-%d %H:%M:%S %Z").to_string());
        assert_eq!(medium, local.format("%Y-%m-%d %H:%M %Z").to_string());
        assert_eq!(short, local.format("%H:%M:%S %Z").to_string());
        assert_eq!(timeline, local.format("%m-%d %H:%M %Z").to_string());
    }

    #[test]
    fn footer_statuses_do_not_use_stale_service_or_watcher_state_when_health_is_down() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut app = App::new(
            "memory".to_string(),
            PathBuf::from("/tmp/memory"),
            ToolVersions {
                mem_cli: "0.4.2".to_string(),
                mem_service: "0.4.2".to_string(),
                memory_watch: "0.4.2".to_string(),
            },
            tx,
        );
        app.ui_status = UiStatus::Error;
        app.health_ok = false;
        app.overview = empty_overview("memory".to_string());
        app.overview.service_status = "ok".to_string();
        app.overview.database_status = "up".to_string();
        app.overview.watchers = Some(WatcherPresenceSummary {
            active_count: 2,
            unhealthy_count: 0,
            stale_after_seconds: 90,
            last_heartbeat_at: None,
            watchers: Vec::new(),
        });

        assert_eq!(service_status_label(&app), "down");
        assert_eq!(watcher_bar_status_label(&app), "unknown");
    }

    #[test]
    fn stream_reconnect_attempts_are_rate_limited() {
        let just_attempted = Instant::now();
        assert!(!should_attempt_stream_reconnect(false, just_attempted));

        let overdue = Instant::now() - Duration::from_secs(2);
        assert!(should_attempt_stream_reconnect(false, overdue));
        assert!(!should_attempt_stream_reconnect(true, overdue));
    }
}
