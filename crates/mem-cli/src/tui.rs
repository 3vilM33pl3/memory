use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self as std_mpsc, RecvTimeoutError},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use chrono::{DateTime, Local, Utc};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use mem_agenttop::{
    AgentSession, AgentSnapshot, ChildProcess as AgentChildProcess,
    SessionStatus as AgentSessionStatus,
};
use mem_api::{
    ActivityDetails, ActivityEvent, ActivityKind, DiagnosticInfo, DiagnosticSeverity,
    LlmAuditStatusResponse, MemoryEntryResponse, MemoryStatus, MemoryType, NamedCount,
    PlanActivityAction, Profile, ProjectMemoriesResponse, ProjectMemoryListItem,
    ProjectOverviewResponse, QueryAnswerMethod, QueryFilters, QueryMatchKind, QueryRequest,
    QueryResponse, QueryResult, ReplacementPolicy, ReplacementProposalListResponse,
    ReplacementProposalRecord, ResumeCheckpoint, ResumeRequest, ResumeResponse, StreamRequest,
    StreamResponse, UpToSpeedRequest, UpToSpeedResponse, WatcherHealth,
    load_repo_replacement_policy, read_capnp_text_frame, repo_agent_settings_path,
    write_capnp_text_frame,
};
use mem_platform::preferred_user_state_dir;
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState, Tabs, Wrap},
};
use serde::Deserialize;
use tokio::{
    net::{TcpStream, UnixStream},
    sync::mpsc,
};

use crate::{
    ApiClient, SkillBundleStatus, SkillInventoryReport, SourceKindString, TuiRestartNotice,
    enable_relay_discovery_and_restart_backend, load_tui_restart_notice, project_skill_inventory,
    resume,
};

const STREAM_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

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
    let profile = api.config.profile;
    let mut app = App::new(
        project,
        repo_root,
        detect_tool_versions(profile),
        api.config.cluster.enabled,
        profile,
        background_tx,
    );
    start_agent_snapshot_worker(
        app.background_tx.clone(),
        app.agents_tab_visible.clone(),
        app.agent_wake_rx
            .take()
            .expect("agent_wake_rx present on fresh App"),
    );
    start_manager_status_worker(
        app.background_tx.clone(),
        app.profile,
        app.startup_at,
        app.versions.mem_cli.clone(),
    );
    start_embedding_backends_worker(
        api.clone(),
        app.project.clone(),
        app.background_tx.clone(),
        app.embeddings_tab_visible.clone(),
        app.embeddings_wake_rx
            .take()
            .expect("embeddings_wake_rx present on fresh App"),
    );
    terminal.draw(|frame| draw(frame, &app))?;
    app.request_refresh(&api, RefreshMode::Startup);
    app.request_activities(&api);
    app.request_llm_audit_status(&api);
    app.request_stream_connect(&api);
    let mut stream: Option<StreamSession> = None;
    let mut last_stream_connect_attempt = Instant::now();

    let mut last_draw = Instant::now();
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
                    app.handle_stream_disconnect(&error.to_string());
                    stream_failed = true;
                }
            }
        }
        if stream_failed {
            stream = None;
            last_stream_connect_attempt = Instant::now();
        }
        while let Ok(event) = background_rx.try_recv() {
            match event {
                BackgroundEvent::StreamConnectCompleted { result } => {
                    app.stream_connecting = false;
                    match result {
                        Ok(new_stream) => {
                            stream = Some(*new_stream);
                            app.status_message =
                                "Streaming updates enabled. Refreshing project data...".to_string();
                            app.needs_redraw = true;
                            app.request_refresh(&api, RefreshMode::Full);
                        }
                        Err(error) => {
                            app.status_message = format!(
                                "Streaming unavailable: {error}. Retrying live updates in the background."
                            );
                            app.needs_redraw = true;
                        }
                    }
                }
                BackgroundEvent::ProjectRefreshLoaded(result) => {
                    let loaded_memories = app.apply_project_refresh(*result);
                    if loaded_memories {
                        app.fetch_selected_detail(&api, stream.as_mut()).await;
                    }
                }
                other => app.apply_background_event(other),
            }
        }
        if should_attempt_stream_reconnect(
            stream.is_some(),
            app.stream_connecting,
            last_stream_connect_attempt,
        ) {
            last_stream_connect_attempt = Instant::now();
            app.request_stream_connect(&api);
        }
        if app.needs_redraw || last_draw.elapsed() >= Duration::from_secs(1) {
            terminal.draw(|frame| draw(frame, &app))?;
            app.needs_redraw = false;
            last_draw = Instant::now();
        }
        if event::poll(Duration::from_millis(500))? {
            match event::read()? {
                Event::Key(key) if should_quit(key, &app) => break,
                Event::Key(key) => {
                    if app.handle_key(key, &api, stream.as_mut()).await? {
                        break;
                    }
                    app.needs_redraw = true;
                }
                Event::Resize(_, _) => {
                    app.needs_redraw = true;
                }
                _ => {}
            }
        }
    }

    restore_terminal(terminal)
}

/// Fast cadence used while the Agents tab is visible.
const AGENT_POLL_ACTIVE: Duration = Duration::from_secs(5);
/// Slow cadence used when no tab displays agent_snapshot. Switching to the
/// Agents tab sends a wake signal so the user doesn't wait this long.
const AGENT_POLL_IDLE: Duration = Duration::from_secs(30);

fn start_agent_snapshot_worker(
    tx: mpsc::UnboundedSender<BackgroundEvent>,
    agents_tab_visible: Arc<AtomicBool>,
    wake_rx: std_mpsc::Receiver<()>,
) {
    std::thread::spawn(move || {
        let mut collector = mem_agenttop::AgentTop::new();
        loop {
            let snapshot = collector.collect_snapshot();
            if tx
                .send(BackgroundEvent::AgentsLoaded {
                    snapshot: Ok(snapshot),
                })
                .is_err()
            {
                break;
            }
            let interval = if agents_tab_visible.load(Ordering::Relaxed) {
                AGENT_POLL_ACTIVE
            } else {
                AGENT_POLL_IDLE
            };
            match wake_rx.recv_timeout(interval) {
                Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    });
}

const EMBEDDINGS_POLL_ACTIVE: Duration = Duration::from_secs(5);
const EMBEDDINGS_POLL_IDLE: Duration = Duration::from_secs(60);

fn start_embedding_backends_worker(
    api: ApiClient,
    project: String,
    tx: mpsc::UnboundedSender<BackgroundEvent>,
    embeddings_tab_visible: Arc<AtomicBool>,
    wake_rx: std_mpsc::Receiver<()>,
) {
    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(_) => return,
        };
        loop {
            let snapshot = runtime
                .block_on(api.list_embedding_backends(Some(&project)))
                .map_err(|err| err.to_string());
            if tx
                .send(BackgroundEvent::EmbeddingBackendsLoaded { snapshot })
                .is_err()
            {
                break;
            }
            let interval = if embeddings_tab_visible.load(Ordering::Relaxed) {
                EMBEDDINGS_POLL_ACTIVE
            } else {
                EMBEDDINGS_POLL_IDLE
            };
            match wake_rx.recv_timeout(interval) {
                Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    });
}

fn start_manager_status_worker(
    tx: mpsc::UnboundedSender<BackgroundEvent>,
    profile: Profile,
    startup_at: DateTime<Utc>,
    running_version: String,
) {
    std::thread::spawn(move || {
        loop {
            if tx
                .send(BackgroundEvent::ManagerStatusLoaded {
                    status: Some(load_manager_footer_status(profile)),
                    restart_notice: load_tui_restart_notice(startup_at, &running_version),
                })
                .is_err()
            {
                break;
            }
            std::thread::sleep(Duration::from_secs(10));
        }
    });
}

fn spawn_stream_connect(
    api: ApiClient,
    project: String,
    memory_id: Option<uuid::Uuid>,
    tx: mpsc::UnboundedSender<BackgroundEvent>,
) {
    tokio::spawn(async move {
        let result = tokio::time::timeout(STREAM_CONNECT_TIMEOUT, async {
            let mut stream = StreamSession::connect(&api).await?;
            subscribe_stream_selection(&mut stream, project, memory_id).await?;
            Ok::<_, anyhow::Error>(stream)
        })
        .await
        .map_err(|_| anyhow::anyhow!("stream connection timed out after 2s"))
        .and_then(|result| result)
        .map(Box::new)
        .map_err(|error| error.to_string());
        let _ = tx.send(BackgroundEvent::StreamConnectCompleted { result });
    });
}

async fn load_project_refresh(
    api: &ApiClient,
    project: String,
    repo_root: PathBuf,
    mode: RefreshMode,
) -> ProjectRefreshResult {
    let health_fut = api.health();
    let overview_fut = api.project_overview(&project);
    let memories_fut = api.project_memories(&project);
    let proposals_fut = api.replacement_proposals(&project);
    let (health, overview, memories, proposals) =
        tokio::join!(health_fut, overview_fut, memories_fut, proposals_fut);
    ProjectRefreshResult {
        mode,
        health: health.map_err(|error| error.to_string()),
        overview: overview.map_err(|error| error.to_string()),
        memories: memories.map_err(|error| error.to_string()),
        proposals: proposals.map_err(|error| error.to_string()),
        skill_inventory: project_skill_inventory(&repo_root, false),
    }
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
    /// History of the selected memory, loaded on demand via the `H`
    /// keystroke. When Some, the detail pane renders the version chain
    /// instead of the usual single-version detail.
    selected_history: Option<mem_api::MemoryHistoryResponse>,
    selected_index: usize,
    table_state: TableState,
    query_text: String,
    query_history: Vec<QueryHistoryEntry>,
    query_history_cursor: Option<usize>,
    query_response: Option<QueryResponse>,
    query_last_duration_ms: Option<u64>,
    query_roundtrip_timing: Option<QueryRoundtripTiming>,
    query_selected_detail: Option<MemoryEntryResponse>,
    query_selected_index: usize,
    query_table_state: TableState,
    query_loading: bool,
    query_started_at: Option<Instant>,
    query_pending_question: Option<String>,
    query_error: Option<String>,
    query_request_id: u64,
    query_detail_loading: bool,
    query_detail_request_id: u64,
    agent_snapshot: Option<AgentSnapshot>,
    agent_loading: bool,
    agent_error: Option<String>,
    agent_selected_index: usize,
    agent_table_state: TableState,
    agent_detail_scroll: u16,
    agent_initial_selection_done: bool,
    resume_response: Option<ResumeResponse>,
    resume_loading: bool,
    resume_loaded: bool,
    resume_error: Option<String>,
    resume_scroll: u16,
    activity_events: Vec<ActivityEntry>,
    activity_selected_index: usize,
    activity_table_state: TableState,
    activity_loading: bool,
    activity_error: Option<String>,
    activity_detail_scroll: u16,
    llm_audit_status: Option<LlmAuditStatusResponse>,
    llm_audit_loading: bool,
    llm_audit_toggling: bool,
    llm_audit_error: Option<String>,
    errors_selected_index: usize,
    errors_table_state: TableState,
    errors_detail_scroll: u16,
    up_to_speed_response: Option<UpToSpeedResponse>,
    up_to_speed_loading: bool,
    up_to_speed_error: Option<String>,
    memories_focus: MemoriesFocus,
    memory_detail_scroll: u16,
    project_scroll: u16,
    watcher_scroll: u16,
    help_open: bool,
    help_tab: TabKind,
    help_scroll: u16,
    replacement_policy: ReplacementPolicy,
    replacement_proposals: Vec<ReplacementProposalRecord>,
    replacement_selected_index: usize,
    review_table_state: TableState,
    versions: ToolVersions,
    skill_inventory: SkillInventoryReport,
    startup_at: DateTime<Utc>,
    ui_status: UiStatus,
    status_message: String,
    health_ok: bool,
    backend_connection_state: BackendConnectionState,
    service_role: Option<String>,
    service_health_state: Option<String>,
    service_database_state: Option<String>,
    manager_status: Option<ManagerFooterStatus>,
    restart_notice: Option<TuiRestartNotice>,
    stream_connecting: bool,
    relay_discovery_enabled: bool,
    profile: Profile,
    filters: Filters,
    input_mode: InputMode,
    startup_resume_autoselect_pending: bool,
    background_tx: mpsc::UnboundedSender<BackgroundEvent>,
    /// Signals the agent-snapshot worker whether the Agents tab is visible;
    /// the worker switches between a fast and slow polling cadence based on
    /// this flag.
    agents_tab_visible: Arc<AtomicBool>,
    /// Wakes the agent-snapshot worker immediately when the user switches
    /// into the Agents tab, so they don't have to wait out the idle
    /// cadence.
    agent_wake_tx: std_mpsc::Sender<()>,
    /// Receiver handed to the worker on startup. `Option` so `run()` can
    /// take it via `.take()` once without requiring the field to be `Clone`.
    agent_wake_rx: Option<std_mpsc::Receiver<()>>,
    // --- Embeddings tab state ---
    embedding_backends_snapshot: Option<mem_api::EmbeddingBackendsResponse>,
    embedding_backends_error: Option<String>,
    embeddings_selected_index: usize,
    embeddings_table_state: TableState,
    embeddings_tab_visible: Arc<AtomicBool>,
    embeddings_wake_tx: std_mpsc::Sender<()>,
    embeddings_wake_rx: Option<std_mpsc::Receiver<()>>,
    embeddings_toggle_message: Option<String>,
    embeddings_toggling: Option<String>,
    embeddings_creation_toggling: bool,
    embeddings_operation: Option<String>,
    needs_redraw: bool,
}

struct ToolVersions {
    mem_cli: String,
    mem_service: String,
    watch_manager: String,
    memory_watch: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BackendConnectionState {
    Connecting,
    Connected,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UiStatus {
    Loading,
    Busy,
    Ready,
    Restart,
    Error,
}

#[derive(Clone, Debug)]
struct ManagerFooterStatus {
    state: ManagerState,
    tracked_sessions: usize,
    warning_count: usize,
    mode: Option<ManagerMode>,
    runtime_mode: Option<String>,
    last_reconcile_reason: Option<String>,
    event_count: u64,
    fallback_scan_count: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ManagerState {
    Active,
    Installed,
    Off,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ManagerMode {
    Service,
    Foreground,
}

#[derive(Debug, Default, Deserialize)]
struct ManagerStateFile {
    #[serde(default)]
    mode: String,
    #[serde(default)]
    last_reconcile_reason: String,
    #[serde(default)]
    event_count: u64,
    #[serde(default)]
    fallback_scan_count: u64,
    #[serde(default)]
    sessions: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    warnings: Vec<String>,
}

enum ActivityEntry {
    Backend(Box<ActivityEvent>),
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
    Success(Box<QueryResponse>),
    Error(String),
}

#[derive(Clone)]
struct QueryHistoryEntry {
    question: String,
    response: Option<QueryResponse>,
    error: Option<String>,
    timing: Option<QueryRoundtripTiming>,
    initial_detail: Option<MemoryEntryResponse>,
    running: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct QueryRoundtripTiming {
    query_api_ms: u64,
    initial_detail_ms: Option<u64>,
    ui_ready_ms: u64,
}

#[derive(Clone)]
struct ProjectRefreshResult {
    mode: RefreshMode,
    health: Result<serde_json::Value, String>,
    overview: Result<ProjectOverviewResponse, String>,
    memories: Result<ProjectMemoriesResponse, String>,
    proposals: Result<ReplacementProposalListResponse, String>,
    skill_inventory: SkillInventoryReport,
}

enum BackgroundEvent {
    ProjectRefreshLoaded(Box<ProjectRefreshResult>),
    StreamConnectCompleted {
        result: Result<Box<StreamSession>, String>,
    },
    ResumeLoaded {
        response: Box<Result<ResumeResponse, String>>,
        checkpoint_present: bool,
        has_changes: bool,
        allow_autoselect: bool,
    },
    AgentsLoaded {
        snapshot: Result<AgentSnapshot, String>,
    },
    ManagerStatusLoaded {
        status: Option<ManagerFooterStatus>,
        restart_notice: Option<TuiRestartNotice>,
    },
    ActivitiesLoaded {
        response: Box<Result<mem_api::ActivityListResponse, String>>,
    },
    LlmAuditStatusLoaded {
        response: Result<LlmAuditStatusResponse, String>,
    },
    LlmAuditToggled {
        enabled: bool,
        response: Result<LlmAuditStatusResponse, String>,
    },
    UpToSpeedLoaded {
        response: Box<Result<UpToSpeedResponse, String>>,
    },
    EmbeddingBackendsLoaded {
        snapshot: Result<mem_api::EmbeddingBackendsResponse, String>,
    },
    EmbeddingBackendToggled {
        name: String,
        result: Result<mem_api::EmbeddingBackendsResponse, String>,
    },
    EmbeddingCreationToggled {
        name: String,
        enabled: bool,
        result: Result<mem_api::EmbeddingBackendsResponse, String>,
    },
    EmbeddingReembedCompleted {
        name: String,
        result: Result<(mem_api::ReembedResponse, mem_api::EmbeddingBackendsResponse), String>,
    },
    EmbeddingReindexCompleted {
        name: String,
        result: Result<(mem_api::ReindexResponse, mem_api::EmbeddingBackendsResponse), String>,
    },
    QueryCompleted {
        request_id: u64,
        request: QueryRequest,
        timing: QueryRoundtripTiming,
        response: Box<Result<QueryResponse, String>>,
        initial_detail: Box<Option<Result<MemoryEntryResponse, String>>>,
    },
    QueryDetailLoaded {
        request_id: u64,
        memory_id: String,
        detail: Box<Result<MemoryEntryResponse, String>>,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RefreshMode {
    Startup,
    Full,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MemoriesFocus {
    List,
    Detail,
}

impl App {
    fn new(
        project: String,
        repo_root: PathBuf,
        versions: ToolVersions,
        relay_discovery_enabled: bool,
        profile: Profile,
        background_tx: mpsc::UnboundedSender<BackgroundEvent>,
    ) -> Self {
        let mut table_state = TableState::default();
        table_state.select(Some(0));
        let mut query_table_state = TableState::default();
        query_table_state.select(Some(0));
        let mut agent_table_state = TableState::default();
        agent_table_state.select(Some(0));
        let mut activity_table_state = TableState::default();
        activity_table_state.select(Some(0));
        let mut errors_table_state = TableState::default();
        errors_table_state.select(Some(0));
        let (agent_wake_tx, agent_wake_rx) = std_mpsc::channel();
        let (embeddings_wake_tx, embeddings_wake_rx) = std_mpsc::channel();
        let mut embeddings_table_state = TableState::default();
        embeddings_table_state.select(Some(0));
        let mut review_table_state = TableState::default();
        review_table_state.select(Some(0));
        Self {
            project: project.clone(),
            repo_root: repo_root.clone(),
            active_tab: TabKind::Memories,
            all_memories: Vec::new(),
            filtered_memories: Vec::new(),
            total_memories: 0,
            overview: empty_overview(project),
            selected_detail: None,
            selected_history: None,
            selected_index: 0,
            table_state,
            query_text: String::new(),
            query_history: Vec::new(),
            query_history_cursor: None,
            query_response: None,
            query_last_duration_ms: None,
            query_roundtrip_timing: None,
            query_selected_detail: None,
            query_selected_index: 0,
            query_table_state,
            query_loading: false,
            query_started_at: None,
            query_pending_question: None,
            query_error: None,
            query_request_id: 0,
            query_detail_loading: false,
            query_detail_request_id: 0,
            agent_snapshot: None,
            agent_loading: true,
            agent_error: None,
            agent_selected_index: 0,
            agent_table_state,
            agent_detail_scroll: 0,
            agent_initial_selection_done: false,
            resume_response: None,
            resume_loading: false,
            resume_loaded: false,
            resume_error: None,
            resume_scroll: 0,
            activity_events: Vec::new(),
            activity_selected_index: 0,
            activity_table_state,
            activity_loading: false,
            activity_error: None,
            activity_detail_scroll: 0,
            llm_audit_status: None,
            llm_audit_loading: false,
            llm_audit_toggling: false,
            llm_audit_error: None,
            errors_selected_index: 0,
            errors_table_state,
            errors_detail_scroll: 0,
            up_to_speed_response: None,
            up_to_speed_loading: false,
            up_to_speed_error: None,
            memories_focus: MemoriesFocus::List,
            memory_detail_scroll: 0,
            project_scroll: 0,
            watcher_scroll: 0,
            help_open: false,
            help_tab: TabKind::Memories,
            help_scroll: 0,
            replacement_policy: load_repo_replacement_policy(&repo_root).unwrap_or_default(),
            replacement_proposals: Vec::new(),
            replacement_selected_index: 0,
            review_table_state,
            versions,
            skill_inventory: project_skill_inventory(&repo_root, false),
            startup_at: Utc::now(),
            ui_status: UiStatus::Loading,
            status_message: "Loading project data...".to_string(),
            health_ok: false,
            backend_connection_state: BackendConnectionState::Connecting,
            service_role: None,
            service_health_state: None,
            service_database_state: None,
            manager_status: None,
            restart_notice: None,
            stream_connecting: false,
            relay_discovery_enabled,
            profile,
            filters: Filters::default(),
            input_mode: InputMode::Normal,
            startup_resume_autoselect_pending: true,
            background_tx,
            agents_tab_visible: Arc::new(AtomicBool::new(false)),
            agent_wake_tx,
            agent_wake_rx: Some(agent_wake_rx),
            embedding_backends_snapshot: None,
            embedding_backends_error: None,
            embeddings_selected_index: 0,
            embeddings_table_state,
            embeddings_tab_visible: Arc::new(AtomicBool::new(false)),
            embeddings_wake_tx,
            embeddings_wake_rx: Some(embeddings_wake_rx),
            embeddings_toggle_message: None,
            embeddings_toggling: None,
            embeddings_creation_toggling: false,
            embeddings_operation: None,
            needs_redraw: true,
        }
    }

    fn set_active_tab(&mut self, tab: TabKind) {
        let became_agents = tab == TabKind::Agents && self.active_tab != TabKind::Agents;
        let became_embeddings =
            tab == TabKind::Embeddings && self.active_tab != TabKind::Embeddings;
        self.active_tab = tab;
        self.agents_tab_visible
            .store(tab == TabKind::Agents, Ordering::Relaxed);
        self.embeddings_tab_visible
            .store(tab == TabKind::Embeddings, Ordering::Relaxed);
        if became_agents {
            // Wake the worker so the newly-opened tab shows fresh data
            // rather than whatever the idle cadence last produced.
            let _ = self.agent_wake_tx.send(());
        }
        if became_embeddings {
            let _ = self.embeddings_wake_tx.send(());
        }
    }

    fn begin_refresh(&mut self, mode: RefreshMode) {
        self.needs_redraw = true;
        self.status_message = "Refreshing...".to_string();
        self.ui_status = if mode == RefreshMode::Startup {
            UiStatus::Loading
        } else {
            UiStatus::Busy
        };
        self.selected_detail = None;
        self.replacement_policy = load_repo_replacement_policy(&self.repo_root).unwrap_or_default();
    }

    fn request_refresh(&mut self, api: &ApiClient, mode: RefreshMode) {
        self.begin_refresh(mode);
        let checkpoint = if mode == RefreshMode::Startup {
            self.resume_checkpoint()
        } else {
            None
        };
        let api = api.clone();
        let project = self.project.clone();
        let repo_root = self.repo_root.clone();
        let tx = self.background_tx.clone();
        tokio::spawn(async move {
            let result = load_project_refresh(&api, project.clone(), repo_root, mode).await;
            let _ = tx.send(BackgroundEvent::ProjectRefreshLoaded(Box::new(result)));
            if mode == RefreshMode::Startup && checkpoint.is_some() {
                let request = ResumeRequest {
                    project: project.clone(),
                    checkpoint: checkpoint.clone(),
                    repo_root: None,
                    since: None,
                    include_llm_summary: false,
                    limit: 12,
                };
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
                    response: Box::new(response),
                    checkpoint_present: true,
                    has_changes,
                    allow_autoselect: true,
                });
            }
        });
    }

    fn request_stream_connect(&mut self, api: &ApiClient) {
        if self.stream_connecting {
            return;
        }
        self.stream_connecting = true;
        let memory_id = self
            .filtered_memories
            .get(self.selected_index)
            .map(|item| item.id);
        spawn_stream_connect(
            api.clone(),
            self.project.clone(),
            memory_id,
            self.background_tx.clone(),
        );
    }

    fn request_activities(&mut self, api: &ApiClient) {
        if self.activity_loading {
            return;
        }
        self.activity_loading = true;
        self.activity_error = None;
        self.needs_redraw = true;
        let api = api.clone();
        let project = self.project.clone();
        let tx = self.background_tx.clone();
        tokio::spawn(async move {
            let response = api
                .project_activities(&project, 100, None)
                .await
                .map_err(|error| error.to_string());
            let _ = tx.send(BackgroundEvent::ActivitiesLoaded {
                response: Box::new(response),
            });
        });
    }

    fn request_llm_audit_status(&mut self, api: &ApiClient) {
        if self.llm_audit_loading {
            return;
        }
        self.llm_audit_loading = true;
        self.llm_audit_error = None;
        self.needs_redraw = true;
        let api = api.clone();
        let tx = self.background_tx.clone();
        tokio::spawn(async move {
            let response = api
                .llm_audit_status()
                .await
                .map_err(|error| error.to_string());
            let _ = tx.send(BackgroundEvent::LlmAuditStatusLoaded { response });
        });
    }

    fn toggle_llm_audit(&mut self, api: &ApiClient) {
        if self.llm_audit_toggling {
            return;
        }
        let enabled = !self
            .llm_audit_status
            .as_ref()
            .map(|status| status.enabled)
            .unwrap_or(false);
        self.llm_audit_toggling = true;
        self.llm_audit_error = None;
        self.status_message = format!(
            "{} LLM audit/debug logging...",
            if enabled { "Enabling" } else { "Disabling" }
        );
        self.ui_status = UiStatus::Busy;
        self.needs_redraw = true;
        let api = api.clone();
        let tx = self.background_tx.clone();
        tokio::spawn(async move {
            let response = api
                .set_llm_audit_enabled(enabled)
                .await
                .map_err(|error| error.to_string());
            let _ = tx.send(BackgroundEvent::LlmAuditToggled { enabled, response });
        });
    }

    fn request_up_to_speed(&mut self, api: &ApiClient, include_llm_summary: bool) {
        if self.up_to_speed_loading {
            return;
        }
        self.up_to_speed_loading = true;
        self.up_to_speed_error = None;
        self.status_message = "Generating get-up-to-speed briefing...".to_string();
        self.ui_status = UiStatus::Busy;
        self.needs_redraw = true;
        let api = api.clone();
        let request = UpToSpeedRequest {
            project: self.project.clone(),
            include_llm_summary,
            limit: 20,
        };
        let tx = self.background_tx.clone();
        tokio::spawn(async move {
            let response = api
                .up_to_speed(&request)
                .await
                .map_err(|error| error.to_string());
            let _ = tx.send(BackgroundEvent::UpToSpeedLoaded {
                response: Box::new(response),
            });
        });
    }

    async fn refresh(&mut self, api: &ApiClient, mode: RefreshMode) {
        self.begin_refresh(mode);
        let result =
            load_project_refresh(api, self.project.clone(), self.repo_root.clone(), mode).await;
        let loaded_memories = self.apply_project_refresh(result);
        if loaded_memories {
            self.fetch_selected_detail(api, None).await;
        }
        if mode == RefreshMode::Startup {
            if self.resume_checkpoint().is_some() {
                self.request_resume_refresh(api, true);
            }
        } else if mode == RefreshMode::Full || self.active_tab == TabKind::Resume {
            self.request_resume_refresh(api, false);
        }
    }

    fn apply_project_refresh(&mut self, result: ProjectRefreshResult) -> bool {
        let mode = result.mode;
        let mut had_error = false;
        let mut loaded_memories = false;

        match result.health {
            Ok(health) => {
                self.health_ok = true;
                self.backend_connection_state = BackendConnectionState::Connected;
                if let Some(version) = health.get("version").and_then(|value| value.as_str()) {
                    self.versions.mem_service = version.to_string();
                }
                self.service_role = health
                    .get("role")
                    .and_then(|value| value.as_str())
                    .map(ToOwned::to_owned);
                self.service_health_state = health
                    .get("status")
                    .and_then(|value| value.as_str())
                    .map(ToOwned::to_owned);
                self.service_database_state = health
                    .get("database")
                    .and_then(|value| value.as_str())
                    .map(ToOwned::to_owned);
            }
            Err(_) => {
                had_error = true;
                self.mark_service_unavailable();
                self.status_message = if self.relay_discovery_enabled {
                    "Backend unavailable. Relay discovery is enabled; press r to retry after another Memory Layer backend becomes reachable.".to_string()
                } else {
                    "Backend unavailable. Press b to enable relay discovery fallback or r to retry."
                        .to_string()
                };
            }
        }

        self.skill_inventory = result.skill_inventory;

        match result.overview {
            Ok(overview) => self.overview = overview,
            Err(error) => {
                if self.health_ok {
                    had_error = true;
                    self.status_message = error.to_string();
                }
            }
        }

        match result.memories {
            Ok(ProjectMemoriesResponse {
                project: _,
                total,
                items,
            }) => {
                self.total_memories = total;
                self.all_memories = items;
                self.apply_filters();
                self.status_message = format!(
                    "Loaded {} visible memories ({} total).",
                    self.filtered_memories.len(),
                    self.total_memories
                );
                loaded_memories = true;
            }
            Err(error) => {
                had_error = true;
                self.all_memories.clear();
                self.filtered_memories.clear();
                self.total_memories = 0;
                self.selected_detail = None;
                self.table_state.select(None);
                if self.health_ok {
                    self.status_message = error.to_string();
                }
            }
        }

        match result.proposals {
            Ok(response) => {
                self.replacement_proposals = response.proposals;
                if self.replacement_proposals.is_empty() {
                    self.replacement_selected_index = 0;
                    self.review_table_state.select(None);
                } else {
                    self.replacement_selected_index = self
                        .replacement_selected_index
                        .min(self.replacement_proposals.len() - 1);
                    self.review_table_state
                        .select(Some(self.replacement_selected_index));
                }
            }
            Err(error) => {
                had_error = true;
                self.replacement_proposals.clear();
                self.replacement_selected_index = 0;
                self.review_table_state.select(None);
                self.status_message = error.to_string();
            }
        }

        self.ui_status = if had_error {
            UiStatus::Error
        } else if self.resume_loading || self.query_loading {
            UiStatus::Busy
        } else {
            UiStatus::Ready
        };

        if mode == RefreshMode::Startup && self.resume_checkpoint().is_some() && loaded_memories {
            self.status_message = format!(
                "{} Resume checkpoint available; open Resume to refresh.",
                self.status_message
            );
        }
        loaded_memories
    }

    fn resume_checkpoint(&self) -> Option<ResumeCheckpoint> {
        resume::load_checkpoint(&self.project, &self.repo_root)
            .ok()
            .flatten()
    }

    fn mark_service_unavailable(&mut self) {
        self.needs_redraw = true;
        self.health_ok = false;
        self.backend_connection_state = BackendConnectionState::Unavailable;
        self.service_role = None;
        self.service_health_state = None;
        self.service_database_state = None;
        self.overview.service_status = "error".to_string();
        self.overview.database_status = "unknown".to_string();
        self.overview.watchers = None;
    }

    fn handle_stream_disconnect(&mut self, error: &str) {
        self.status_message = format!(
            "Streaming disconnected: {error}. Retrying live updates; backend health is unchanged."
        );
        self.ui_status = if self.health_ok {
            UiStatus::Ready
        } else {
            UiStatus::Loading
        };
        self.needs_redraw = true;
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
            repo_root: Some(self.repo_root.display().to_string()),
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
                response: Box::new(response),
                checkpoint_present: checkpoint.is_some(),
                has_changes,
                allow_autoselect,
            });
        });
    }

    fn apply_background_event(&mut self, event: BackgroundEvent) {
        self.needs_redraw = true;
        match event {
            BackgroundEvent::ProjectRefreshLoaded(result) => {
                self.apply_project_refresh(*result);
            }
            BackgroundEvent::StreamConnectCompleted { result } => {
                self.stream_connecting = false;
                if let Err(error) = result {
                    self.status_message = format!(
                        "Streaming unavailable: {error}. Retrying live updates in the background."
                    );
                }
            }
            BackgroundEvent::ResumeLoaded {
                response,
                checkpoint_present,
                has_changes,
                allow_autoselect,
            } => {
                self.resume_loading = false;
                match *response {
                    Ok(response) => {
                        self.resume_response = Some(response);
                        self.resume_loaded = true;
                        self.resume_error = None;
                        if allow_autoselect
                            && self.startup_resume_autoselect_pending
                            && checkpoint_present
                            && has_changes
                        {
                            self.set_active_tab(TabKind::Resume);
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
            BackgroundEvent::AgentsLoaded { snapshot } => match snapshot {
                Ok(snapshot) => {
                    self.agent_loading = false;
                    self.agent_error = None;
                    self.agent_snapshot = Some(snapshot);
                    let session_count = self
                        .agent_snapshot
                        .as_ref()
                        .map(|snapshot| snapshot.sessions.len())
                        .unwrap_or(0);
                    if session_count == 0 {
                        self.agent_selected_index = 0;
                        self.agent_table_state.select(None);
                    } else {
                        if !self.agent_initial_selection_done {
                            self.agent_selected_index =
                                self.agent_snapshot
                                    .as_ref()
                                    .and_then(|snapshot| {
                                        snapshot.sessions.iter().position(|session| {
                                            session.project_name == self.project
                                        })
                                    })
                                    .unwrap_or(0);
                            self.agent_initial_selection_done = true;
                        } else {
                            self.agent_selected_index = self
                                .agent_selected_index
                                .min(session_count.saturating_sub(1));
                        }
                        self.agent_table_state
                            .select(Some(self.agent_selected_index));
                    }
                }
                Err(error) => {
                    self.agent_loading = false;
                    self.agent_error = Some(error);
                }
            },
            BackgroundEvent::ManagerStatusLoaded {
                status,
                restart_notice,
            } => {
                self.manager_status = status;
                let had_restart_notice = self.restart_notice.is_some();
                self.restart_notice = restart_notice;
                if let Some(notice) = &self.restart_notice {
                    self.ui_status = UiStatus::Restart;
                    self.status_message = format!(
                        "Memory Layer was updated to v{}; restart the TUI to load the installed binary. Marker: {}",
                        notice.version,
                        notice.marker_path.display()
                    );
                } else if had_restart_notice && matches!(self.ui_status, UiStatus::Restart) {
                    self.ui_status = UiStatus::Ready;
                    self.status_message = "TUI restart marker cleared.".to_string();
                }
            }
            BackgroundEvent::ActivitiesLoaded { response } => {
                self.activity_loading = false;
                match *response {
                    Ok(response) => {
                        self.activity_error = None;
                        self.activity_events = response
                            .items
                            .into_iter()
                            .map(|event| ActivityEntry::Backend(Box::new(event)))
                            .collect();
                        self.finish_activity_insert();
                        self.status_message = format!(
                            "Loaded {} persisted activity event(s).",
                            self.activity_events.len()
                        );
                    }
                    Err(error) => {
                        self.activity_error = Some(error.clone());
                        self.status_message = format!("Activities unavailable: {error}");
                    }
                }
            }
            BackgroundEvent::LlmAuditStatusLoaded { response } => {
                self.llm_audit_loading = false;
                match response {
                    Ok(status) => {
                        self.llm_audit_error = None;
                        self.llm_audit_status = Some(status);
                    }
                    Err(error) => {
                        self.llm_audit_error = Some(error.clone());
                        self.status_message = format!("LLM audit status unavailable: {error}");
                    }
                }
            }
            BackgroundEvent::LlmAuditToggled { enabled, response } => {
                self.llm_audit_toggling = false;
                match response {
                    Ok(status) => {
                        self.llm_audit_error = None;
                        self.llm_audit_status = Some(status);
                        self.status_message = format!(
                            "LLM audit/debug logging {}.",
                            if enabled { "enabled" } else { "disabled" }
                        );
                        self.ui_status = UiStatus::Ready;
                    }
                    Err(error) => {
                        self.llm_audit_error = Some(error.clone());
                        self.status_message = format!("LLM audit toggle failed: {error}");
                        self.ui_status = UiStatus::Error;
                    }
                }
            }
            BackgroundEvent::UpToSpeedLoaded { response } => {
                self.up_to_speed_loading = false;
                match *response {
                    Ok(response) => {
                        self.up_to_speed_error = None;
                        self.up_to_speed_response = Some(response);
                        self.status_message = "Get-up-to-speed briefing generated.".to_string();
                        self.ui_status = UiStatus::Ready;
                    }
                    Err(error) => {
                        self.up_to_speed_error = Some(error.clone());
                        self.status_message = format!("Get-up-to-speed failed: {error}");
                        self.ui_status = UiStatus::Error;
                    }
                }
            }
            BackgroundEvent::EmbeddingBackendsLoaded { snapshot } => match snapshot {
                Ok(snapshot) => {
                    self.embedding_backends_error = None;
                    let selected_index = active_embedding_backend_index(&snapshot).or_else(|| {
                        clamped_embedding_backend_index(self.embeddings_selected_index, &snapshot)
                    });
                    self.embedding_backends_snapshot = Some(snapshot);
                    if let Some(index) = selected_index {
                        self.embeddings_selected_index = index;
                        self.embeddings_table_state
                            .select(Some(self.embeddings_selected_index));
                    } else {
                        self.embeddings_selected_index = 0;
                        self.embeddings_table_state.select(None);
                    }
                }
                Err(error) => {
                    self.embedding_backends_error = Some(error);
                }
            },
            BackgroundEvent::EmbeddingBackendToggled { name, result } => {
                self.embeddings_toggling = None;
                match result {
                    Ok(snapshot) => {
                        self.embedding_backends_error = None;
                        self.embeddings_toggle_message =
                            if snapshot.active.as_deref() == Some(name.as_str()) {
                                Some(format!("Activated {name}"))
                            } else {
                                Some("Embeddings off".to_string())
                            };
                        let selected_index =
                            active_embedding_backend_index(&snapshot).or_else(|| {
                                clamped_embedding_backend_index(
                                    self.embeddings_selected_index,
                                    &snapshot,
                                )
                            });
                        self.embedding_backends_snapshot = Some(snapshot);
                        if let Some(index) = selected_index {
                            self.embeddings_selected_index = index;
                            self.embeddings_table_state
                                .select(Some(self.embeddings_selected_index));
                        } else {
                            self.embeddings_selected_index = 0;
                            self.embeddings_table_state.select(None);
                        }
                    }
                    Err(error) => {
                        self.embeddings_toggle_message = Some(format!("Toggle failed: {error}"));
                    }
                }
            }
            BackgroundEvent::EmbeddingCreationToggled {
                name,
                enabled,
                result,
            } => {
                self.embeddings_creation_toggling = false;
                match result {
                    Ok(snapshot) => {
                        self.embedding_backends_error = None;
                        self.embeddings_toggle_message = Some(format!(
                            "Automatic embedding creation {} for {name}",
                            if enabled { "on" } else { "off" },
                        ));
                        let selected_index =
                            active_embedding_backend_index(&snapshot).or_else(|| {
                                clamped_embedding_backend_index(
                                    self.embeddings_selected_index,
                                    &snapshot,
                                )
                            });
                        self.embedding_backends_snapshot = Some(snapshot);
                        if let Some(index) = selected_index {
                            self.embeddings_selected_index = index;
                            self.embeddings_table_state
                                .select(Some(self.embeddings_selected_index));
                        } else {
                            self.embeddings_selected_index = 0;
                            self.embeddings_table_state.select(None);
                        }
                    }
                    Err(error) => {
                        self.embeddings_toggle_message =
                            Some(format!("Creation toggle failed: {error}"));
                    }
                }
            }
            BackgroundEvent::EmbeddingReembedCompleted { name, result } => {
                self.embeddings_operation = None;
                match result {
                    Ok((response, snapshot)) => {
                        self.embedding_backends_error = None;
                        self.embeddings_toggle_message = Some(format!(
                            "Created {} chunk embedding(s) for {name}",
                            response.reembedded_chunks
                        ));
                        let selected_index = embedding_backend_index_by_name(&snapshot, &name)
                            .or_else(|| {
                                clamped_embedding_backend_index(
                                    self.embeddings_selected_index,
                                    &snapshot,
                                )
                            });
                        self.embedding_backends_snapshot = Some(snapshot);
                        if let Some(index) = selected_index {
                            self.embeddings_selected_index = index;
                            self.embeddings_table_state
                                .select(Some(self.embeddings_selected_index));
                        } else {
                            self.embeddings_selected_index = 0;
                            self.embeddings_table_state.select(None);
                        }
                    }
                    Err(error) => {
                        self.embeddings_toggle_message =
                            Some(format!("Embedding creation failed for {name}: {error}"));
                    }
                }
            }
            BackgroundEvent::EmbeddingReindexCompleted { name, result } => {
                self.embeddings_operation = None;
                match result {
                    Ok((response, snapshot)) => {
                        self.embedding_backends_error = None;
                        let target = if name == "all backends" {
                            "all backends"
                        } else {
                            name.as_str()
                        };
                        self.embeddings_toggle_message = Some(format!(
                            "Reindexed {} memory entries for {target}",
                            response.reindexed_entries
                        ));
                        let selected_index = embedding_backend_index_by_name(&snapshot, &name)
                            .or_else(|| {
                                clamped_embedding_backend_index(
                                    self.embeddings_selected_index,
                                    &snapshot,
                                )
                            });
                        self.embedding_backends_snapshot = Some(snapshot);
                        if let Some(index) = selected_index {
                            self.embeddings_selected_index = index;
                            self.embeddings_table_state
                                .select(Some(self.embeddings_selected_index));
                        } else {
                            self.embeddings_selected_index = 0;
                            self.embeddings_table_state.select(None);
                        }
                    }
                    Err(error) => {
                        self.embeddings_toggle_message =
                            Some(format!("Reindex failed for {name}: {error}"));
                    }
                }
            }
            BackgroundEvent::QueryCompleted {
                request_id,
                request,
                timing,
                response,
                initial_detail,
            } => {
                self.apply_query_completed(request_id, request, timing, *response, *initial_detail)
            }
            BackgroundEvent::QueryDetailLoaded {
                request_id,
                memory_id,
                detail,
            } => self.apply_query_detail_loaded(request_id, memory_id, *detail),
        }
    }

    fn move_agent_selection(&mut self, delta: isize) {
        let Some(snapshot) = &self.agent_snapshot else {
            self.agent_selected_index = 0;
            self.agent_table_state.select(None);
            return;
        };
        let len = snapshot.sessions.len();
        if len == 0 {
            self.agent_selected_index = 0;
            self.agent_table_state.select(None);
            return;
        }
        let next = (self.agent_selected_index as isize + delta).clamp(0, len as isize - 1);
        self.agent_selected_index = next as usize;
        self.agent_table_state
            .select(Some(self.agent_selected_index));
    }

    fn move_embeddings_selection(&mut self, delta: isize) {
        let len = self
            .embedding_backends_snapshot
            .as_ref()
            .map(|s| s.backends.len())
            .unwrap_or(0);
        if len == 0 {
            self.embeddings_selected_index = 0;
            self.embeddings_table_state.select(None);
            return;
        }
        // Cyclic wrap so j/k loops within the list.
        let cur = self.embeddings_selected_index as isize;
        let next = ((cur + delta) % len as isize + len as isize) % len as isize;
        self.embeddings_selected_index = next as usize;
        self.embeddings_table_state
            .select(Some(self.embeddings_selected_index));
    }

    fn selected_embedding_backend_name(&self) -> Option<String> {
        self.embedding_backends_snapshot
            .as_ref()
            .and_then(|snapshot| {
                snapshot
                    .backends
                    .get(self.embeddings_selected_index)
                    .map(|b| b.name.clone())
            })
    }

    fn selected_embedding_backend_is_active(&self) -> bool {
        self.embedding_backends_snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.backends.get(self.embeddings_selected_index))
            .is_some_and(|backend| backend.active)
    }

    fn selected_embedding_backend_create_enabled(&self) -> Option<bool> {
        self.embedding_backends_snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.backends.get(self.embeddings_selected_index))
            .map(|backend| backend.create_enabled)
    }

    fn scroll_agent_detail(&mut self, delta: i16) {
        self.agent_detail_scroll = self.agent_detail_scroll.saturating_add_signed(delta);
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

        if key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL {
            return Ok(true);
        }

        if self.help_open {
            self.handle_help_key(key);
            return Ok(false);
        }

        match key.code {
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') if key.modifiers.is_empty() => {
                self.set_active_tab(self.active_tab.next());
                if self.active_tab == TabKind::Resume && !self.resume_loaded {
                    self.request_resume_refresh(api, false);
                }
            }
            KeyCode::BackTab
                if key.modifiers == KeyModifiers::SHIFT || key.modifiers.is_empty() =>
            {
                self.set_active_tab(self.active_tab.prev());
                if self.active_tab == TabKind::Resume && !self.resume_loaded {
                    self.request_resume_refresh(api, false);
                }
            }
            KeyCode::Left if key.modifiers.is_empty() => {
                self.set_active_tab(self.active_tab.prev());
                if self.active_tab == TabKind::Resume && !self.resume_loaded {
                    self.request_resume_refresh(api, false);
                }
            }
            KeyCode::Char('h') if key.modifiers.is_empty() => {
                self.open_help_for_active_tab();
            }
            KeyCode::Char('r')
                if key.modifiers.is_empty()
                    && self.active_tab != TabKind::Activity
                    && self.active_tab != TabKind::Errors
                    && self.active_tab != TabKind::Embeddings =>
            {
                self.refresh(api, RefreshMode::Full).await
            }
            KeyCode::Down | KeyCode::Char('j') if self.active_tab == TabKind::Resume => {
                self.scroll_resume(1);
            }
            KeyCode::Up | KeyCode::Char('k') if self.active_tab == TabKind::Resume => {
                self.scroll_resume(-1);
            }
            KeyCode::Char('b')
                if key.modifiers.is_empty() && !self.health_ok && !self.relay_discovery_enabled =>
            {
                self.status_message =
                    "Enabling relay discovery fallback and restarting backend...".to_string();
                match enable_relay_discovery_and_restart_backend().await {
                    Ok(message) => {
                        self.relay_discovery_enabled = true;
                        self.status_message = message;
                        self.refresh(api, RefreshMode::Full).await;
                    }
                    Err(error) => {
                        self.status_message = error.to_string();
                    }
                }
            }
            KeyCode::Down | KeyCode::Char('j') if self.active_tab == TabKind::Memories => {
                if self.memories_focus == MemoriesFocus::Detail {
                    self.scroll_memory_detail(1);
                } else {
                    self.move_selection(1, api, stream).await;
                }
            }
            KeyCode::Up | KeyCode::Char('k') if self.active_tab == TabKind::Memories => {
                if self.memories_focus == MemoriesFocus::Detail {
                    self.scroll_memory_detail(-1);
                } else {
                    self.move_selection(-1, api, stream).await;
                }
            }
            KeyCode::Down | KeyCode::Char('j') if self.active_tab == TabKind::Agents => {
                self.move_agent_selection(1);
            }
            KeyCode::Up | KeyCode::Char('k') if self.active_tab == TabKind::Agents => {
                self.move_agent_selection(-1);
            }
            KeyCode::Down | KeyCode::Char('j') if self.active_tab == TabKind::Query => {
                self.move_query_selection(1, api);
            }
            KeyCode::Up | KeyCode::Char('k') if self.active_tab == TabKind::Query => {
                self.move_query_selection(-1, api);
            }
            KeyCode::Down | KeyCode::Char('j') if self.active_tab == TabKind::Activity => {
                self.move_activity_selection(1);
            }
            KeyCode::Up | KeyCode::Char('k') if self.active_tab == TabKind::Activity => {
                self.move_activity_selection(-1);
            }
            KeyCode::Char('r') if self.active_tab == TabKind::Activity => {
                self.request_activities(api);
                self.request_llm_audit_status(api);
            }
            KeyCode::Down | KeyCode::Char('j') if self.active_tab == TabKind::Errors => {
                self.move_error_selection(1);
            }
            KeyCode::Up | KeyCode::Char('k') if self.active_tab == TabKind::Errors => {
                self.move_error_selection(-1);
            }
            KeyCode::Char('r') if self.active_tab == TabKind::Errors => {
                self.request_activities(api);
            }
            KeyCode::PageDown if self.active_tab == TabKind::Errors => {
                self.errors_detail_scroll = self.errors_detail_scroll.saturating_add(8);
            }
            KeyCode::PageUp if self.active_tab == TabKind::Errors => {
                self.errors_detail_scroll = self.errors_detail_scroll.saturating_sub(8);
            }
            KeyCode::Home if self.active_tab == TabKind::Errors => {
                self.errors_detail_scroll = 0;
            }
            KeyCode::Char('g') if self.active_tab == TabKind::Activity => {
                self.request_up_to_speed(api, false);
            }
            KeyCode::Char('L')
                if self.active_tab == TabKind::Activity && key.modifiers == KeyModifiers::SHIFT =>
            {
                self.request_up_to_speed(api, true);
            }
            KeyCode::Char('A')
                if self.active_tab == TabKind::Activity && key.modifiers == KeyModifiers::SHIFT =>
            {
                self.toggle_llm_audit(api);
            }
            KeyCode::PageDown if self.active_tab == TabKind::Activity => {
                self.activity_detail_scroll = self.activity_detail_scroll.saturating_add(8);
            }
            KeyCode::PageUp if self.active_tab == TabKind::Activity => {
                self.activity_detail_scroll = self.activity_detail_scroll.saturating_sub(8);
            }
            KeyCode::Home if self.active_tab == TabKind::Activity => {
                self.activity_detail_scroll = 0;
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
            KeyCode::Down | KeyCode::Char('j') if self.active_tab == TabKind::Embeddings => {
                self.move_embeddings_selection(1);
            }
            KeyCode::Up | KeyCode::Char('k') if self.active_tab == TabKind::Embeddings => {
                self.move_embeddings_selection(-1);
            }
            KeyCode::Enter if self.active_tab == TabKind::Embeddings => {
                if let Some(name) = self.selected_embedding_backend_name() {
                    let deactivate = self.selected_embedding_backend_is_active();
                    self.embeddings_toggling = Some(if deactivate {
                        format!("turning off {name}")
                    } else {
                        name.clone()
                    });
                    self.embeddings_toggle_message = None;
                    let tx = self.background_tx.clone();
                    let api = api.clone();
                    tokio::spawn(async move {
                        let result = if deactivate {
                            api.deactivate_embedding_backend().await
                        } else {
                            api.activate_embedding_backend(&name).await
                        }
                        .map_err(|err| err.to_string());
                        let _ = tx.send(BackgroundEvent::EmbeddingBackendToggled { name, result });
                    });
                }
            }
            KeyCode::Char('c') if self.active_tab == TabKind::Embeddings => {
                if let Some(name) = self.selected_embedding_backend_name()
                    && let Some(current) = self.selected_embedding_backend_create_enabled()
                {
                    let enabled = !current;
                    self.embeddings_creation_toggling = true;
                    self.embeddings_toggle_message = None;
                    let tx = self.background_tx.clone();
                    let api = api.clone();
                    tokio::spawn(async move {
                        let result = api
                            .set_embedding_creation_enabled(&name, enabled)
                            .await
                            .map_err(|err| err.to_string());
                        let _ = tx.send(BackgroundEvent::EmbeddingCreationToggled {
                            name,
                            enabled,
                            result,
                        });
                    });
                }
            }
            KeyCode::Char('e') if self.active_tab == TabKind::Embeddings => {
                if self.embeddings_operation.is_none()
                    && let Some(name) = self.selected_embedding_backend_name()
                {
                    self.embeddings_operation = Some(format!("creating embeddings for {name}"));
                    self.embeddings_toggle_message = None;
                    let project = self.project.clone();
                    let tx = self.background_tx.clone();
                    let api = api.clone();
                    tokio::spawn(async move {
                        let result = async {
                            let response = api.reembed(&project, false, Some(&name)).await?;
                            let snapshot = api.list_embedding_backends(Some(&project)).await?;
                            anyhow::Ok((response, snapshot))
                        }
                        .await
                        .map_err(|err| err.to_string());
                        let _ =
                            tx.send(BackgroundEvent::EmbeddingReembedCompleted { name, result });
                    });
                }
            }
            KeyCode::Char('I')
                if self.active_tab == TabKind::Embeddings
                    && self.embeddings_operation.is_none() =>
            {
                let name = "all backends".to_string();
                self.embeddings_operation = Some("reindexing all backends".to_string());
                self.embeddings_toggle_message = None;
                let project = self.project.clone();
                let tx = self.background_tx.clone();
                let api = api.clone();
                tokio::spawn(async move {
                    let result = async {
                        let response = api.reindex(&project, false, None).await?;
                        let snapshot = api.list_embedding_backends(Some(&project)).await?;
                        anyhow::Ok((response, snapshot))
                    }
                    .await
                    .map_err(|err| err.to_string());
                    let _ = tx.send(BackgroundEvent::EmbeddingReindexCompleted { name, result });
                });
            }
            KeyCode::Char('r') if self.active_tab == TabKind::Embeddings => {
                let _ = self.embeddings_wake_tx.send(());
            }
            KeyCode::PageDown if self.active_tab == TabKind::Memories => {
                self.scroll_memory_detail(8);
            }
            KeyCode::PageUp if self.active_tab == TabKind::Memories => {
                self.scroll_memory_detail(-8);
            }
            KeyCode::Home if self.active_tab == TabKind::Memories => {
                self.scroll_memory_detail_home();
            }
            KeyCode::End if self.active_tab == TabKind::Memories => {
                self.scroll_memory_detail_end();
            }
            KeyCode::Enter if self.active_tab == TabKind::Memories => {
                self.toggle_memories_focus();
            }
            KeyCode::Esc if self.active_tab == TabKind::Memories => {
                self.focus_memories_list();
            }
            KeyCode::PageDown if self.active_tab == TabKind::Agents => {
                self.scroll_agent_detail(8);
            }
            KeyCode::PageUp if self.active_tab == TabKind::Agents => {
                self.scroll_agent_detail(-8);
            }
            KeyCode::Home if self.active_tab == TabKind::Agents => {
                self.agent_detail_scroll = 0;
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
                if self.active_tab == TabKind::Review && key.modifiers.is_empty() =>
            {
                self.cycle_replacement_policy().await?;
            }
            KeyCode::Char('[')
                if self.active_tab == TabKind::Review && key.modifiers.is_empty() =>
            {
                self.select_replacement_proposal(-1);
            }
            KeyCode::Char(']')
                if self.active_tab == TabKind::Review && key.modifiers.is_empty() =>
            {
                self.select_replacement_proposal(1);
            }
            KeyCode::Char('y')
                if self.active_tab == TabKind::Review && key.modifiers.is_empty() =>
            {
                self.approve_selected_replacement_proposal(api).await?;
            }
            KeyCode::Char('n')
                if self.active_tab == TabKind::Review && key.modifiers.is_empty() =>
            {
                self.reject_selected_replacement_proposal(api).await?;
            }
            KeyCode::Down | KeyCode::Char('j') if self.active_tab == TabKind::Review => {
                self.select_replacement_proposal(1);
            }
            KeyCode::Up | KeyCode::Char('k') if self.active_tab == TabKind::Review => {
                self.select_replacement_proposal(-1);
            }
            KeyCode::PageDown if self.active_tab == TabKind::Review => {
                self.select_replacement_proposal(8);
            }
            KeyCode::PageUp if self.active_tab == TabKind::Review => {
                self.select_replacement_proposal(-8);
            }
            KeyCode::Home if self.active_tab == TabKind::Review => {
                self.jump_replacement_proposal(0);
            }
            KeyCode::End if self.active_tab == TabKind::Review => {
                let len = self.replacement_proposals.len();
                self.jump_replacement_proposal(len.saturating_sub(1));
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
                self.set_active_tab(TabKind::Query);
                self.start_query_input();
            }
            KeyCode::Enter if self.active_tab == TabKind::Query => {
                self.start_query_input();
            }
            KeyCode::Char(ch)
                if self.active_tab == TabKind::Query
                    && (key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT) =>
            {
                let mut buffer = String::new();
                buffer.push(ch);
                self.input_mode = InputMode::Query(buffer);
                self.query_history_cursor = None;
                self.status_message =
                    "Type a question, Enter to run, Up/Down for history, Esc to cancel."
                        .to_string();
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
                let response = api
                    .curate(&self.project, self.replacement_policy, false)
                    .await?;
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
                let response = api.reindex(&self.project, false, None).await?;
                self.status_message =
                    format!("Reindexed {} memory entries.", response.reindexed_entries);
                self.refresh(api, RefreshMode::Full).await;
            }
            KeyCode::Char('e') if key.modifiers.is_empty() => {
                let response = api.reembed(&self.project, false, None).await?;
                self.status_message = format!(
                    "Materialized {} chunk embeddings for the active space.",
                    response.reembedded_chunks
                );
                self.refresh(api, RefreshMode::Full).await;
            }
            KeyCode::Char('a') if key.modifiers.is_empty() => {
                let response = api.archive_low_value(&self.project, false).await?;
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
            KeyCode::Char('H')
                if key.modifiers == KeyModifiers::SHIFT && self.active_tab == TabKind::Memories =>
            {
                self.toggle_selected_history(api).await;
            }
            _ => {}
        }
        Ok(false)
    }

    fn handle_help_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('h') | KeyCode::Esc if key.modifiers.is_empty() => self.close_help(),
            KeyCode::Down | KeyCode::Char('j') if key.modifiers.is_empty() => self.scroll_help(1),
            KeyCode::Up | KeyCode::Char('k') if key.modifiers.is_empty() => self.scroll_help(-1),
            KeyCode::PageDown => self.scroll_help(8),
            KeyCode::PageUp => self.scroll_help(-8),
            KeyCode::Home => self.help_scroll = 0,
            KeyCode::End => self.scroll_help_end(),
            _ => {}
        }
    }

    async fn toggle_selected_history(&mut self, api: &ApiClient) {
        // Second press hides the chain and returns to the single-version
        // detail view — cheap UX for users who don't want a dedicated
        // close key.
        if self.selected_history.is_some() {
            self.selected_history = None;
            self.memory_detail_scroll = 0;
            self.status_message = "Hid version history.".to_string();
            return;
        }
        let Some(item) = self.filtered_memories.get(self.selected_index) else {
            self.status_message = "No memory selected.".to_string();
            return;
        };
        self.status_message = "Loading version history...".to_string();
        match api.memory_history(&item.id.to_string()).await {
            Ok(history) => {
                self.status_message = format!(
                    "Loaded {} version(s) for canonical {}.",
                    history.versions.len(),
                    history.canonical_id
                );
                self.selected_history = Some(history);
                self.memory_detail_scroll = 0;
                self.memories_focus = MemoriesFocus::Detail;
            }
            Err(error) => {
                self.status_message = format!("History unavailable: {error}");
            }
        }
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
                if kind == TextInputKind::Query {
                    self.query_history_cursor = None;
                }
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
                        self.query_history_cursor = None;
                        self.run_query(api);
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
                if kind == TextInputKind::Query {
                    self.query_history_cursor = None;
                }
                self.input_mode = kind.wrap(buffer.clone());
            }
            KeyCode::Up if kind == TextInputKind::Query => {
                self.apply_query_history_delta(buffer, -1);
                self.input_mode = kind.wrap(buffer.clone());
            }
            KeyCode::Down if kind == TextInputKind::Query => {
                self.apply_query_history_delta(buffer, 1);
                self.input_mode = kind.wrap(buffer.clone());
            }
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                buffer.push(ch);
                if kind == TextInputKind::Query {
                    self.query_history_cursor = None;
                }
                self.input_mode = kind.wrap(buffer.clone());
            }
            _ => {
                self.input_mode = kind.wrap(buffer.clone());
            }
        }
        Ok(())
    }

    fn start_query_input(&mut self) {
        self.input_mode = InputMode::Query(String::new());
        self.query_history_cursor = None;
        self.status_message =
            "Type a question, Enter to run, Up/Down for history, Esc to cancel.".to_string();
    }

    fn remember_query_history_entry(&mut self) {
        let question = self.query_text.trim();
        if question.is_empty() {
            return;
        }
        if self
            .query_history
            .iter()
            .any(|previous| previous.question == question)
        {
            return;
        }
        self.query_history.push(QueryHistoryEntry {
            question: question.to_string(),
            response: None,
            error: None,
            timing: None,
            initial_detail: None,
            running: false,
        });
        if self.query_history.len() > 50 {
            self.query_history.remove(0);
        }
    }

    fn start_query_history_run(&mut self, question: &str) {
        self.query_text = question.to_string();
        self.remember_query_history_entry();
        if let Some(entry) = self
            .query_history
            .iter_mut()
            .find(|entry| entry.question == question)
        {
            entry.response = None;
            entry.error = None;
            entry.timing = None;
            entry.initial_detail = None;
            entry.running = true;
        }
    }

    fn update_query_history_success(
        &mut self,
        question: &str,
        response: &QueryResponse,
        timing: QueryRoundtripTiming,
        initial_detail: Option<&MemoryEntryResponse>,
    ) {
        if self
            .query_history
            .iter()
            .all(|previous| previous.question != question)
        {
            self.query_text = question.to_string();
            self.remember_query_history_entry();
        }
        if let Some(entry) = self
            .query_history
            .iter_mut()
            .find(|entry| entry.question == question)
        {
            entry.response = Some(response.clone());
            entry.error = None;
            entry.timing = Some(timing);
            entry.initial_detail = initial_detail.cloned();
            entry.running = false;
        }
    }

    fn update_query_history_error(
        &mut self,
        question: &str,
        error: &str,
        timing: QueryRoundtripTiming,
    ) {
        if self
            .query_history
            .iter()
            .all(|previous| previous.question != question)
        {
            self.query_text = question.to_string();
            self.remember_query_history_entry();
        }
        if let Some(entry) = self
            .query_history
            .iter_mut()
            .find(|entry| entry.question == question)
        {
            entry.response = None;
            entry.error = Some(error.to_string());
            entry.timing = Some(timing);
            entry.initial_detail = None;
            entry.running = false;
        }
    }

    fn apply_query_history_delta(&mut self, buffer: &mut String, delta: isize) {
        if self.query_history.is_empty() {
            self.status_message = "No previous queries in this TUI session.".to_string();
            return;
        }

        let last = self.query_history.len().saturating_sub(1);
        let next = match (self.query_history_cursor, delta) {
            (None, value) if value < 0 => Some(last),
            (None, value) if value > 0 => None,
            (Some(index), value) if value < 0 => Some(index.saturating_sub(1)),
            (Some(index), value) if value > 0 && index >= last => None,
            (Some(index), value) if value > 0 => Some(index + 1),
            (current, _) => current,
        };

        self.query_history_cursor = next;
        match next {
            Some(index) => {
                *buffer = self.query_history[index].question.clone();
                self.restore_query_history_entry(index);
            }
            None => {
                buffer.clear();
                self.clear_visible_query_state();
                self.status_message = "Returned to a new empty query.".to_string();
            }
        }
    }

    fn clear_visible_query_state(&mut self) {
        self.query_loading = false;
        self.query_started_at = None;
        self.query_pending_question = None;
        self.query_error = None;
        self.query_detail_loading = false;
        self.query_response = None;
        self.query_last_duration_ms = None;
        self.query_roundtrip_timing = None;
        self.query_selected_detail = None;
        self.query_selected_index = 0;
        self.query_table_state.select(None);
    }

    fn restore_query_history_entry(&mut self, index: usize) {
        let Some(entry) = self.query_history.get(index).cloned() else {
            self.clear_visible_query_state();
            self.status_message = "Query history item is unavailable.".to_string();
            return;
        };
        self.query_text = entry.question.clone();
        self.query_loading = entry.running;
        self.query_started_at = None;
        self.query_pending_question = entry.running.then_some(entry.question.clone());
        self.query_error = entry.error.clone();
        self.query_response = entry.response.clone();
        self.query_roundtrip_timing = entry.timing;
        self.query_last_duration_ms = entry.timing.map(|timing| timing.ui_ready_ms);
        self.query_selected_detail = if entry.response.is_some() {
            entry.initial_detail.clone()
        } else {
            None
        };
        self.query_detail_loading = false;
        self.query_selected_index = 0;
        if self.query_results().is_empty() {
            self.query_table_state.select(None);
        } else {
            self.query_table_state.select(Some(0));
        }
        let result_state = if entry.running {
            "still running"
        } else if entry.response.is_some() {
            "with cached results"
        } else if entry.error.is_some() {
            "with cached error"
        } else {
            "without cached results"
        };
        self.status_message = format!(
            "Loaded query history item {}/{} {result_state}.",
            index + 1,
            self.query_history.len()
        );
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
        self.selected_history = None;
        self.memory_detail_scroll = 0;
        self.memories_focus = MemoriesFocus::List;
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
            self.selected_history = None;
            self.memories_focus = MemoriesFocus::List;
        } else {
            self.selected_index = self.selected_index.min(self.filtered_memories.len() - 1);
            self.table_state.select(Some(self.selected_index));
        }
    }

    fn apply_stream_response(&mut self, response: StreamResponse) {
        self.needs_redraw = true;
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
                self.memories_focus = MemoriesFocus::List;
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

    fn run_query(&mut self, api: &ApiClient) {
        if self.clear_empty_query_if_needed() {
            return;
        }

        let question = self.query_text.trim();
        self.query_request_id = self.query_request_id.saturating_add(1);
        let request_id = self.query_request_id;
        let question = question.to_string();
        self.start_query_history_run(&question);
        self.query_loading = true;
        self.query_started_at = Some(Instant::now());
        self.query_pending_question = Some(question.clone());
        self.query_error = None;
        self.query_selected_detail = None;
        self.query_roundtrip_timing = None;
        self.query_detail_loading = false;
        self.status_message = format!("Searching \"{question}\"...");
        self.ui_status = UiStatus::Busy;
        let request = QueryRequest {
            project: self.project.clone(),
            query: question.clone(),
            filters: QueryFilters::default(),
            top_k: 8,
            min_confidence: None,
            history: false,
            retrieval_mode: None,
            answer_mode: None,
        };
        let api = api.clone();
        let tx = self.background_tx.clone();
        tokio::spawn(async move {
            let total_started = Instant::now();
            let api_started = Instant::now();
            let response = api.query(&request).await.map_err(|error| error.to_string());
            let query_api_ms = api_started.elapsed().as_millis() as u64;
            let mut initial_detail_ms = None;
            let initial_detail = match &response {
                Ok(response) => {
                    if let Some(result) = response.results.first() {
                        let detail_started = Instant::now();
                        let detail = api
                            .memory_detail(&result.memory_id.to_string())
                            .await
                            .map_err(|error| error.to_string());
                        initial_detail_ms = Some(detail_started.elapsed().as_millis() as u64);
                        Some(detail)
                    } else {
                        None
                    }
                }
                Err(_) => None,
            };
            let timing = QueryRoundtripTiming {
                query_api_ms,
                initial_detail_ms,
                ui_ready_ms: total_started.elapsed().as_millis() as u64,
            };
            let _ = tx.send(BackgroundEvent::QueryCompleted {
                request_id,
                request,
                timing,
                response: Box::new(response),
                initial_detail: Box::new(initial_detail),
            });
        });
    }

    fn clear_empty_query_if_needed(&mut self) -> bool {
        if self.query_text.trim().is_empty() {
            self.query_loading = false;
            self.query_started_at = None;
            self.query_pending_question = None;
            self.query_error = None;
            self.query_detail_loading = false;
            self.query_response = None;
            self.query_last_duration_ms = None;
            self.query_roundtrip_timing = None;
            self.query_selected_detail = None;
            self.query_selected_index = 0;
            self.query_table_state.select(None);
            self.status_message = "Enter a query before running search.".to_string();
            return true;
        }
        false
    }

    fn apply_query_completed(
        &mut self,
        request_id: u64,
        request: QueryRequest,
        timing: QueryRoundtripTiming,
        response: Result<QueryResponse, String>,
        initial_detail: Option<Result<MemoryEntryResponse, String>>,
    ) {
        if request_id != self.query_request_id {
            match response {
                Ok(response) => {
                    let initial_detail = initial_detail.and_then(Result::ok);
                    self.update_query_history_success(
                        &request.query,
                        &response,
                        timing,
                        initial_detail.as_ref(),
                    );
                }
                Err(error) => self.update_query_history_error(&request.query, &error, timing),
            }
            return;
        }
        self.query_loading = false;
        self.query_started_at = None;
        self.query_pending_question = None;
        self.query_detail_request_id = self.query_detail_request_id.saturating_add(1);
        self.query_detail_loading = false;
        match response {
            Ok(response) => {
                self.record_query_activity(
                    request.clone(),
                    timing.ui_ready_ms,
                    QueryLogOutcome::Success(Box::new(response.clone())),
                );
                self.resume_loaded = false;
                self.query_error = None;
                self.query_last_duration_ms = Some(timing.ui_ready_ms);
                self.query_roundtrip_timing = Some(timing);
                let response_for_history = response.clone();
                self.query_response = Some(response);
                self.query_selected_index = 0;
                let mut loaded_initial_detail = None;
                if self.query_results().is_empty() {
                    self.query_selected_detail = None;
                    self.query_table_state.select(None);
                } else {
                    self.query_table_state.select(Some(0));
                    match initial_detail {
                        Some(Ok(detail)) => {
                            loaded_initial_detail = Some(detail.clone());
                            self.query_selected_detail = Some(detail);
                        }
                        Some(Err(error)) => {
                            self.query_selected_detail = None;
                            self.status_message = format!("Query detail unavailable: {error}");
                        }
                        None => self.query_selected_detail = None,
                    }
                }
                self.update_query_history_success(
                    &request.query,
                    &response_for_history,
                    timing,
                    loaded_initial_detail.as_ref(),
                );
                if self.status_message.starts_with("Query detail unavailable:") {
                    self.status_message = format!(
                        "{} Query returned {} memories in {} ms.",
                        self.status_message,
                        self.query_results().len(),
                        timing.ui_ready_ms
                    );
                } else {
                    self.status_message = format!(
                        "Query returned {} memories in {} ms.",
                        self.query_results().len(),
                        timing.ui_ready_ms
                    );
                }
                self.ui_status = UiStatus::Ready;
            }
            Err(error) => {
                self.record_query_activity(
                    request.clone(),
                    timing.ui_ready_ms,
                    QueryLogOutcome::Error(error.to_string()),
                );
                self.resume_loaded = false;
                self.query_response = None;
                self.query_last_duration_ms = Some(timing.ui_ready_ms);
                self.query_roundtrip_timing = Some(timing);
                self.query_selected_detail = None;
                self.query_table_state.select(None);
                self.query_error = Some(error.clone());
                self.update_query_history_error(&request.query, &error, timing);
                self.status_message = format!("Query failed: {error}");
                self.ui_status = UiStatus::Error;
            }
        }
    }

    fn move_query_selection(&mut self, delta: isize, api: &ApiClient) {
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
            self.fetch_selected_query_detail(api);
        }
    }

    fn fetch_selected_query_detail(&mut self, api: &ApiClient) {
        self.query_selected_detail = None;
        self.query_detail_loading = false;
        if let Some(memory_id) = self
            .query_results()
            .get(self.query_selected_index)
            .map(|result| result.memory_id.to_string())
        {
            self.query_detail_request_id = self.query_detail_request_id.saturating_add(1);
            let request_id = self.query_detail_request_id;
            self.query_detail_loading = true;
            let api = api.clone();
            let tx = self.background_tx.clone();
            tokio::spawn(async move {
                let detail = api
                    .memory_detail(&memory_id)
                    .await
                    .map_err(|error| error.to_string());
                let _ = tx.send(BackgroundEvent::QueryDetailLoaded {
                    request_id,
                    memory_id,
                    detail: Box::new(detail),
                });
            });
        }
    }

    fn apply_query_detail_loaded(
        &mut self,
        request_id: u64,
        memory_id: String,
        detail: Result<MemoryEntryResponse, String>,
    ) {
        if request_id != self.query_detail_request_id {
            return;
        }
        let selected_memory_id = self
            .query_results()
            .get(self.query_selected_index)
            .map(|result| result.memory_id.to_string());
        if selected_memory_id.as_deref() != Some(memory_id.as_str()) {
            return;
        }
        self.query_detail_loading = false;
        match detail {
            Ok(detail) => self.query_selected_detail = Some(detail),
            Err(error) => {
                self.query_selected_detail = None;
                self.status_message = format!("Query detail unavailable: {error}");
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
        self.activity_events.retain(|entry| match entry {
            ActivityEntry::Backend(existing) => existing.id != event.id,
            ActivityEntry::Query(_) => true,
        });
        self.activity_events
            .insert(0, ActivityEntry::Backend(Box::new(event)));
        self.finish_activity_insert();
    }

    fn finish_activity_insert(&mut self) {
        if self.activity_events.len() > 200 {
            self.activity_events.truncate(200);
        }
        self.activity_selected_index = 0;
        if self.activity_events.is_empty() {
            self.activity_table_state.select(None);
        } else {
            self.activity_table_state.select(Some(0));
        }
        self.activity_detail_scroll = 0;
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

    fn move_error_selection(&mut self, delta: isize) {
        let len = collect_error_items(self).len();
        if len == 0 {
            self.errors_selected_index = 0;
            self.errors_table_state.select(None);
            return;
        }
        let next = (self.errors_selected_index as isize + delta)
            .clamp(0, len.saturating_sub(1) as isize) as usize;
        if next != self.errors_selected_index {
            self.errors_selected_index = next;
            self.errors_table_state.select(Some(next));
            self.errors_detail_scroll = 0;
        }
    }

    fn select_replacement_proposal(&mut self, delta: isize) {
        let len = self.replacement_proposals.len();
        if len == 0 {
            self.replacement_selected_index = 0;
            self.review_table_state.select(None);
            return;
        }
        // Cyclic wrap so j/k/[ ] loops within the list.
        let cur = self.replacement_selected_index as isize;
        let next = ((cur + delta) % len as isize + len as isize) % len as isize;
        self.replacement_selected_index = next as usize;
        self.review_table_state
            .select(Some(self.replacement_selected_index));
    }

    fn jump_replacement_proposal(&mut self, index: usize) {
        let len = self.replacement_proposals.len();
        if len == 0 {
            self.replacement_selected_index = 0;
            self.review_table_state.select(None);
            return;
        }
        self.replacement_selected_index = index.min(len - 1);
        self.review_table_state
            .select(Some(self.replacement_selected_index));
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
        self.run_query(api);
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

    fn open_help_for_active_tab(&mut self) {
        self.help_open = true;
        self.help_tab = self.active_tab;
        self.help_scroll = 0;
        self.status_message = format!(
            "Showing {} help. Press h or Esc to return.",
            self.help_tab.label()
        );
    }

    fn close_help(&mut self) {
        self.help_open = false;
        self.help_scroll = 0;
        self.status_message = "Help closed.".to_string();
    }

    fn scroll_help(&mut self, delta: i16) {
        let area = current_frame_area().unwrap_or_else(default_frame_area);
        self.scroll_help_in_area(delta, area);
    }

    fn scroll_help_in_area(&mut self, delta: i16, frame_area: Rect) {
        let max_scroll = help_max_scroll(self.help_tab, frame_area);
        self.help_scroll = if delta.is_negative() {
            self.help_scroll.saturating_sub(delta.unsigned_abs())
        } else {
            self.help_scroll
                .saturating_add(u16::try_from(delta).unwrap_or(0))
        }
        .min(max_scroll);
    }

    fn scroll_help_end(&mut self) {
        let area = current_frame_area().unwrap_or_else(default_frame_area);
        self.help_scroll = help_max_scroll(self.help_tab, area);
    }

    fn scroll_memory_detail(&mut self, delta: i16) {
        let area = current_frame_area().unwrap_or_else(default_frame_area);
        self.scroll_memory_detail_in_area(delta, area);
    }

    fn scroll_memory_detail_in_area(&mut self, delta: i16, frame_area: Rect) {
        let max_scroll = memory_detail_max_scroll(self, frame_area);
        self.memory_detail_scroll = if delta.is_negative() {
            self.memory_detail_scroll
                .saturating_sub(delta.unsigned_abs())
        } else {
            self.memory_detail_scroll
                .saturating_add(u16::try_from(delta).unwrap_or(0))
        }
        .min(max_scroll);
    }

    fn scroll_memory_detail_home(&mut self) {
        self.memory_detail_scroll = 0;
    }

    fn scroll_memory_detail_end(&mut self) {
        let area = current_frame_area().unwrap_or_else(default_frame_area);
        self.memory_detail_scroll = memory_detail_max_scroll(self, area);
    }

    fn toggle_memories_focus(&mut self) {
        self.memories_focus = match self.memories_focus {
            MemoriesFocus::List if self.selected_detail.is_some() => MemoriesFocus::Detail,
            MemoriesFocus::Detail => MemoriesFocus::List,
            MemoriesFocus::List => MemoriesFocus::List,
        };
    }

    fn focus_memories_list(&mut self) {
        self.memories_focus = MemoriesFocus::List;
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

async fn subscribe_stream_selection(
    stream: &mut StreamSession,
    project: String,
    memory_id: Option<uuid::Uuid>,
) -> Result<()> {
    stream
        .send(StreamRequest::SubscribeProject { project })
        .await?;
    if let Some(memory_id) = memory_id {
        stream
            .send(StreamRequest::SubscribeMemory { memory_id })
            .await?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TabKind {
    Memories,
    Agents,
    Query,
    Activity,
    Errors,
    Project,
    Review,
    Watchers,
    Embeddings,
    Resume,
}

const VISIBLE_TABS: [TabKind; 10] = [
    TabKind::Memories,
    TabKind::Agents,
    TabKind::Query,
    TabKind::Activity,
    TabKind::Errors,
    TabKind::Project,
    TabKind::Review,
    TabKind::Watchers,
    TabKind::Embeddings,
    TabKind::Resume,
];

impl TabKind {
    fn label(self) -> &'static str {
        match self {
            Self::Memories => "Memories",
            Self::Agents => "Agents",
            Self::Query => "Query",
            Self::Activity => "Activity",
            Self::Errors => "Errors",
            Self::Project => "Project",
            Self::Review => "Review",
            Self::Watchers => "Watchers",
            Self::Embeddings => "Embeddings",
            Self::Resume => "Resume",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Memories => Self::Agents,
            Self::Agents => Self::Query,
            Self::Query => Self::Activity,
            Self::Activity => Self::Errors,
            Self::Errors => Self::Project,
            Self::Project => Self::Review,
            Self::Review => Self::Watchers,
            Self::Watchers => Self::Embeddings,
            Self::Embeddings => Self::Resume,
            Self::Resume => Self::Memories,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Memories => Self::Resume,
            Self::Agents => Self::Memories,
            Self::Query => Self::Agents,
            Self::Activity => Self::Query,
            Self::Errors => Self::Activity,
            Self::Project => Self::Errors,
            Self::Review => Self::Project,
            Self::Watchers => Self::Review,
            Self::Embeddings => Self::Watchers,
            Self::Resume => Self::Embeddings,
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Memories => 0,
            Self::Agents => 1,
            Self::Query => 2,
            Self::Activity => 3,
            Self::Errors => 4,
            Self::Project => 5,
            Self::Review => 6,
            Self::Watchers => 7,
            Self::Embeddings => 8,
            Self::Resume => 9,
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

#[derive(Clone, Copy, PartialEq, Eq)]
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
    Documentation,
    Task,
    Plan,
    Implementation,
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
            Self::DomainFact => Self::Documentation,
            Self::Documentation => Self::Task,
            Self::Task => Self::Plan,
            Self::Plan => Self::Implementation,
            Self::Implementation => Self::All,
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
                | (Self::Documentation, MemoryType::Documentation)
                | (Self::Task, MemoryType::Task)
                | (Self::Plan, MemoryType::Plan)
                | (Self::Implementation, MemoryType::Implementation)
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
            Self::Documentation => "documentation",
            Self::Task => "task",
            Self::Plan => "plan",
            Self::Implementation => "implementation",
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

    let titles = VISIBLE_TABS
        .into_iter()
        .map(|tab| Line::from(Span::styled(tab.label(), Style::default().fg(Theme::TEXT))))
        .collect::<Vec<_>>();
    let title = match app.profile {
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

    let control_line = if app.help_open {
        Line::from(vec![
            accent_span("back "),
            Span::styled("h/Esc  ", Style::default().fg(Theme::TEXT)),
            accent_span("scroll "),
            Span::styled("j/k PgUp/PgDn  ", Style::default().fg(Theme::TEXT)),
            accent_span("jump "),
            Span::styled("Home/End  ", Style::default().fg(Theme::TEXT)),
            Span::styled(
                format!("showing {} help", app.help_tab.label()),
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
                    match app.memories_focus {
                        MemoriesFocus::List => "list",
                        MemoriesFocus::Detail => "detail",
                    },
                    Style::default()
                        .fg(match app.memories_focus {
                            MemoriesFocus::List => Theme::ACCENT,
                            MemoriesFocus::Detail => Theme::ACCENT_STRONG,
                        })
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    match app.memories_focus {
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
        .block(themed_block(if app.help_open {
            "Help Controls"
        } else {
            match &app.input_mode {
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

    if app.help_open {
        draw_help_tab(frame, app, chunks[2]);
    } else if app.health_ok {
        match app.active_tab {
            TabKind::Resume => draw_resume_tab(frame, app, chunks[2]),
            TabKind::Memories => draw_memories_tab(frame, app, chunks[2]),
            TabKind::Agents => draw_agents_tab(frame, app, chunks[2]),
            TabKind::Query => draw_query_tab(frame, app, chunks[2]),
            TabKind::Activity => draw_activity_tab(frame, app, chunks[2]),
            TabKind::Errors => draw_errors_tab(frame, app, chunks[2]),
            TabKind::Project => draw_project_tab(frame, app, chunks[2]),
            TabKind::Review => draw_review_tab(frame, app, chunks[2]),
            TabKind::Watchers => draw_watchers_tab(frame, app, chunks[2]),
            TabKind::Embeddings => draw_embeddings_tab(frame, app, chunks[2]),
        }
    } else {
        draw_backend_recovery(frame, app, chunks[2]);
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
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
        ])
        .split(area);

    frame.render_widget(
        Paragraph::new(component_status_line(
            "TUI",
            &app.versions.mem_cli,
            tui_status_label(app),
            tui_status_color(app),
            tui_status_detail(app),
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
            "Manager",
            &app.versions.watch_manager,
            manager_status_label(app),
            manager_status_color(app),
            manager_status_detail(app),
        ))
        .style(Style::default().bg(Theme::PANEL_ALT)),
        sections[2],
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
        sections[3],
    );

    frame.render_widget(
        Paragraph::new(component_status_line(
            "Skills",
            &app.skill_inventory.bundle_version,
            app.skill_inventory.status.label(),
            skill_bundle_status_color(app.skill_inventory.status),
            Some(app.skill_inventory.summary.clone()),
        ))
        .style(Style::default().bg(Theme::PANEL_ALT)),
        sections[4],
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

fn draw_backend_recovery(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    if app.backend_connection_state == BackendConnectionState::Connecting {
        draw_backend_connecting(frame, area);
        return;
    }

    let mut lines = vec![
        Line::from(Span::styled(
            "Memory Layer backend is unavailable.",
            Style::default()
                .fg(Theme::DANGER)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("The TUI could not reach /healthz on the configured backend."),
    ];
    if app.relay_discovery_enabled {
        lines.push(Line::from(
            "Relay discovery fallback is already enabled in shared config.",
        ));
        lines.push(Line::from(
            "If another Memory Layer backend is running on the local network, press r to retry.",
        ));
    } else {
        lines.push(Line::from(
            "Press b to enable relay discovery fallback and restart the shared backend.",
        ));
    }
    lines.push(Line::from("Press r to retry or q to quit."));

    let widget = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(themed_block("Backend Recovery"));
    frame.render_widget(widget, area);
}

fn draw_backend_connecting(frame: &mut ratatui::Frame<'_>, area: Rect) {
    let lines = vec![
        Line::from(Span::styled(
            "Connecting to Memory Layer backend...",
            Style::default()
                .fg(Theme::ACCENT_STRONG)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("The TUI is waiting for the first backend health check to complete."),
        Line::from(
            "This can take a moment while the service starts, runs migrations, or reconnects.",
        ),
        Line::from(""),
        Line::from("Press q to quit."),
    ];

    let widget = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(themed_block("Backend Connection"));
    frame.render_widget(widget, area);
}

fn draw_help_tab(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let max_scroll = help_max_scroll_in_area(app.help_tab, area);
    let scroll = app.help_scroll.min(max_scroll);
    let help = Paragraph::new(tab_help_lines(app.help_tab))
        .style(Style::default().bg(Theme::PANEL))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0))
        .block(themed_block(format!(
            "{} Help (scroll {}/{})",
            app.help_tab.label(),
            scroll,
            max_scroll
        )));
    frame.render_widget(help, area);
}

fn draw_memories_tab(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let chunks = split_memories_area(area);

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
            Constraint::Length(16),
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
    .block(themed_focus_block(
        format!(
            "Memories (showing {} / {})",
            app.filtered_memories.len(),
            app.total_memories
        ),
        app.memories_focus == MemoriesFocus::List,
    ));
    let mut state = app.table_state.clone();
    frame.render_stateful_widget(table, chunks[0], &mut state);

    let detail_text = build_memory_detail_lines(app);
    let detail_block = themed_focus_block(
        match app.memories_focus {
            MemoriesFocus::List => "Detail".to_string(),
            MemoriesFocus::Detail => "Detail Reader".to_string(),
        },
        app.memories_focus == MemoriesFocus::Detail,
    );
    let detail_inner = detail_block.inner(chunks[1]);
    let max_scroll = if detail_inner.width == 0 || detail_inner.height == 0 {
        0
    } else {
        wrapped_line_count(&detail_text, detail_inner.width)
            .saturating_sub(detail_inner.height as usize) as u16
    };
    let detail = Paragraph::new(detail_text)
        .style(Style::default().bg(Theme::PANEL))
        .scroll((app.memory_detail_scroll.min(max_scroll), 0))
        .wrap(Wrap { trim: false })
        .block(detail_block);
    frame.render_widget(detail, chunks[1]);
}

fn build_history_lines(history: &mem_api::MemoryHistoryResponse) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        label_span("Canonical: "),
        Span::styled(
            history.canonical_id.to_string(),
            Style::default().fg(Theme::TEXT),
        ),
        Span::raw("   "),
        label_span("Versions: "),
        Span::styled(
            history.versions.len().to_string(),
            Style::default().fg(Theme::ACCENT_STRONG),
        ),
    ]));
    lines.push(Line::from(Span::styled(
        "Press Shift+H again to return to the single-version detail.",
        Style::default().fg(Theme::MUTED),
    )));
    lines.push(Line::from(""));
    for version in &history.versions {
        let header_style = if version.is_tombstone {
            Style::default()
                .fg(Theme::DANGER)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(Theme::ACCENT_STRONG)
                .add_modifier(Modifier::BOLD)
        };
        let tombstone_suffix = if version.is_tombstone {
            "  [tombstone]"
        } else {
            ""
        };
        lines.push(Line::from(vec![
            Span::styled(format!("v{}", version.version_no), header_style),
            Span::raw("  "),
            memory_type_span(&version.memory_type),
            Span::raw("  "),
            status_span(match version.status {
                MemoryStatus::Active => "active",
                MemoryStatus::Archived => "archived",
            }),
            Span::styled(
                tombstone_suffix.to_string(),
                Style::default().fg(Theme::DANGER),
            ),
        ]));
        lines.push(Line::from(vec![
            label_span("id: "),
            Span::styled(version.id.to_string(), Style::default().fg(Theme::MUTED)),
            Span::raw("   "),
            label_span("updated: "),
            Span::styled(
                format_timestamp_medium(version.updated_at),
                Style::default().fg(Theme::MUTED),
            ),
        ]));
        if version.is_tombstone {
            lines.push(Line::from(Span::styled(
                "  (empty — memory was deleted at this point)",
                Style::default().fg(Theme::MUTED),
            )));
        } else {
            lines.push(Line::from(vec![
                label_span("summary: "),
                Span::styled(version.summary.clone(), Style::default().fg(Theme::TEXT)),
            ]));
            let preview: String = version.canonical_text.chars().take(320).collect();
            let ellipsis = if version.canonical_text.chars().count() > 320 {
                "..."
            } else {
                ""
            };
            lines.push(Line::from(Span::styled(
                format!("{preview}{ellipsis}"),
                Style::default().fg(Theme::TEXT),
            )));
        }
        lines.push(Line::from(""));
    }
    lines
}

fn build_memory_detail_lines(app: &App) -> Vec<Line<'static>> {
    if let Some(history) = &app.selected_history {
        return build_history_lines(history);
    }
    if let Some(detail) = &app.selected_detail {
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
            Line::from(vec![section_span("Embeddings")]),
        ];
        if detail.embedding_spaces.is_empty() {
            lines.push(Line::from(Span::styled(
                "No embeddings for this memory yet. Run Re-embed for this project to populate the active embedding space.",
                Style::default().fg(Theme::MUTED),
            )));
        } else {
            for space in &detail.embedding_spaces {
                let chunks_label = if space.chunk_count == 1 {
                    "1 chunk".to_string()
                } else {
                    format!("{} chunks", space.chunk_count)
                };
                let mut spans = vec![
                    Span::styled(space.provider.clone(), Style::default().fg(Theme::ACCENT)),
                    Span::raw(" · "),
                    Span::styled(space.model.clone(), Style::default().fg(Theme::TEXT)),
                    Span::raw(" · "),
                    Span::styled(chunks_label, Style::default().fg(Theme::TEXT)),
                ];
                if let Some(updated) = space.last_updated {
                    spans.push(Span::raw(" · "));
                    spans.push(Span::styled(
                        format!("updated {}", format_timestamp_medium(updated)),
                        Style::default().fg(Theme::MUTED),
                    ));
                }
                lines.push(Line::from(spans));
                if !embedding_base_url_is_default(&space.provider, &space.base_url) {
                    lines.push(Line::from(Span::styled(
                        format!("    {}", space.base_url),
                        Style::default().fg(Theme::MUTED),
                    )));
                }
            }
        }
        lines.push(Line::from(""));
        lines.push(Line::from(vec![section_span("Canonical Text")]));
        lines.extend(render_markdown_lines(&detail.canonical_text));
        lines.push(Line::from(""));
        lines.extend([
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
        ]);

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
    }
}

fn draw_agents_tab(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
        .split(area);

    if app.agent_loading && app.agent_snapshot.is_none() {
        frame.render_widget(
            Paragraph::new("Loading agent sessions...")
                .style(Style::default().fg(Theme::ACCENT).bg(Theme::PANEL_ALT))
                .block(themed_block("Agents")),
            area,
        );
        return;
    }

    if let Some(error) = &app.agent_error
        && app.agent_snapshot.is_none()
    {
        frame.render_widget(
            Paragraph::new(format!("Agents unavailable: {error}"))
                .style(Style::default().fg(Theme::WARNING).bg(Theme::PANEL_ALT))
                .wrap(Wrap { trim: false })
                .block(themed_block("Agents")),
            area,
        );
        return;
    }

    let Some(snapshot) = &app.agent_snapshot else {
        frame.render_widget(
            Paragraph::new("No agent data available yet.")
                .style(Style::default().fg(Theme::MUTED).bg(Theme::PANEL_ALT))
                .block(themed_block("Agents")),
            area,
        );
        return;
    };

    let header = Row::new(["Project", "Agent", "Status", "Tok", "Ctx", "Task"]).style(
        Style::default()
            .fg(Theme::ACCENT_STRONG)
            .bg(Theme::PANEL_ALT)
            .add_modifier(Modifier::BOLD),
    );
    let rows = snapshot.sessions.iter().map(agent_row);
    let table = Table::new(
        rows,
        [
            Constraint::Length(20),
            Constraint::Length(7),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(7),
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
        "Agents ({} sessions, {} orphan ports)",
        snapshot.sessions.len(),
        snapshot.orphan_ports.len()
    )));
    let mut state = app.agent_table_state.clone();
    frame.render_stateful_widget(table, chunks[0], &mut state);

    let detail = Paragraph::new(agent_detail_lines(app, snapshot))
        .scroll((app.agent_detail_scroll, 0))
        .wrap(Wrap { trim: false })
        .style(Style::default().bg(Theme::PANEL))
        .block(themed_block(format!(
            "Agent Detail (scroll {})",
            app.agent_detail_scroll
        )));
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
        metric_line(
            "Latest plan",
            Span::styled(latest_plan_display(app), Style::default().fg(Theme::TEXT)),
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
            "Skill bundle",
            Span::styled(
                format!(
                    "v{} {} ({})",
                    app.skill_inventory.bundle_version,
                    app.skill_inventory.status.label(),
                    app.skill_inventory.summary
                ),
                Style::default().fg(skill_bundle_status_color(app.skill_inventory.status)),
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
                    "{} / {} pending (see Review tab)",
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
        .constraints([Constraint::Min(8), Constraint::Length(7)])
        .split(chunks[2]);

    let bottom_top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
        .split(bottom[0]);

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

fn draw_review_tab(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(area);

    let pending = app.replacement_proposals.len();
    let selected_label = if pending == 0 {
        "—".to_string()
    } else {
        format!("{}/{}", app.replacement_selected_index + 1, pending)
    };
    let header = Paragraph::new(vec![
        Line::from(vec![
            label_span("Policy: "),
            Span::styled(
                app.replacement_policy.to_string(),
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

    if app.replacement_proposals.is_empty() {
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
        let mut state = app.review_table_state.clone();
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

fn review_detail_lines(app: &App) -> Vec<Line<'static>> {
    let Some(proposal) = app
        .replacement_proposals
        .get(app.replacement_selected_index)
    else {
        return vec![Line::from(Span::styled(
            "Select a proposal on the left to inspect it here.",
            Style::default().fg(Theme::MUTED),
        ))];
    };

    let mut lines = vec![
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
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        proposal.candidate_canonical_text.clone(),
        Style::default().fg(Theme::MUTED),
    )));
    lines
}

fn truncate_for_list(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
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
                "Use `memory watcher manager enable` on Linux, or `memory watcher enable --project <slug>` / `memory watcher run --project <slug>` for manual mode.",
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

fn draw_embeddings_tab(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(8)])
        .split(area);

    let snapshot = app.embedding_backends_snapshot.as_ref();
    let backends = snapshot.map(|s| s.backends.as_slice()).unwrap_or(&[]);
    let configured = backends.len();
    let ready = backends.iter().filter(|b| b.ready).count();
    let not_ready = configured.saturating_sub(ready);
    let active_display = snapshot
        .and_then(|s| s.active.clone())
        .unwrap_or_else(|| "(none)".to_string());
    let create_display = snapshot
        .and_then(|snapshot| snapshot.backends.get(app.embeddings_selected_index))
        .map(|backend| {
            format!(
                "{} for {}",
                if backend.create_enabled { "on" } else { "off" },
                backend.name
            )
        })
        .unwrap_or_else(|| "unknown".to_string());

    let message_line = if app.embeddings_creation_toggling {
        Line::from(vec![
            label_span("Status: "),
            Span::styled(
                "toggling automatic embedding creation...",
                Style::default().fg(Theme::ACCENT),
            ),
        ])
    } else if let Some(operation) = &app.embeddings_operation {
        Line::from(vec![
            label_span("Status: "),
            Span::styled(
                format!("{operation}..."),
                Style::default().fg(Theme::ACCENT),
            ),
        ])
    } else if let Some(toggling) = &app.embeddings_toggling {
        Line::from(vec![
            label_span("Status: "),
            Span::styled(
                format!("toggling {toggling}..."),
                Style::default().fg(Theme::ACCENT),
            ),
        ])
    } else if let Some(msg) = &app.embeddings_toggle_message {
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
    } else if let Some(err) = &app.embedding_backends_error {
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
        let body = if app.embedding_backends_snapshot.is_some() {
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
    let mut state = app.embeddings_table_state.clone();
    frame.render_stateful_widget(table, chunks[1], &mut state);
}

fn active_embedding_backend_index(snapshot: &mem_api::EmbeddingBackendsResponse) -> Option<usize> {
    snapshot.backends.iter().position(|backend| backend.active)
}

fn embedding_backend_index_by_name(
    snapshot: &mem_api::EmbeddingBackendsResponse,
    name: &str,
) -> Option<usize> {
    snapshot
        .backends
        .iter()
        .position(|backend| backend.name == name)
}

fn clamped_embedding_backend_index(
    current: usize,
    snapshot: &mem_api::EmbeddingBackendsResponse,
) -> Option<usize> {
    (!snapshot.backends.is_empty()).then(|| current.min(snapshot.backends.len().saturating_sub(1)))
}

fn draw_query_tab(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(13),
            Constraint::Min(12),
        ])
        .split(area);
    let query_editing = matches!(app.input_mode, InputMode::Query(_));
    let query_input_area = chunks[0];
    let query_inner_width = query_input_area.width.saturating_sub(2);
    let query_input = query_input_display(&current_query_display(app), query_inner_width);
    let query_title = if app.query_loading {
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
            query_editing || app.query_loading,
        ));
    frame.render_widget(query_box, query_input_area);
    if query_editing && query_input_area.width > 2 && query_input_area.height > 2 {
        frame.set_cursor_position(Position::new(
            query_input_area.x + 1 + query_input.cursor_col,
            query_input_area.y + 1,
        ));
    }

    let answer_text = if app.query_loading {
        let elapsed = app
            .query_started_at
            .map(|started| started.elapsed().as_millis() as u64)
            .unwrap_or_default();
        let pending = app
            .query_pending_question
            .as_deref()
            .unwrap_or(app.query_text.as_str());
        let previous = app
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
    } else if let Some(error) = &app.query_error {
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
    } else if let Some(response) = &app.query_response {
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
            app.query_roundtrip_timing,
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
    let mut state = app.query_table_state.clone();
    frame.render_stateful_widget(table, lower[0], &mut state);

    let detail_text = if let Some(result) = app.query_results().get(app.query_selected_index) {
        let result_number = app.query_selected_index + 1;
        let cited_in_answer = app.query_response.as_ref().is_some_and(|response| {
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
        } else if app.query_detail_loading {
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

fn current_query_display(app: &App) -> String {
    match &app.input_mode {
        InputMode::Query(value) => value.clone(),
        _ => app.query_text.clone(),
    }
}

struct QueryInputDisplay {
    text: String,
    cursor_col: u16,
    placeholder: bool,
}

fn query_input_display(value: &str, inner_width: u16) -> QueryInputDisplay {
    let width = inner_width as usize;
    if width == 0 {
        return QueryInputDisplay {
            text: String::new(),
            cursor_col: 0,
            placeholder: value.is_empty(),
        };
    }
    if value.is_empty() {
        let placeholder = "Ask project memory a question...";
        let text = placeholder.chars().take(width).collect::<String>();
        return QueryInputDisplay {
            text,
            cursor_col: 0,
            placeholder: true,
        };
    }

    let char_count = value.chars().count();
    if char_count <= width {
        return QueryInputDisplay {
            text: value.to_string(),
            cursor_col: char_count.min(width.saturating_sub(1)) as u16,
            placeholder: false,
        };
    }

    let tail_width = width.saturating_sub(1);
    let mut tail = value
        .chars()
        .skip(char_count.saturating_sub(tail_width))
        .collect::<String>();
    tail.insert(0, '<');
    QueryInputDisplay {
        text: tail,
        cursor_col: width.saturating_sub(1) as u16,
        placeholder: false,
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
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(11), Constraint::Min(8)])
        .split(area);

    let mut briefing_lines = activity_briefing_lines(app);
    briefing_lines.extend(llm_audit_status_lines(app));
    frame.render_widget(
        Paragraph::new(briefing_lines)
            .style(Style::default().bg(Theme::PANEL_ALT))
            .wrap(Wrap { trim: false })
            .block(themed_block("Get Up To Speed")),
        vertical[0],
    );

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(vertical[1]);

    let header = Row::new(["When", "Kind", "Tok", "Ms", "Summary"]).style(
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
            Constraint::Length(7),
            Constraint::Length(6),
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
        .scroll((app.activity_detail_scroll, 0))
        .style(Style::default().bg(Theme::PANEL))
        .wrap(Wrap { trim: false })
        .block(themed_block(format!(
            "Activity Detail (scroll {})",
            app.activity_detail_scroll
        )));
    frame.render_widget(detail, chunks[1]);
}

#[derive(Clone)]
struct ErrorItem {
    when: Option<DateTime<Utc>>,
    diagnostic: DiagnosticInfo,
}

fn draw_errors_tab(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let items = collect_error_items(app);
    let selected_index = app.errors_selected_index.min(items.len().saturating_sub(1));
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(44), Constraint::Percentage(56)])
        .split(area);

    let header = Row::new(["When", "Sev", "Source", "Component", "Summary"]).style(
        Style::default()
            .fg(Theme::ACCENT_STRONG)
            .bg(Theme::PANEL_ALT)
            .add_modifier(Modifier::BOLD),
    );
    let rows = items.iter().map(error_row);
    let table = Table::new(
        rows,
        [
            Constraint::Length(16),
            Constraint::Length(7),
            Constraint::Length(12),
            Constraint::Length(13),
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
    .block(themed_block(format!("Errors ({})", items.len())));
    let mut state = app.errors_table_state.clone();
    if items.is_empty() {
        state.select(None);
    } else {
        state.select(Some(selected_index));
    }
    frame.render_stateful_widget(table, chunks[0], &mut state);

    let lines = if let Some(item) = items.get(selected_index) {
        error_detail_lines(item)
    } else {
        vec![
            Line::from(Span::styled(
                "No diagnostics recorded for this project or TUI session.",
                Style::default().fg(Theme::SUCCESS),
            )),
            Line::from(Span::styled(
                "Provider errors, query failures, watcher failures, and TUI connection errors will appear here with fix hints.",
                Style::default().fg(Theme::MUTED),
            )),
        ]
    };
    let detail = Paragraph::new(lines)
        .scroll((app.errors_detail_scroll, 0))
        .style(Style::default().bg(Theme::PANEL))
        .wrap(Wrap { trim: false })
        .block(themed_block(format!(
            "Error Detail (scroll {})",
            app.errors_detail_scroll
        )));
    frame.render_widget(detail, chunks[1]);
}

fn collect_error_items(app: &App) -> Vec<ErrorItem> {
    let mut items = Vec::new();
    if !app.health_ok {
        items.push(ErrorItem {
            when: Some(Utc::now()),
            diagnostic: session_diagnostic(
                "backend_unavailable",
                "tui",
                "service",
                "health",
                "Memory Layer backend is unavailable.",
                Some("The TUI cannot reach the service yet or the service health check is failing."),
                Some("Start the service or run `memory doctor` to inspect configuration and database connectivity."),
            ),
        });
    }
    for (code, component, operation, message) in [
        ("query_failed", "tui", "query", app.query_error.as_ref()),
        ("agents_failed", "tui", "agents", app.agent_error.as_ref()),
        ("resume_failed", "tui", "resume", app.resume_error.as_ref()),
        (
            "activity_failed",
            "tui",
            "activity",
            app.activity_error.as_ref(),
        ),
        (
            "briefing_failed",
            "tui",
            "up_to_speed",
            app.up_to_speed_error.as_ref(),
        ),
        (
            "embeddings_failed",
            "tui",
            "embeddings",
            app.embedding_backends_error.as_ref(),
        ),
    ] {
        if let Some(message) = message {
            items.push(ErrorItem {
                when: Some(Utc::now()),
                diagnostic: session_diagnostic(
                    code,
                    "tui",
                    component,
                    operation,
                    message,
                    Some("This error was observed locally by the current TUI session."),
                    Some("Refresh the tab, then run `memory doctor` if the problem persists."),
                ),
            });
        }
    }
    for entry in &app.activity_events {
        if let ActivityEntry::Backend(event) = entry {
            match &event.details {
                Some(ActivityDetails::Diagnostic { diagnostic }) => items.push(ErrorItem {
                    when: Some(event.recorded_at),
                    diagnostic: diagnostic.clone(),
                }),
                Some(ActivityDetails::Query {
                    error: Some(error), ..
                }) => {
                    items.push(ErrorItem {
                        when: Some(event.recorded_at),
                        diagnostic: session_diagnostic(
                            "query_error",
                            event.source.as_deref().unwrap_or("service"),
                            "query",
                            "query",
                            error,
                            Some("A persisted project query failed."),
                            Some("Open the query/activity detail and run `memory doctor` if this repeats."),
                        ),
                    });
                }
                Some(ActivityDetails::WatcherHealth {
                    health: WatcherHealth::Failed | WatcherHealth::Stale | WatcherHealth::Restarting,
                    message,
                    watcher_id,
                    ..
                }) => {
                    items.push(ErrorItem {
                        when: Some(event.recorded_at),
                        diagnostic: session_diagnostic(
                            "watcher_health",
                            event.source.as_deref().unwrap_or("watcher"),
                            "watcher",
                            "heartbeat",
                            message.as_deref().unwrap_or(&event.summary),
                            Some("A watcher reported unhealthy or restarting state."),
                            Some(&format!(
                                "Inspect watcher `{watcher_id}` with `memory watcher list` or run `memory doctor`."
                            )),
                        ),
                    });
                }
                _ if matches!(event.kind, ActivityKind::QueryError) => items.push(ErrorItem {
                    when: Some(event.recorded_at),
                    diagnostic: session_diagnostic(
                        "query_error",
                        event.source.as_deref().unwrap_or("service"),
                        "query",
                        "query",
                        &event.summary,
                        Some("A persisted project query failed."),
                        Some("Open the activity detail and run `memory doctor` if this repeats."),
                    ),
                }),
                _ => {}
            }
        }
    }
    items.sort_by_key(|item| std::cmp::Reverse(item.when));
    items
}

fn session_diagnostic(
    code: &str,
    source: &str,
    component: &str,
    operation: &str,
    message: &str,
    explanation: Option<&str>,
    fix_hint: Option<&str>,
) -> DiagnosticInfo {
    DiagnosticInfo {
        code: code.to_string(),
        source: source.to_string(),
        component: component.to_string(),
        operation: operation.to_string(),
        severity: DiagnosticSeverity::Error,
        message: message.to_string(),
        raw_error: Some(message.to_string()),
        explanation: explanation.map(str::to_string),
        fix_hint: fix_hint.map(str::to_string),
        doctor_hint: Some("memory doctor".to_string()),
        command_hint: Some("memory doctor".to_string()),
    }
}

fn error_count(app: &App) -> usize {
    collect_error_items(app).len()
}

fn error_row(item: &ErrorItem) -> Row<'static> {
    Row::new(vec![
        Cell::from(Span::styled(
            item.when
                .map(format_timestamp_short)
                .unwrap_or_else(|| "-".to_string()),
            Style::default().fg(Theme::TEXT),
        )),
        Cell::from(Span::styled(
            diagnostic_severity_label(&item.diagnostic.severity),
            Style::default().fg(diagnostic_severity_color(&item.diagnostic.severity)),
        )),
        Cell::from(Span::styled(
            non_empty_or(&item.diagnostic.source, "unknown"),
            Style::default().fg(Theme::MUTED),
        )),
        Cell::from(Span::styled(
            non_empty_or(&item.diagnostic.component, "unknown"),
            Style::default().fg(Theme::ACCENT),
        )),
        Cell::from(Span::styled(
            item.diagnostic.message.clone(),
            Style::default().fg(Theme::TEXT),
        )),
    ])
}

fn error_detail_lines(item: &ErrorItem) -> Vec<Line<'static>> {
    let diagnostic = &item.diagnostic;
    let mut lines = vec![
        Line::from(vec![
            label_span("When: "),
            Span::styled(
                item.when
                    .map(format_timestamp_full)
                    .unwrap_or_else(|| "session-local".to_string()),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
        Line::from(vec![
            label_span("Severity: "),
            Span::styled(
                diagnostic_severity_label(&diagnostic.severity),
                Style::default()
                    .fg(diagnostic_severity_color(&diagnostic.severity))
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            label_span("Code: "),
            Span::styled(
                non_empty_or(&diagnostic.code, "unknown"),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
        Line::from(vec![
            label_span("Source: "),
            Span::styled(
                non_empty_or(&diagnostic.source, "unknown"),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Component: "),
            Span::styled(
                non_empty_or(&diagnostic.component, "unknown"),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Operation: "),
            Span::styled(
                non_empty_or(&diagnostic.operation, "unknown"),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
        Line::from(""),
        Line::from(vec![section_span("Summary")]),
        Line::from(Span::styled(
            diagnostic.message.clone(),
            Style::default().fg(Theme::TEXT),
        )),
    ];
    if let Some(explanation) = &diagnostic.explanation {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![section_span("Explanation")]));
        lines.push(Line::from(Span::styled(
            explanation.clone(),
            Style::default().fg(Theme::TEXT),
        )));
    }
    if let Some(fix_hint) = &diagnostic.fix_hint {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![section_span("How To Fix")]));
        lines.push(Line::from(Span::styled(
            fix_hint.clone(),
            Style::default().fg(Theme::SUCCESS),
        )));
    }
    if diagnostic.doctor_hint.is_some() || diagnostic.command_hint.is_some() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![section_span("Commands")]));
        if let Some(command) = &diagnostic.doctor_hint {
            lines.push(Line::from(vec![
                label_span("Doctor: "),
                Span::styled(command.clone(), Style::default().fg(Theme::ACCENT_STRONG)),
            ]));
        }
        if let Some(command) = &diagnostic.command_hint {
            lines.push(Line::from(vec![
                label_span("Related: "),
                Span::styled(command.clone(), Style::default().fg(Theme::ACCENT_STRONG)),
            ]));
        }
    }
    if let Some(raw_error) = &diagnostic.raw_error {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![section_span("Raw Error")]));
        for line in raw_error.lines().take(12) {
            lines.push(Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(Theme::MUTED),
            )));
        }
    }
    lines
}

fn diagnostic_severity_label(severity: &DiagnosticSeverity) -> &'static str {
    match severity {
        DiagnosticSeverity::Info => "info",
        DiagnosticSeverity::Warning => "warn",
        DiagnosticSeverity::Error => "error",
    }
}

fn diagnostic_severity_color(severity: &DiagnosticSeverity) -> Color {
    match severity {
        DiagnosticSeverity::Info => Theme::ACCENT,
        DiagnosticSeverity::Warning => Theme::WARNING,
        DiagnosticSeverity::Error => Theme::DANGER,
    }
}

fn non_empty_or(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn activity_briefing_lines(app: &App) -> Vec<Line<'static>> {
    if app.up_to_speed_loading {
        return vec![Line::from(Span::styled(
            "Generating get-up-to-speed briefing...",
            Style::default().fg(Theme::ACCENT_STRONG),
        ))];
    }
    if let Some(error) = &app.up_to_speed_error {
        return vec![Line::from(Span::styled(
            format!("Briefing failed: {error}"),
            Style::default().fg(Theme::DANGER),
        ))];
    }
    if let Some(response) = &app.up_to_speed_response {
        let mut lines = vec![Line::from(Span::styled(
            response
                .briefing
                .lines()
                .next()
                .unwrap_or("Get-up-to-speed briefing")
                .to_string(),
            Style::default().fg(Theme::TEXT),
        ))];
        if !response.next_actions.is_empty() {
            lines.push(Line::from(vec![
                label_span("Next: "),
                Span::styled(
                    response.next_actions[0].title.clone(),
                    Style::default().fg(Theme::ACCENT_STRONG),
                ),
            ]));
        }
        lines.push(Line::from(vec![
            label_span("Support: "),
            Span::styled(
                format!(
                    "{} activities / {} useful memories / {} token-tracked actions",
                    response.recent_activities.len(),
                    response.useful_memories.len(),
                    response.token_usage.action_count
                ),
                Style::default().fg(Theme::TEXT),
            ),
        ]));
        return lines;
    }
    vec![
        Line::from(Span::styled(
            "Press g for a deterministic briefing, or L for an LLM-assisted briefing.",
            Style::default().fg(Theme::TEXT),
        )),
        Line::from(Span::styled(
            "The briefing uses persisted activities, recent memory changes, commits, warnings, and token counts.",
            Style::default().fg(Theme::MUTED),
        )),
    ]
}

fn llm_audit_status_lines(app: &App) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from("")];
    if app.llm_audit_toggling {
        lines.push(Line::from(vec![
            label_span("LLM audit: "),
            Span::styled("updating...", Style::default().fg(Theme::ACCENT_STRONG)),
            Span::styled("  A toggle", Style::default().fg(Theme::MUTED)),
        ]));
        return lines;
    }
    if app.llm_audit_loading {
        lines.push(Line::from(vec![
            label_span("LLM audit: "),
            Span::styled("loading...", Style::default().fg(Theme::ACCENT)),
        ]));
        return lines;
    }
    if let Some(error) = &app.llm_audit_error {
        lines.push(Line::from(vec![
            label_span("LLM audit: "),
            Span::styled("unknown", Style::default().fg(Theme::WARNING)),
            Span::styled(format!("  {error}"), Style::default().fg(Theme::MUTED)),
        ]));
        lines.push(Line::from(Span::styled(
            "Press A to retry toggling, or run memory doctor if status stays unavailable.",
            Style::default().fg(Theme::MUTED),
        )));
        return lines;
    }
    let Some(status) = &app.llm_audit_status else {
        lines.push(Line::from(vec![
            label_span("LLM audit: "),
            Span::styled("unknown", Style::default().fg(Theme::MUTED)),
            Span::styled("  A enable", Style::default().fg(Theme::MUTED)),
        ]));
        return lines;
    };
    lines.push(Line::from(vec![
        label_span("LLM audit: "),
        Span::styled(
            if status.enabled { "on" } else { "off" },
            Style::default()
                .fg(if status.enabled {
                    Theme::SUCCESS
                } else {
                    Theme::MUTED
                })
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                "  redaction={}  profile={}  A {}",
                if status.redacted { "on" } else { "off" },
                status.profile,
                if status.enabled { "disable" } else { "enable" }
            ),
            Style::default().fg(Theme::MUTED),
        ),
    ]));
    if let Some(path) = &status.config_path {
        lines.push(Line::from(vec![
            label_span("Audit config: "),
            Span::styled(path.clone(), Style::default().fg(Theme::MUTED)),
        ]));
    }
    lines
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

fn recent_activity_lines(app: &App) -> Vec<Line<'static>> {
    if app.activity_events.is_empty() {
        return vec![Line::from(Span::styled(
            "No recent activity in this TUI session.",
            Style::default().fg(Theme::MUTED),
        ))];
    }

    app.activity_events
        .iter()
        .take(6)
        .map(|event| {
            Line::from(vec![
                Span::styled(
                    format_timestamp_short(activity_recorded_at(event)),
                    Style::default().fg(Theme::MUTED),
                ),
                Span::raw(" "),
                activity_entry_kind_span(event),
                Span::raw(" "),
                Span::styled(activity_summary(event), Style::default().fg(Theme::TEXT)),
            ])
        })
        .collect()
}

fn latest_plan_display(app: &App) -> String {
    app.all_memories
        .iter()
        .filter(|item| item.memory_type == MemoryType::Plan)
        .max_by(|left, right| {
            left.updated_at
                .cmp(&right.updated_at)
                .then_with(|| left.id.cmp(&right.id))
        })
        .map(|item| {
            let thread = item
                .tags
                .iter()
                .find_map(|tag| tag.strip_prefix("plan-thread:"));
            match thread {
                Some(thread) => format!("{} ({thread})", item.summary),
                None => item.summary.clone(),
            }
        })
        .unwrap_or_else(|| "none".to_string())
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
                "Start the Linux manager with `memory watcher manager enable`, or use `memory watcher enable --project <slug>` / `memory watcher run --project <slug>` for manual mode.",
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
                "Start the Linux manager with `memory watcher manager enable`, or use `memory watcher enable --project <slug>` / `memory watcher run --project <slug>` for manual mode.",
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
        if watcher.agent_cli.is_some() || watcher.agent_session_id.is_some() {
            lines.push(Line::from(Span::styled(
                format!(
                    "  owner: {} session={} pid={}",
                    watcher
                        .agent_cli
                        .clone()
                        .unwrap_or_else(|| "unknown".to_string()),
                    watcher
                        .agent_session_id
                        .clone()
                        .unwrap_or_else(|| "n/a".to_string()),
                    watcher
                        .agent_pid
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "n/a".to_string()),
                ),
                Style::default().fg(Theme::MUTED),
            )));
        }
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
    // Build the summary cell with an optional "v2"/"v3"/... badge so the
    // user can tell at a glance that the row is a replacement rather than
    // an original capture. v1 never shows a badge to keep the list clean.
    let mut summary_spans = Vec::with_capacity(2);
    summary_spans.push(Span::styled(
        item.summary.clone(),
        Style::default().fg(Theme::TEXT),
    ));
    if item.version_no > 1 {
        summary_spans.push(Span::raw("  "));
        summary_spans.push(Span::styled(
            format!("v{}", item.version_no),
            Style::default()
                .fg(Theme::ACCENT_STRONG)
                .add_modifier(Modifier::BOLD),
        ));
    }
    Row::new(vec![
        Cell::from(Line::from(summary_spans)),
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

fn agent_row(session: &AgentSession) -> Row<'static> {
    Row::new(vec![
        Cell::from(Span::styled(
            session.project_name.clone(),
            Style::default().fg(Theme::TEXT),
        )),
        Cell::from(Span::styled(
            session.agent_cli.to_string(),
            Style::default().fg(Theme::ACCENT),
        )),
        Cell::from(agent_status_span(&session.status)),
        Cell::from(Span::styled(
            format_token_count(session.total_tokens()),
            Style::default().fg(Theme::TEXT),
        )),
        Cell::from(Span::styled(
            format_context_percent(session.context_percent),
            context_percent_style(session.context_percent),
        )),
        Cell::from(Span::styled(
            agent_task_summary(session),
            Style::default().fg(Theme::TEXT),
        )),
    ])
}

fn query_row(result_number: usize, item: &QueryResult, cited: bool) -> Row<'static> {
    let number = if cited {
        format!("[{result_number}]")
    } else {
        result_number.to_string()
    };
    let number_style = if cited {
        Style::default()
            .fg(Theme::SUCCESS)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Theme::MUTED)
    };
    Row::new(vec![
        Cell::from(Span::styled(number, number_style)),
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

fn format_query_citation_numbers(numbers: &[usize]) -> String {
    if numbers.is_empty() {
        "none".to_string()
    } else {
        numbers
            .iter()
            .map(|number| format!("[{number}]"))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn query_answer_method_span(method: &QueryAnswerMethod) -> Span<'static> {
    let color = match method {
        QueryAnswerMethod::Llm => Theme::SUCCESS,
        QueryAnswerMethod::Deterministic => Theme::ACCENT,
        QueryAnswerMethod::Fallback => Theme::WARNING,
    };
    Span::styled(method.to_string(), Style::default().fg(color))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct QueryTimingBreakdown {
    backend_reported_ms: u64,
    transport_overhead_ms: u64,
    retrieval_other_ms: u64,
}

fn query_timing_breakdown(
    response: &QueryResponse,
    timing: QueryRoundtripTiming,
) -> QueryTimingBreakdown {
    let diagnostics = &response.diagnostics;
    let backend_reported_ms = diagnostics
        .total_duration_ms
        .saturating_add(response.answer_generation.duration_ms);
    let retrieval_known_ms = diagnostics
        .lexical_duration_ms
        .saturating_add(diagnostics.semantic_duration_ms)
        .saturating_add(diagnostics.graph_duration_ms)
        .saturating_add(diagnostics.rerank_duration_ms);
    QueryTimingBreakdown {
        backend_reported_ms,
        transport_overhead_ms: timing.query_api_ms.saturating_sub(backend_reported_ms),
        retrieval_other_ms: diagnostics
            .total_duration_ms
            .saturating_sub(retrieval_known_ms),
    }
}

fn format_query_timing(value: Option<u64>) -> String {
    value
        .map(|value| format!("{value} ms"))
        .unwrap_or_else(|| "n/a".to_string())
}

fn format_query_timing_with_percent(value: u64, total: u64) -> String {
    value
        .saturating_mul(100)
        .checked_div(total)
        .map(|percent| format!("{value} ms ({percent}%)"))
        .unwrap_or_else(|| format!("{value} ms"))
}

fn query_timing_breakdown_lines(
    response: &QueryResponse,
    timing: Option<QueryRoundtripTiming>,
) -> Vec<Line<'static>> {
    let fallback_timing = QueryRoundtripTiming {
        query_api_ms: response
            .diagnostics
            .total_duration_ms
            .saturating_add(response.answer_generation.duration_ms),
        initial_detail_ms: None,
        ui_ready_ms: response
            .diagnostics
            .total_duration_ms
            .saturating_add(response.answer_generation.duration_ms),
    };
    let timing = timing.unwrap_or(fallback_timing);
    let breakdown = query_timing_breakdown(response, timing);
    let retrieval_total = response.diagnostics.total_duration_ms;

    vec![
        Line::from(vec![section_span("Timing Breakdown")]),
        Line::from(vec![
            label_span("UI ready: "),
            Span::styled(
                format_query_timing(Some(timing.ui_ready_ms)),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Query API: "),
            Span::styled(
                format_query_timing(Some(timing.query_api_ms)),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Initial detail: "),
            Span::styled(
                format_query_timing(timing.initial_detail_ms),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
        Line::from(vec![
            label_span("Backend: "),
            Span::styled(
                format_query_timing_with_percent(breakdown.backend_reported_ms, timing.ui_ready_ms),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Retrieval: "),
            Span::styled(
                format_query_timing_with_percent(retrieval_total, timing.ui_ready_ms),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Answer: "),
            Span::styled(
                format_query_timing_with_percent(
                    response.answer_generation.duration_ms,
                    timing.ui_ready_ms,
                ),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Overhead: "),
            Span::styled(
                format_query_timing(Some(breakdown.transport_overhead_ms)),
                Style::default().fg(Theme::MUTED),
            ),
        ]),
        Line::from(vec![
            label_span("Lexical: "),
            Span::styled(
                format!(
                    "{} candidates, {}",
                    response.diagnostics.lexical_candidates,
                    format_query_timing_with_percent(
                        response.diagnostics.lexical_duration_ms,
                        retrieval_total,
                    )
                ),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Semantic: "),
            Span::styled(
                format!(
                    "{} [{}], {}",
                    response.diagnostics.semantic_candidates,
                    response.diagnostics.semantic_status,
                    format_query_timing_with_percent(
                        response.diagnostics.semantic_duration_ms,
                        retrieval_total,
                    )
                ),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
        Line::from(vec![
            label_span("Graph: "),
            Span::styled(
                format!(
                    "{} [{}], {}",
                    response.diagnostics.graph_candidates,
                    response.diagnostics.graph_status,
                    format_query_timing_with_percent(
                        response.diagnostics.graph_duration_ms,
                        retrieval_total,
                    )
                ),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Rerank/relation: "),
            Span::styled(
                format_query_timing_with_percent(
                    response.diagnostics.rerank_duration_ms,
                    retrieval_total,
                ),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Other: "),
            Span::styled(
                format_query_timing_with_percent(breakdown.retrieval_other_ms, retrieval_total),
                Style::default().fg(Theme::MUTED),
            ),
        ]),
    ]
}

fn activity_row(item: &ActivityEntry) -> Row<'static> {
    Row::new(vec![
        Cell::from(Span::styled(
            format_timestamp_short(activity_recorded_at(item)),
            Style::default().fg(Theme::TEXT),
        )),
        Cell::from(activity_entry_kind_span(item)),
        Cell::from(Span::styled(
            activity_tokens(item),
            Style::default().fg(Theme::ACCENT_STRONG),
        )),
        Cell::from(Span::styled(
            activity_duration(item),
            Style::default().fg(Theme::MUTED),
        )),
        Cell::from(Span::styled(
            activity_summary(item),
            Style::default().fg(Theme::TEXT),
        )),
    ])
}

fn agent_detail_lines(app: &App, snapshot: &AgentSnapshot) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(vec![
            label_span("Collected: "),
            Span::styled(
                format_timestamp_short(snapshot.collected_at),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
        Line::from(vec![
            label_span("Sessions: "),
            Span::styled(
                snapshot.sessions.len().to_string(),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Orphan ports: "),
            Span::styled(
                snapshot.orphan_ports.len().to_string(),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
    ];

    let selected_agent_cli = app
        .agent_table_state
        .selected()
        .and_then(|i| snapshot.sessions.get(i))
        .map(|s| s.agent_cli);
    let matching_limits: Vec<_> = snapshot
        .rate_limits
        .iter()
        .filter(|rl| selected_agent_cli.is_none_or(|cli| cli == rl.source))
        .collect();
    if !matching_limits.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![section_span("Rate Limits")]));
        for rate_limit in &matching_limits {
            lines.push(Line::from(vec![
                label_span("Source: "),
                Span::styled(rate_limit.source.clone(), Style::default().fg(Theme::TEXT)),
            ]));
            if let Some(percent) = rate_limit.five_hour_pct {
                lines.push(quota_bar_line(
                    "5h",
                    percent,
                    20,
                    rate_limit_reset_label(rate_limit.five_hour_resets_at),
                ));
            }
            if let Some(percent) = rate_limit.seven_day_pct {
                lines.push(quota_bar_line(
                    "7d",
                    percent,
                    20,
                    rate_limit_reset_label(rate_limit.seven_day_resets_at),
                ));
            }
        }
    }

    if !snapshot.orphan_ports.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![section_span("Open Orphan Ports")]));
        for orphan in snapshot.orphan_ports.iter().take(6) {
            lines.push(Line::from(Span::styled(
                format!(
                    "- {}:{}  {}",
                    orphan.project_name, orphan.port, orphan.command
                ),
                Style::default().fg(Theme::WARNING),
            )));
        }
    }

    let Some(session) = snapshot.sessions.get(app.agent_selected_index) else {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "No agent sessions are currently visible.",
            Style::default().fg(Theme::MUTED),
        )));
        return lines;
    };

    lines.push(Line::from(""));
    lines.push(Line::from(vec![section_span("Selected Session")]));
    lines.push(Line::from(vec![
        label_span("Project: "),
        Span::styled(
            session.project_name.clone(),
            Style::default().fg(Theme::TEXT),
        ),
        Span::raw("   "),
        label_span("Agent: "),
        Span::styled(
            session.agent_cli.to_string(),
            Style::default().fg(Theme::TEXT),
        ),
    ]));
    lines.push(Line::from(vec![
        label_span("Status: "),
        agent_status_span(&session.status),
        Span::raw("   "),
        label_span("PID: "),
        Span::styled(session.pid.to_string(), Style::default().fg(Theme::TEXT)),
    ]));
    lines.push(Line::from(vec![
        label_span("Model: "),
        Span::styled(session.model.clone(), Style::default().fg(Theme::TEXT)),
    ]));
    lines.push(Line::from(vec![
        label_span("Session: "),
        Span::styled(session.session_id.clone(), Style::default().fg(Theme::TEXT)),
    ]));
    lines.push(Line::from(vec![
        label_span("CWD: "),
        Span::styled(session.cwd.clone(), Style::default().fg(Theme::TEXT)),
    ]));
    lines.push(Line::from(vec![
        label_span("Started: "),
        Span::styled(
            format_elapsed_from_started(session.started_at),
            Style::default().fg(Theme::TEXT),
        ),
        Span::raw("   "),
        label_span("Version: "),
        Span::styled(session.version.clone(), Style::default().fg(Theme::TEXT)),
    ]));
    lines.push(Line::from(vec![
        label_span("Context: "),
        Span::styled(
            format_context_percent(session.context_percent),
            context_percent_style(session.context_percent),
        ),
        Span::raw("   "),
        label_span("Tokens: "),
        Span::styled(
            format_token_count(session.total_tokens()),
            Style::default().fg(Theme::TEXT),
        ),
    ]));
    lines.push(usage_bar_line("Ctx", session.context_percent, 20, None));
    lines.push(Line::from(vec![
        label_span("Git: "),
        Span::styled(
            format!(
                "{}  +{} ~{}",
                session.git_branch, session.git_added, session.git_modified
            ),
            Style::default().fg(Theme::TEXT),
        ),
    ]));
    lines.push(Line::from(vec![
        label_span("Task: "),
        Span::styled(
            agent_task_summary(session),
            Style::default().fg(Theme::TEXT),
        ),
    ]));

    if !session.children.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![section_span("Child Processes")]));
        for child in session.children.iter().take(8) {
            lines.push(Line::from(Span::styled(
                format_agent_child(child),
                Style::default().fg(Theme::TEXT),
            )));
        }
    }

    lines
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
                                "lexical {} ms | semantic {} ms | graph {} ms | rerank {} ms | total {} ms",
                                response.diagnostics.lexical_duration_ms,
                                response.diagnostics.semantic_duration_ms,
                                response.diagnostics.graph_duration_ms,
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
                                "lexical {} | semantic {} | graph {} [{}] | merged {} | returned {} | relation {} | graph augmented {}",
                                response.diagnostics.lexical_candidates,
                                response.diagnostics.semantic_candidates,
                                response.diagnostics.graph_candidates,
                                response.diagnostics.graph_status,
                                response.diagnostics.merged_candidates,
                                response.diagnostics.returned_results,
                                response.diagnostics.relation_augmented_candidates,
                                response.diagnostics.graph_augmented_candidates
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
                                    "  debug: chunk {:.2} | entry {:.2} | semantic {:.2} | relation {:.2} | graph {:.2}",
                                    result.debug.chunk_fts,
                                    result.debug.entry_fts,
                                    result.debug.semantic_similarity,
                                    result.debug.relation_boost,
                                    result.debug.graph_boost
                                ),
                                Style::default().fg(Theme::MUTED),
                            )));
                            if !result.score_explanation.is_empty() {
                                lines.push(Line::from(Span::styled(
                                    format!("  why: {}", result.score_explanation.join(" | ")),
                                    Style::default().fg(Theme::ACCENT),
                                )));
                            }
                            if !result.graph_connections.is_empty() {
                                let graph = result
                                    .graph_connections
                                    .iter()
                                    .take(2)
                                    .map(|connection| {
                                        format!(
                                            "{} {} boost={:.2}",
                                            connection.reason,
                                            connection.file_path,
                                            connection.score_boost
                                        )
                                    })
                                    .collect::<Vec<_>>()
                                    .join(" | ");
                                lines.push(Line::from(Span::styled(
                                    format!("  graph: {graph}"),
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
        activity_kv_line(
            "Duration",
            activity_duration(&ActivityEntry::Backend(Box::new(event.clone()))),
        ),
        activity_kv_line(
            "Tokens",
            activity_tokens(&ActivityEntry::Backend(Box::new(event.clone()))),
        ),
        activity_kv_line(
            "Source",
            event.source.clone().unwrap_or_else(|| "n/a".to_string()),
        ),
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
            ActivityDetails::GraphExtract {
                repo_root,
                git_head,
                since,
                extraction_run_id,
                dry_run,
                reused_existing_run,
                index_reused,
                analyzer_version,
                strategy_version,
                symbol_count,
                reference_count,
                resolved_reference_count,
                unresolved_reference_count,
                ambiguous_reference_count,
                graph_node_count,
                graph_edge_count,
                evidence_count,
            } => {
                lines.push(activity_kv_line("Repo root", repo_root.clone()));
                if let Some(run_id) = extraction_run_id {
                    lines.push(activity_kv_line("Extraction run", run_id.to_string()));
                }
                lines.push(activity_kv_line("Dry run", dry_run.to_string()));
                lines.push(activity_kv_line(
                    "Reused existing run",
                    reused_existing_run.to_string(),
                ));
                lines.push(activity_kv_line("Index reused", index_reused.to_string()));
                lines.push(activity_kv_line("Analyzer", analyzer_version.clone()));
                lines.push(activity_kv_line("Strategy", strategy_version.clone()));
                lines.push(activity_kv_line("Symbols", symbol_count.to_string()));
                lines.push(activity_kv_line("References", reference_count.to_string()));
                lines.push(activity_kv_line(
                    "Resolved",
                    resolved_reference_count.to_string(),
                ));
                lines.push(activity_kv_line(
                    "Unresolved",
                    unresolved_reference_count.to_string(),
                ));
                lines.push(activity_kv_line(
                    "Ambiguous",
                    ambiguous_reference_count.to_string(),
                ));
                lines.push(activity_kv_line(
                    "Graph nodes",
                    graph_node_count.to_string(),
                ));
                lines.push(activity_kv_line(
                    "Graph edges",
                    graph_edge_count.to_string(),
                ));
                lines.push(activity_kv_line("Evidence", evidence_count.to_string()));
                if let Some(head) = git_head {
                    lines.push(activity_kv_line("HEAD", head.clone()));
                }
                if let Some(since) = since {
                    lines.push(activity_kv_line("Since", since.clone()));
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
                graph_status,
                graph_candidates,
                graph_augmented_candidates,
                graph_duration_ms,
                graph_result_count,
                graph_connection_count,
                graph_connections,
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
                if let Some(graph_status) = graph_status {
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![section_span("Graph Retrieval")]));
                    lines.push(activity_kv_line("Status", graph_status.clone()));
                    lines.push(activity_kv_line("Candidates", graph_candidates.to_string()));
                    lines.push(activity_kv_line(
                        "Augmented results",
                        graph_augmented_candidates.to_string(),
                    ));
                    lines.push(activity_kv_line(
                        "Duration",
                        format!("{graph_duration_ms} ms"),
                    ));
                    lines.push(activity_kv_line(
                        "Results with graph",
                        graph_result_count.to_string(),
                    ));
                    lines.push(activity_kv_line(
                        "Connections",
                        graph_connection_count.to_string(),
                    ));
                    if !graph_connections.is_empty() {
                        lines.push(Line::from(""));
                        lines.push(Line::from(vec![section_span("Graph Connections")]));
                        for connection in graph_connections {
                            let mut parts = vec![
                                connection.reason.clone(),
                                connection.file_path.clone(),
                                format!("boost={:.2}", connection.score_boost),
                            ];
                            if let Some(symbol) = &connection.symbol {
                                parts.push(format!("symbol={symbol}"));
                            }
                            if let Some(edge_kind) = &connection.edge_kind {
                                parts.push(format!("edge={edge_kind}"));
                            }
                            if let Some(neighbor) = &connection.neighbor_symbol {
                                parts.push(format!("neighbor={neighbor}"));
                            }
                            lines.push(Line::from(Span::styled(
                                format!("- {}", parts.join(" | ")),
                                Style::default().fg(Theme::TEXT),
                            )));
                        }
                    }
                }
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
            ActivityDetails::LlmAudit {
                operation,
                request_summary,
                status,
                redacted,
                truncated,
                messages,
                error,
            } => {
                lines.push(activity_kv_line("Operation", operation.clone()));
                lines.push(activity_kv_line("Request", request_summary.clone()));
                lines.push(activity_kv_line("Status", status.clone()));
                lines.push(activity_kv_line("Redacted", redacted.to_string()));
                lines.push(activity_kv_line("Truncated", truncated.to_string()));
                if let Some(error) = error {
                    lines.push(activity_kv_line("Error", error.clone()));
                }
                if !messages.is_empty() {
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![section_span("LLM Messages")]));
                    for message in messages {
                        lines.push(Line::from(vec![
                            label_span(format!("Role {}: ", message.role)),
                            Span::styled(
                                if message.truncated {
                                    format!("{}\n[message truncated]", message.content)
                                } else {
                                    message.content.clone()
                                },
                                Style::default().fg(Theme::TEXT),
                            ),
                        ]));
                    }
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
            ActivityDetails::Diagnostic { diagnostic } => {
                lines.extend(error_detail_lines(&ErrorItem {
                    when: Some(event.recorded_at),
                    diagnostic: diagnostic.clone(),
                }));
            }
            ActivityDetails::WatcherHealth {
                watcher_id,
                hostname,
                health,
                managed_by_service,
                restart_attempt_count,
                agent_cli,
                agent_session_id,
                agent_pid,
                previous_health,
                recovered_after_restart_attempts,
                message,
            } => {
                lines.push(activity_kv_line("Watcher", watcher_id.clone()));
                lines.push(activity_kv_line("Hostname", hostname.clone()));
                if let Some(agent_cli) = agent_cli {
                    lines.push(activity_kv_line("Agent CLI", agent_cli.clone()));
                }
                if let Some(agent_session_id) = agent_session_id {
                    lines.push(activity_kv_line("Agent session", agent_session_id.clone()));
                }
                if let Some(agent_pid) = agent_pid {
                    lines.push(activity_kv_line("Agent PID", agent_pid.to_string()));
                }
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

fn activity_tokens(item: &ActivityEntry) -> String {
    match item {
        ActivityEntry::Backend(event) => event
            .token_usage
            .as_ref()
            .map(|usage| format_compact_count(usage.total_tokens))
            .unwrap_or_else(|| "-".to_string()),
        ActivityEntry::Query(entry) => match &entry.outcome {
            QueryLogOutcome::Success(response) => response
                .answer_generation
                .token_usage
                .as_ref()
                .map(|usage| format_compact_count(usage.total_tokens))
                .unwrap_or_else(|| "-".to_string()),
            QueryLogOutcome::Error(_) => "-".to_string(),
        },
    }
}

fn activity_duration(item: &ActivityEntry) -> String {
    match item {
        ActivityEntry::Backend(event) => event
            .duration_ms
            .map(format_compact_count)
            .unwrap_or_else(|| "-".to_string()),
        ActivityEntry::Query(entry) => format_compact_count(entry.duration_ms),
    }
}

fn format_compact_count(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}m", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}k", value as f64 / 1_000.0)
    } else {
        value.to_string()
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

fn embedding_base_url_is_default(provider: &str, base_url: &str) -> bool {
    // Keep in sync with mem_search::embedding_backend::default_base_url.
    let expected = match provider {
        "openai_compatible" | "openai" => "https://api.openai.com/v1",
        "ollama" => "http://127.0.0.1:11434/v1",
        "voyage" => "https://api.voyageai.com",
        "cohere" => "https://api.cohere.com",
        "gemini" => "https://generativelanguage.googleapis.com/v1beta",
        _ => return false,
    };
    base_url.trim_end_matches('/') == expected
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

fn split_root_area(area: Rect) -> [Rect; 4] {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(4),
        ])
        .split(area);
    [chunks[0], chunks[1], chunks[2], chunks[3]]
}

fn split_memories_area(area: Rect) -> [Rect; 2] {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);
    [chunks[0], chunks[1]]
}

fn current_frame_area() -> Option<Rect> {
    let (width, height) = crossterm::terminal::size().ok()?;
    Some(Rect::new(0, 0, width, height))
}

fn default_frame_area() -> Rect {
    Rect::new(0, 0, 160, 48)
}

fn memory_detail_max_scroll(app: &App, frame_area: Rect) -> u16 {
    let root = split_root_area(frame_area);
    let detail_area = split_memories_area(root[2])[1];
    let block = themed_focus_block("Detail", app.memories_focus == MemoriesFocus::Detail);
    let inner = block.inner(detail_area);
    if inner.width == 0 || inner.height == 0 {
        return 0;
    }
    wrapped_line_count(&build_memory_detail_lines(app), inner.width)
        .saturating_sub(inner.height as usize) as u16
}

fn help_max_scroll(tab: TabKind, frame_area: Rect) -> u16 {
    let root = split_root_area(frame_area);
    help_max_scroll_in_area(tab, root[2])
}

fn help_max_scroll_in_area(tab: TabKind, area: Rect) -> u16 {
    let block = themed_block("Help");
    let inner = block.inner(area);
    if inner.width == 0 || inner.height == 0 {
        return 0;
    }
    wrapped_line_count(&tab_help_lines(tab), inner.width).saturating_sub(inner.height as usize)
        as u16
}

fn tab_help_lines(tab: TabKind) -> Vec<Line<'static>> {
    render_markdown_lines(tab_help_markdown(tab))
}

fn tab_help_markdown(tab: TabKind) -> &'static str {
    match tab {
        TabKind::Memories => {
            r#"# Memories Help

## Purpose
Browse canonical project memory, inspect one entry in detail, and maintain durable knowledge.

## Layout
- Left table: filtered memories with summary, type, status, confidence, and update time.
- Right detail: canonical text, embeddings, tags, sources, history, and related memories.
- Focus indicator: shows whether movement keys select memories or scroll detail.

## Controls
- `j/k` or `Up/Down`: select memories or scroll detail when detail focus is active.
- `Enter`: toggle list/detail focus. `Esc`: return to list focus.
- `PgUp/PgDn`, `Home`, `End`: scroll or jump detail.
- `/`: text filter. `g`: tag filter. `s`: status filter. `t`: type filter. `x`: clear filters.
- `c`: curate. `i`: reindex chunks. `e`: re-embed active space. `a`: archive low-value memories. `Shift+D`: delete. `Shift+H`: history.

## Workflows
- Filter by type or text, select a memory, then read canonical text and sources.
- Verify provenance before relying on a memory in implementation work.
- Use curation and Review rather than creating duplicate memories.

## Troubleshooting
- If detail is empty, move selection or refresh project state.
- If embeddings are missing, use `e` here or the Embeddings tab.
"#
        }
        TabKind::Agents => {
            r#"# Agents Help

## Purpose
Monitor live coding-agent sessions across projects, including process state, token pressure, context usage, rate limits, and active work.

## Layout
- Session table: detected Codex and Claude sessions, preferring the current project when possible.
- Detail pane: model, status, transcript, ports, child processes, current task, context budget, and rate limits.
- Auto-refresh: fast while this tab is visible, slower while hidden.

## Controls
- `j/k` or `Up/Down`: select a session.
- `PgUp/PgDn`: scroll details. `Home`: jump to top.

## Workflows
- Check which agent owns a watcher or whether a session is active, idle, stale, or over budget.
- Inspect context and rate-limit bars before adding more work to a busy session.
- Use process and port details to diagnose stuck local tools.

## Troubleshooting
- If no agents appear, check transcript permissions and watcher-manager state.
- If the wrong project is selected, restart the TUI from the intended repository.
"#
        }
        TabKind::Query => {
            r#"# Query Help

## Purpose
Ask questions against project memory and inspect the evidence, citations, timings, and graph connections behind the answer.

## Layout
- Question box: current or last submitted question.
- Query Result: answer, confidence, citations, evidence state, match count, and timing breakdown.
- Results/detail: ranked memories and why the selected memory matched.

## Controls
- `Enter`: start a new empty question from Query.
- `?`: jump to Query and start a question from anywhere.
- While editing: `Enter` submits, `Esc` cancels, `Up/Down` restores cached query history.
- `j/k`: move through results. `Shift+D`: delete selected result memory.

## Workflows
- Compare answer citations with numbered returned memories before trusting an answer.
- Use timing breakdown to locate slow lexical, semantic, graph, rerank, answer, or UI phases.
- Treat graph connections as retrieval explanations; citations still point to memories.

## Troubleshooting
- If evidence is insufficient, add or curate memory and ask again.
- If a restored history item is stale, press `Enter` to re-run it.
"#
        }
        TabKind::Activity => {
            r#"# Activity Help

## Purpose
Review persisted backend activity and generate get-up-to-speed briefings for new or returning agents.

## Layout
- Briefing panel: deterministic or LLM-generated continuity context plus LLM audit/debug status.
- Activity table: event time, kind, tokens, duration, and summary.
- Detail pane: selected event metadata, including query diagnostics, graph details, token usage, or curation counts.

## Controls
- `j/k` or `Up/Down`: select activity.
- `PgUp/PgDn`: scroll detail. `Home`: jump to top.
- `g`: deterministic briefing. `Shift+L`: LLM briefing. `r`: refresh.
- `Shift+A`: toggle LLM audit/debug logging in the running service and persist the config.

## Workflows
- Use this tab at handoff or after interruption.
- Enable audit briefly when you need to inspect service-side LLM prompts, then disable it after debugging.
- Inspect token and duration fields to understand cost and latency.
- Open query activities to see retrieval mode, graph behavior, and answer cost.

## Troubleshooting
- If activity is empty, perform a query, capture, curation, graph extraction, or embedding operation.
- If LLM briefing fails, use deterministic briefing and check Errors.
"#
        }
        TabKind::Errors => {
            r#"# Errors Help

## Purpose
Inspect backend diagnostics and session-local TUI errors with explanations and suggested fixes.

## Layout
- Error table: time, severity, source, component, and summary.
- Detail pane: explanation, fix hints, command suggestions, and raw metadata.
- Sources include TUI, service, watcher, manager, database, and providers.

## Controls
- `j/k` or `Up/Down`: select an error.
- `PgUp/PgDn`: scroll detail. `Home`: jump to top.
- `r`: refresh diagnostics.

## Workflows
- Open this tab when the footer shows warnings/errors or an operation fails.
- Prefer suggested `memory doctor` commands when shown.
- Use source/component to route fixes to config, service, watcher, manager, provider, or database.

## Troubleshooting
- If the table is empty but the footer is red, refresh and check live connection state.
- If provider errors repeat, verify API keys and backend readiness.
"#
        }
        TabKind::Project => {
            r#"# Project Help

## Purpose
Show high-level project health, counts, embedding/search state, recent activity, and automation status.

## Layout
- Scrollable report with memory totals, type/status breakdowns, backend health, watcher/automation state, and embedding coverage.
- It is a dashboard for deciding which specialist tab to inspect next.

## Controls
- `j/k` or `Up/Down`: scroll.
- `PgUp/PgDn`: page. `Home`: jump to top.
- `r`: refresh project state outside help.

## Workflows
- Start here for a quick project health check.
- Use counts to spot missing memory, missing embeddings, or pending curation.
- Follow up in Memories, Activity, Errors, Watchers, or Embeddings.

## Troubleshooting
- If counts look stale, refresh after backend work completes.
- If backend state is unavailable, check Errors and the footer.
"#
        }
        TabKind::Review => {
            r#"# Review Help

## Purpose
Review replacement proposals so duplicate or superseded memories can be approved or rejected safely.

## Layout
- Proposal list: pending replacement candidates.
- Detail pane: target, candidate, policy, score, reasons, source overlap, and canonical text comparison.
- Replacement policy controls how aggressively curation proposes or applies replacements.

## Controls
- `j/k`, `Up/Down`, `[` and `]`: move through proposals.
- `PgUp/PgDn`: jump by page. `Home/End`: first/last proposal.
- `y`: approve. `n`: reject. `p`: cycle policy. `r`: refresh.

## Workflows
- Approve only when the candidate is clearly better and provenance remains valid.
- Reject lexical or ambiguous matches that would lose context.
- Change policy deliberately; stricter policies reduce replacement noise.

## Troubleshooting
- No proposals means no pending candidates or conservative policy.
- If approval fails, check Errors and refresh.
"#
        }
        TabKind::Watchers => {
            r#"# Watchers Help

## Purpose
Show project watchers, heartbeat state, agent ownership, service identity, restart attempts, and recovery behavior.

## Layout
- Scrollable watcher report.
- Each watcher shows health, mode, repo root, watcher id, owner agent/session/pid, host service, heartbeat, and restart attempts.
- Managed watchers belong to agent sessions; manual watchers were started directly.

## Controls
- `j/k` or `Up/Down`: scroll.
- `PgUp/PgDn`: page. `Home`: jump to top.
- `r`: refresh project state outside help.

## Workflows
- Use this tab when captures are not appearing or watcher health is degraded.
- Check owner/session and stale heartbeat before restarting anything.
- Use restart attempts to distinguish transient restarts from repeated failures.

## Troubleshooting
- If a managed watcher stays stale, check Manager footer and Errors.
- If only manual watchers exist, start through the manager-integrated path.
"#
        }
        TabKind::Embeddings => {
            r#"# Embeddings Help

## Purpose
Inspect embedding backends, compare per-project coverage, switch semantic search, and backfill missing vectors.

## Layout
- Header: active backend, create setting, ready/not-ready counts, and operation status.
- Table: backend name, provider, model, create flag, base URL, chunk count, and memory count.
- `*` marks active. `!` marks a backend that did not resolve at startup.

## Controls
- `j/k` or `Up/Down`: select backend.
- `Enter`: activate selected backend, or deactivate when selected backend is active.
- `c`: toggle automatic embedding creation.
- `e`: create missing embeddings for selected backend.
- `I`: rebuild chunks and embeddings for all configured backends.
- `r`: refresh backend list and counts.

## Workflows
- Use `e` for normal missing-embedding backfill.
- Use `I` only when chunks need rebuilding or all backends should be refreshed.
- Switch active backend after both spaces are populated to compare semantic retrieval.

## Troubleshooting
- If a backend has `!`, fix API key/model config and restart service.
- If counts differ, run `e` on the lower-coverage backend.
"#
        }
        TabKind::Resume => {
            r#"# Resume Help

## Purpose
Get back into flow after interruption with checkpoint, current thread, next step, recent changes, attention items, and durable context.

## Layout
- Scrollable briefing with checkpoint metadata, current thread, next step, change summary, attention items, context memories, and recent activity.
- Loading and error lines appear at the top.

## Controls
- `j/k` or `Up/Down`: scroll.
- `PgUp/PgDn`: page. `Home`: jump to top.
- `r`: refresh resume context outside help.

## Workflows
- Open this first when returning to a task or handing work to another agent.
- Use the next-step section as the immediate continuation point.
- Follow context references into Memories or Query for provenance.

## Troubleshooting
- If there is no checkpoint, save one before leaving future sessions.
- If resume feels stale, refresh after recent activity or curation completes.
"#
        }
    }
}

fn wrapped_line_count(lines: &[Line<'_>], width: u16) -> usize {
    if width == 0 {
        return 0;
    }
    let width = width as usize;
    lines
        .iter()
        .map(|line| {
            let line_width = line.width();
            if line_width == 0 {
                1
            } else {
                line_width.div_ceil(width)
            }
        })
        .sum()
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

fn themed_focus_block<'a>(title: impl Into<Line<'a>>, focused: bool) -> Block<'a> {
    let border = if focused {
        Theme::ACCENT
    } else {
        Theme::BORDER
    };
    let title_color = if focused {
        Theme::ACCENT_STRONG
    } else {
        Theme::TITLE
    };
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border).add_modifier(if focused {
            Modifier::BOLD
        } else {
            Modifier::empty()
        }))
        .title(title)
        .title_style(
            Style::default()
                .fg(title_color)
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().bg(Theme::PANEL))
}

fn render_markdown_lines(input: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut in_code_block = false;

    for raw_line in input.lines() {
        let line = raw_line.trim_end_matches('\r');

        if let Some(fence) = line.trim_start().strip_prefix("```") {
            in_code_block = !in_code_block;
            if !fence.trim().is_empty() && in_code_block {
                lines.push(Line::from(vec![
                    Span::styled("code ", Style::default().fg(Theme::ACCENT_STRONG)),
                    Span::styled(
                        fence.trim().to_string(),
                        Style::default()
                            .fg(Theme::ACCENT_STRONG)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
            } else {
                lines.push(Line::from(""));
            }
            continue;
        }

        if in_code_block {
            lines.push(Line::from(vec![Span::styled(
                format!("  {line}"),
                Style::default()
                    .fg(Theme::TEXT)
                    .bg(Theme::PANEL_ALT)
                    .add_modifier(Modifier::BOLD),
            )]));
            continue;
        }

        if line.trim().is_empty() {
            lines.push(Line::from(""));
            continue;
        }

        if is_thematic_break(line) {
            lines.push(Line::from(Span::styled(
                "─".repeat(32),
                Style::default().fg(Theme::BORDER),
            )));
            continue;
        }

        if let Some((level, content)) = parse_heading(line) {
            lines.push(Line::from(render_inline_markdown(
                content,
                heading_style(level),
            )));
            continue;
        }

        if let Some((depth, content)) = parse_blockquote(line) {
            let mut spans = vec![Span::styled(
                format!("{} ", "│ ".repeat(depth.max(1))),
                Style::default().fg(Theme::ACCENT),
            )];
            spans.extend(render_inline_markdown(
                content,
                Style::default()
                    .fg(Theme::TEXT)
                    .add_modifier(Modifier::ITALIC),
            ));
            lines.push(Line::from(spans));
            continue;
        }

        if let Some((indent, marker, content, checked)) = parse_list_item(line) {
            let mut spans = vec![Span::styled(
                " ".repeat(indent),
                Style::default().fg(Theme::TEXT),
            )];
            let marker_span = match checked {
                Some(true) => Span::styled(
                    "[x] ".to_string(),
                    Style::default()
                        .fg(Theme::SUCCESS)
                        .add_modifier(Modifier::BOLD),
                ),
                Some(false) => Span::styled(
                    "[ ] ".to_string(),
                    Style::default()
                        .fg(Theme::WARNING)
                        .add_modifier(Modifier::BOLD),
                ),
                None => Span::styled(
                    marker,
                    Style::default()
                        .fg(Theme::ACCENT_STRONG)
                        .add_modifier(Modifier::BOLD),
                ),
            };
            spans.push(marker_span);
            spans.extend(render_inline_markdown(
                content,
                Style::default().fg(Theme::TEXT),
            ));
            lines.push(Line::from(spans));
            continue;
        }

        lines.push(Line::from(render_inline_markdown(
            line,
            Style::default().fg(Theme::TEXT),
        )));
    }

    if lines.is_empty() {
        vec![Line::from("")]
    } else {
        lines
    }
}

fn heading_style(level: usize) -> Style {
    let color = match level {
        1 => Theme::ACCENT_STRONG,
        2 => Theme::ACCENT,
        _ => Theme::TITLE,
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

fn is_thematic_break(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.len() >= 3
        && (trimmed.chars().all(|ch| ch == '-')
            || trimmed.chars().all(|ch| ch == '*')
            || trimmed.chars().all(|ch| ch == '_'))
}

fn parse_heading(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|&ch| ch == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let content = trimmed[hashes..].trim_start();
    (!content.is_empty()).then_some((hashes, content))
}

fn parse_blockquote(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_start();
    let mut depth = 0usize;
    let mut rest = trimmed;
    while let Some(remainder) = rest.strip_prefix('>') {
        depth += 1;
        rest = remainder.trim_start();
    }
    (depth > 0).then_some((depth, rest))
}

fn parse_list_item(line: &str) -> Option<(usize, String, &str, Option<bool>)> {
    let indent = line.chars().take_while(|ch| ch.is_whitespace()).count();
    let trimmed = &line[indent..];
    for bullet in ["- ", "* ", "+ "] {
        if let Some(rest) = trimmed.strip_prefix(bullet) {
            if let Some(content) = rest.strip_prefix("[ ] ") {
                return Some((indent, String::new(), content, Some(false)));
            }
            if let Some(content) = rest.strip_prefix("[x] ") {
                return Some((indent, String::new(), content, Some(true)));
            }
            if let Some(content) = rest.strip_prefix("[X] ") {
                return Some((indent, String::new(), content, Some(true)));
            }
            return Some((indent, "• ".to_string(), rest, None));
        }
    }
    let digits = trimmed.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digits > 0 && trimmed[digits..].starts_with(". ") {
        let number = &trimmed[..digits];
        let content = &trimmed[(digits + 2)..];
        return Some((indent, format!("{number}. "), content, None));
    }
    None
}

fn render_inline_markdown(input: &str, base_style: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut buffer = String::new();
    let mut chars = input.chars().peekable();
    let mut emphasis = false;
    let mut strong = false;
    let mut code = false;

    let flush = |spans: &mut Vec<Span<'static>>,
                 buffer: &mut String,
                 emphasis: bool,
                 strong: bool,
                 code: bool,
                 base_style: Style| {
        if buffer.is_empty() {
            return;
        }
        spans.push(Span::styled(
            std::mem::take(buffer),
            inline_markdown_style(base_style, emphasis, strong, code),
        ));
    };

    while let Some(ch) = chars.next() {
        if ch == '[' {
            let mut label = String::new();
            let mut temp = chars.clone();
            let mut found_close = false;
            for next in temp.by_ref() {
                if next == ']' {
                    found_close = true;
                    break;
                }
                label.push(next);
            }
            if found_close {
                let mut temp_after = temp.clone();
                if temp_after.next() == Some('(') {
                    let mut url = String::new();
                    let mut found_url_close = false;
                    for next in temp_after {
                        if next == ')' {
                            found_url_close = true;
                            break;
                        }
                        url.push(next);
                    }
                    if found_url_close {
                        flush(&mut spans, &mut buffer, emphasis, strong, code, base_style);
                        for _ in 0..(label.chars().count() + url.chars().count() + 3) {
                            let _ = chars.next();
                        }
                        spans.push(Span::styled(
                            format!("{label} ({url})"),
                            inline_markdown_style(base_style, emphasis, strong, code)
                                .fg(Theme::ACCENT),
                        ));
                        continue;
                    }
                }
            }
            buffer.push(ch);
            continue;
        }

        if ch == '`' {
            flush(&mut spans, &mut buffer, emphasis, strong, code, base_style);
            code = !code;
            continue;
        }

        if (ch == '*' || ch == '_') && chars.peek() == Some(&ch) {
            let _ = chars.next();
            flush(&mut spans, &mut buffer, emphasis, strong, code, base_style);
            strong = !strong;
            continue;
        }

        if ch == '*' || ch == '_' {
            flush(&mut spans, &mut buffer, emphasis, strong, code, base_style);
            emphasis = !emphasis;
            continue;
        }

        buffer.push(ch);
    }

    flush(&mut spans, &mut buffer, emphasis, strong, code, base_style);
    if spans.is_empty() {
        vec![Span::styled(String::new(), base_style)]
    } else {
        spans
    }
}

fn inline_markdown_style(base_style: Style, emphasis: bool, strong: bool, code: bool) -> Style {
    let mut style = base_style;
    if emphasis {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if strong {
        style = style.add_modifier(Modifier::BOLD);
    }
    if code {
        style = style.bg(Theme::PANEL_ALT).fg(Theme::ACCENT_STRONG);
    }
    style
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
        ActivityKind::GraphExtract => ("graph", Theme::ACCENT_STRONG),
        ActivityKind::Query => ("query", Theme::ACCENT),
        ActivityKind::QueryError => ("query-error", Theme::DANGER),
        ActivityKind::MemoryReplacement => ("replacement", Theme::WARNING),
        ActivityKind::CaptureTask => ("capture", Theme::ACCENT),
        ActivityKind::Curate => ("curate", Theme::SUCCESS),
        ActivityKind::Reindex => ("reindex", Theme::ACCENT_STRONG),
        ActivityKind::Reembed => ("reembed", Theme::ACCENT_STRONG),
        ActivityKind::Archive => ("archive", Theme::WARNING),
        ActivityKind::DeleteMemory => ("delete", Theme::DANGER),
        ActivityKind::Briefing => ("briefing", Theme::SUCCESS),
        ActivityKind::WatcherHealth => ("watcher-health", Theme::WARNING),
        ActivityKind::Diagnostic => ("diagnostic", Theme::DANGER),
        ActivityKind::LlmAudit => ("llm-audit", Theme::WARNING),
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
    if app.restart_notice.is_some() {
        return "restart";
    }
    match app.ui_status {
        UiStatus::Loading => "loading",
        UiStatus::Busy => "busy",
        UiStatus::Ready => "ready",
        UiStatus::Restart => "restart",
        UiStatus::Error => "error",
    }
}

fn tui_status_color(app: &App) -> Color {
    if app.restart_notice.is_some() {
        return Theme::DANGER;
    }
    match app.ui_status {
        UiStatus::Loading => Theme::ACCENT,
        UiStatus::Busy => Theme::ACCENT_STRONG,
        UiStatus::Ready => Theme::SUCCESS,
        UiStatus::Restart => Theme::DANGER,
        UiStatus::Error => Theme::DANGER,
    }
}

fn tui_status_detail(app: &App) -> Option<String> {
    let count = error_count(app);
    (count > 0).then(|| format!("{count} error{}", if count == 1 { "" } else { "s" }))
}

fn service_status_label(app: &App) -> &'static str {
    if !app.health_ok {
        "down"
    } else {
        let is_relay = matches!(app.service_role.as_deref(), Some("relay"));
        let database_status = app
            .service_database_state
            .as_deref()
            .unwrap_or(app.overview.database_status.as_str());
        let service_status = app
            .service_health_state
            .as_deref()
            .unwrap_or(app.overview.service_status.as_str());
        if !is_relay && !matches!(database_status, "ok" | "up") {
            return "degraded";
        }
        match service_status {
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
        return None;
    }
    let mut parts = Vec::new();
    if let Some(role) = app.service_role.as_deref() {
        parts.push(role.to_string());
    }
    let is_relay = matches!(app.service_role.as_deref(), Some("relay"));
    let database_status = app
        .service_database_state
        .as_deref()
        .unwrap_or(app.overview.database_status.as_str());
    if !is_relay && !matches!(database_status, "ok" | "up") {
        parts.push(format!("db {database_status}"));
    }
    (!parts.is_empty()).then_some(parts.join(", "))
}

fn manager_status_label(app: &App) -> &'static str {
    match app.manager_status.as_ref().map(|status| status.state) {
        Some(ManagerState::Active) => "active",
        Some(ManagerState::Installed) => "installed",
        Some(ManagerState::Off) => "off",
        Some(ManagerState::Error) => "error",
        None => "unknown",
    }
}

fn manager_status_color(app: &App) -> Color {
    match manager_status_label(app) {
        "active" => Theme::SUCCESS,
        "installed" => Theme::WARNING,
        "off" => Theme::MUTED,
        "error" => Theme::DANGER,
        _ => Theme::WARNING,
    }
}

fn manager_status_detail(app: &App) -> Option<String> {
    let status = app.manager_status.as_ref()?;
    let mut parts = Vec::new();
    if let Some(mode) = status.mode {
        parts.push(match mode {
            ManagerMode::Service => "service".to_string(),
            ManagerMode::Foreground => "manual".to_string(),
        });
    }
    if let Some(runtime_mode) = &status.runtime_mode {
        parts.push(runtime_mode.clone());
    }
    if let Some(reason) = &status.last_reconcile_reason {
        parts.push(format!("last {reason}"));
    }
    parts.push(format!(
        "{} session{}",
        status.tracked_sessions,
        if status.tracked_sessions == 1 {
            ""
        } else {
            "s"
        }
    ));
    if status.warning_count > 0 {
        parts.push(format!("{} warn", status.warning_count));
    }
    if status.event_count > 0 || status.fallback_scan_count > 0 {
        parts.push(format!(
            "{} events, {} fallback",
            status.event_count, status.fallback_scan_count
        ));
    }
    Some(parts.join(", "))
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
        "documentation" => Color::Rgb(170, 210, 255),
        "plan" => Color::Rgb(255, 120, 200),
        "implementation" => Color::Rgb(120, 230, 140),
        "all" => Theme::TEXT,
        _ => Theme::TEXT,
    };
    Span::styled(
        label.to_string(),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

fn agent_status_span(status: &AgentSessionStatus) -> Span<'static> {
    let (label, color) = match status {
        AgentSessionStatus::Working => ("working", Theme::SUCCESS),
        AgentSessionStatus::Waiting => ("waiting", Theme::WARNING),
        AgentSessionStatus::Done => ("done", Theme::MUTED),
    };
    Span::styled(
        label.to_string(),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

fn context_percent_style(percent: f64) -> Style {
    let color = if percent >= 90.0 {
        Theme::DANGER
    } else if percent >= 70.0 {
        Theme::WARNING
    } else {
        Theme::SUCCESS
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

fn format_context_percent(percent: f64) -> String {
    if percent.is_finite() && percent > 100.0 {
        "100%+".to_string()
    } else {
        format!("{percent:.0}%")
    }
}

fn normalized_percent(percent: f64) -> f64 {
    if !percent.is_finite() {
        0.0
    } else {
        percent.clamp(0.0, 100.0)
    }
}

fn filled_bar_cells(percent: f64, width: usize) -> usize {
    let width = width.max(1);
    let normalized = normalized_percent(percent);
    ((normalized / 100.0) * width as f64).round() as usize
}

fn remaining_bar_cells(percent_used: f64, width: usize) -> usize {
    let width = width.max(1);
    let remaining = 100.0 - normalized_percent(percent_used);
    ((remaining / 100.0) * width as f64).round() as usize
}

fn interpolate_theme_color(start: Color, end: Color, factor: f64) -> Color {
    let factor = factor.clamp(0.0, 1.0);
    match (start, end) {
        (Color::Rgb(sr, sg, sb), Color::Rgb(er, eg, eb)) => {
            let lerp =
                |s: u8, e: u8| -> u8 { (s as f64 + (e as f64 - s as f64) * factor).round() as u8 };
            Color::Rgb(lerp(sr, er), lerp(sg, eg), lerp(sb, eb))
        }
        _ => end,
    }
}

fn context_gradient_color(percent: f64) -> Color {
    interpolate_theme_color(
        Theme::SUCCESS,
        Theme::DANGER,
        normalized_percent(percent) / 100.0,
    )
}

fn usage_bar_line(
    label: &str,
    percent: f64,
    width: usize,
    suffix: Option<String>,
) -> Line<'static> {
    let width = width.max(1);
    let filled = filled_bar_cells(percent, width).min(width);
    let empty = width.saturating_sub(filled);
    let percent_color = context_gradient_color(percent);
    let mut spans = vec![label_span(format!("{label}: "))];
    for idx in 0..filled {
        let cell_percent = ((idx + 1) as f64 / width as f64) * 100.0;
        spans.push(Span::styled(
            "█",
            Style::default()
                .fg(context_gradient_color(cell_percent))
                .add_modifier(Modifier::BOLD),
        ));
    }
    spans.extend([
        Span::styled("░".repeat(empty), Style::default().fg(Theme::BORDER)),
        Span::raw(" "),
        Span::styled(
            format_context_percent(percent),
            Style::default()
                .fg(percent_color)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    if let Some(suffix) = suffix {
        spans.push(Span::raw("   "));
        spans.push(Span::styled(suffix, Style::default().fg(Theme::MUTED)));
    }
    Line::from(spans)
}

fn quota_bar_line(
    label: &str,
    percent_used: f64,
    width: usize,
    suffix: Option<String>,
) -> Line<'static> {
    let width = width.max(1);
    let remaining_cells = remaining_bar_cells(percent_used, width).min(width);
    let used_cells = width.saturating_sub(remaining_cells);
    let remaining_percent = 100.0 - normalized_percent(percent_used);
    let remaining_style = context_percent_style(100.0 - remaining_percent);
    let mut spans = vec![
        label_span(format!("{label}: ")),
        Span::styled("█".repeat(remaining_cells), remaining_style),
        Span::styled("░".repeat(used_cells), Style::default().fg(Theme::BORDER)),
        Span::raw(" "),
        Span::styled(format!("{remaining_percent:.0}% left"), remaining_style),
    ];
    if let Some(suffix) = suffix {
        spans.push(Span::raw("   "));
        spans.push(Span::styled(suffix, Style::default().fg(Theme::MUTED)));
    }
    Line::from(spans)
}

fn rate_limit_reset_label(resets_at: Option<u64>) -> Option<String> {
    resets_at.map(|resets_at| format!("resets {}", format_epoch_reset_time(resets_at)))
}

fn format_epoch_reset_time(epoch_seconds: u64) -> String {
    let Some(timestamp) = DateTime::<Utc>::from_timestamp(epoch_seconds as i64, 0) else {
        return "n/a".to_string();
    };
    format_timestamp_short(timestamp)
}

fn format_token_count(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

fn agent_task_summary(session: &AgentSession) -> String {
    if let Some(task) = session.current_tasks.first() {
        task.clone()
    } else if !session.initial_prompt.is_empty() {
        session.initial_prompt.clone()
    } else if !session.first_assistant_text.is_empty() {
        session.first_assistant_text.clone()
    } else {
        "no current task".to_string()
    }
}

fn format_agent_child(child: &AgentChildProcess) -> String {
    match child.port {
        Some(port) => format!(
            "- {}  {}  {}  :{}",
            child.pid,
            child.command,
            format_token_count(child.mem_kb / 1024),
            port
        ),
        None => format!(
            "- {}  {}  {}",
            child.pid,
            child.command,
            format_token_count(child.mem_kb / 1024)
        ),
    }
}

fn format_elapsed_from_started(started_at: u64) -> String {
    if started_at == 0 {
        return "n/a".to_string();
    }
    let Some(started_at) = DateTime::<Utc>::from_timestamp_millis(started_at as i64) else {
        return "n/a".to_string();
    };
    let elapsed = Utc::now().signed_duration_since(started_at);
    if elapsed.num_seconds() < 60 {
        format!("{}s", elapsed.num_seconds().max(0))
    } else if elapsed.num_minutes() < 60 {
        format!("{}m", elapsed.num_minutes().max(0))
    } else {
        format!(
            "{}h {}m",
            elapsed.num_hours().max(0),
            elapsed.num_minutes().max(0) % 60
        )
    }
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

fn skill_bundle_status_color(status: SkillBundleStatus) -> Color {
    match status {
        SkillBundleStatus::Ok => Theme::SUCCESS,
        SkillBundleStatus::Warn => Theme::WARNING,
        SkillBundleStatus::Error => Theme::DANGER,
    }
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

fn should_attempt_stream_reconnect(
    stream_connected: bool,
    stream_connecting: bool,
    last_attempt: Instant,
) -> bool {
    !stream_connected && !stream_connecting && last_attempt.elapsed() >= Duration::from_secs(1)
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

fn load_manager_footer_status(profile: Profile) -> ManagerFooterStatus {
    let unit_installed = manager_unit_path(profile).is_some_and(|path| path.exists());
    let unit_enabled = manager_service_enabled(profile);
    let unit_active = manager_service_running(profile);
    let foreground_active = foreground_manager_process_running(profile);
    let state_file = load_manager_state_file(profile);
    let tracked_sessions = state_file
        .as_ref()
        .map(|state| state.sessions.len())
        .unwrap_or(0);
    let warning_count = state_file
        .as_ref()
        .map(|state| state.warnings.len())
        .unwrap_or(0);
    let runtime_mode = state_file
        .as_ref()
        .and_then(|state| (!state.mode.is_empty()).then(|| state.mode.clone()));
    let last_reconcile_reason = state_file.as_ref().and_then(|state| {
        (!state.last_reconcile_reason.is_empty()).then(|| state.last_reconcile_reason.clone())
    });
    let event_count = state_file
        .as_ref()
        .map(|state| state.event_count)
        .unwrap_or(0);
    let fallback_scan_count = state_file
        .as_ref()
        .map(|state| state.fallback_scan_count)
        .unwrap_or(0);
    let state = derive_manager_state(
        unit_installed,
        unit_enabled,
        unit_active,
        foreground_active,
        state_file.is_some() || manager_unit_path(profile).is_some(),
    );
    let mode = if unit_active {
        Some(ManagerMode::Service)
    } else if foreground_active {
        Some(ManagerMode::Foreground)
    } else {
        None
    };
    ManagerFooterStatus {
        state,
        tracked_sessions,
        warning_count,
        mode,
        runtime_mode,
        last_reconcile_reason,
        event_count,
        fallback_scan_count,
    }
}

fn derive_manager_state(
    unit_installed: bool,
    unit_enabled: bool,
    unit_active: bool,
    foreground_active: bool,
    can_probe: bool,
) -> ManagerState {
    if unit_active || foreground_active {
        ManagerState::Active
    } else if unit_installed || unit_enabled {
        ManagerState::Installed
    } else if can_probe {
        ManagerState::Off
    } else {
        ManagerState::Error
    }
}

fn foreground_manager_process_running(profile: Profile) -> bool {
    #[cfg(target_os = "macos")]
    let output = ProcessCommand::new("ps")
        .args(["-ww", "-axo", "pid=,command="])
        .output();

    #[cfg(not(target_os = "macos"))]
    let output = ProcessCommand::new("ps")
        .args(["-ww", "-eo", "pid=,command="])
        .output();

    let Ok(output) = output else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let current_pid = std::process::id().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let Some(pid) = parts.next() else {
            continue;
        };
        if pid == current_pid {
            continue;
        }
        let command = parts.collect::<Vec<_>>().join(" ");
        if command_is_manager_for_profile(&command, profile) {
            return true;
        }
    }
    false
}

fn command_is_manager_for_profile(command: &str, profile: Profile) -> bool {
    if !(command.contains(" watcher manager run")
        || command.ends_with("watcher manager run")
        || command.contains("watcher manager run "))
    {
        return false;
    }
    match profile {
        Profile::Prod => !command_looks_dev_stack(command),
        Profile::Dev => command_looks_dev_stack(command),
    }
}

fn command_looks_dev_stack(command: &str) -> bool {
    command.contains("target/debug/memory")
        || command.contains("target/release/memory")
        || command.contains("MEMORY_LAYER_PROFILE=dev")
        || command.contains("MEMORY_LAYER_PROFILE=\"dev\"")
        || command.contains("MEMORY_LAYER_PROFILE='dev'")
        || command.contains("config.dev.toml")
        || command.contains("/.mem/runtime/dev/")
}

#[cfg(not(target_os = "macos"))]
fn linux_manager_unit_path() -> Option<PathBuf> {
    let config_home = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|home| PathBuf::from(home).join(".config"))
        })?;
    Some(
        config_home
            .join("systemd")
            .join("user")
            .join("memory-watch-manager.service"),
    )
}

fn manager_unit_path(profile: Profile) -> Option<PathBuf> {
    if profile == Profile::Dev {
        return None;
    }

    #[cfg(target_os = "macos")]
    {
        mem_platform::watch_manager_launch_agent_path()
    }

    #[cfg(not(target_os = "macos"))]
    {
        linux_manager_unit_path()
    }
}

fn load_manager_state_file(profile: Profile) -> Option<ManagerStateFile> {
    let filename = match profile {
        Profile::Dev => "watcher-manager-state-dev.json",
        Profile::Prod => "watcher-manager-state.json",
    };
    let path = preferred_user_state_dir()?.join(filename);
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

#[cfg(not(target_os = "macos"))]
fn systemctl_user_check(action: &str, unit: &str) -> bool {
    ProcessCommand::new("systemctl")
        .args(["--user", action, unit])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn manager_service_enabled(profile: Profile) -> bool {
    if profile == Profile::Dev {
        return false;
    }

    #[cfg(target_os = "macos")]
    {
        let Some(path) = mem_platform::watch_manager_launch_agent_path() else {
            return false;
        };
        path.exists()
    }

    #[cfg(not(target_os = "macos"))]
    {
        systemctl_user_check("is-enabled", "memory-watch-manager.service")
    }
}

fn manager_service_running(profile: Profile) -> bool {
    if profile == Profile::Dev {
        return false;
    }

    #[cfg(target_os = "macos")]
    {
        let Some(label) = Some(mem_platform::watch_manager_launch_agent_label()) else {
            return false;
        };
        launchctl_print_succeeds(label)
    }

    #[cfg(not(target_os = "macos"))]
    {
        systemctl_user_check("is-active", "memory-watch-manager.service")
    }
}

#[cfg(target_os = "macos")]
fn launchctl_print_succeeds(label: &str) -> bool {
    let Ok(output) = ProcessCommand::new("id").arg("-u").output() else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let uid = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let target = format!("gui/{uid}/{label}");
    ProcessCommand::new("launchctl")
        .args(["print", &target])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn detect_tool_versions(profile: Profile) -> ToolVersions {
    let version = profile.display_version(env!("CARGO_PKG_VERSION"));
    ToolVersions {
        mem_cli: version.clone(),
        mem_service: version.clone(),
        watch_manager: version.clone(),
        memory_watch: version,
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
    use crossterm::event::{KeyCode, KeyEvent};

    #[cfg_attr(target_os = "macos", allow(unused_imports))]
    use super::{
        AgentSnapshot, App, BackendConnectionState, BackgroundEvent, ManagerState, MemoriesFocus,
        ProjectRefreshResult, QueryHistoryEntry, QueryRoundtripTiming, RefreshMode, TabKind, Theme,
        ToolVersions, UiStatus, activity_duration, activity_tokens, backend_activity_detail_lines,
        build_memory_detail_lines, collect_error_items, context_gradient_color,
        current_query_display, derive_manager_state, empty_overview, filled_bar_cells,
        format_context_percent, format_epoch_reset_time, format_query_citation_numbers,
        format_query_timing_with_percent, format_timestamp, format_timestamp_full,
        format_timestamp_medium, format_timestamp_short, format_timestamp_timeline,
        latest_plan_display, llm_audit_status_lines, manager_service_enabled,
        manager_service_running, manager_status_detail, manager_status_label, manager_unit_path,
        memory_detail_max_scroll, normalized_percent, query_input_display, query_timing_breakdown,
        query_timing_breakdown_lines, remaining_bar_cells, render_markdown_lines,
        service_status_detail, service_status_label, should_attempt_stream_reconnect,
        skill_bundle_status_color, tui_status_color, tui_status_detail, tui_status_label,
        watcher_bar_status_label,
    };
    use crate::{SkillBundleStatus, TuiRestartNotice, project_skill_inventory};
    use mem_agenttop::{AgentSession, SessionStatus as AgentSessionStatus};
    use mem_api::{
        ActivityDetails, ActivityEvent, ActivityKind, DiagnosticInfo, DiagnosticSeverity,
        LlmAuditMessage, LlmAuditStatusResponse, MemoryEmbeddingSpace, MemoryEntryResponse,
        MemoryStatus, MemoryType, Profile, ProjectMemoriesResponse, QueryAnswerGeneration,
        QueryAnswerMethod, QueryDiagnostics, QueryFilters, QueryMatchKind, QueryRequest,
        QueryResponse, QueryResult, QueryResultDebug, ReplacementProposalListResponse, TokenUsage,
        WatcherPresenceSummary,
    };
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};
    use tokio::sync::mpsc;
    use uuid::Uuid;

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
    fn documentation_memory_type_filter_matches_and_labels() {
        let filter = super::TypeFilter::Documentation;

        assert!(filter.matches(&MemoryType::Documentation));
        assert!(!filter.matches(&MemoryType::Implementation));
        assert_eq!(filter.label(), "documentation");
        assert_eq!(
            super::TypeFilter::DomainFact.next().label(),
            "documentation"
        );
    }

    #[test]
    fn every_visible_tab_has_comprehensive_help() {
        for tab in super::VISIBLE_TABS {
            let help = super::tab_help_markdown(tab);
            assert!(help.contains("# "));
            assert!(
                help.contains("## Purpose"),
                "{} missing purpose",
                tab.label()
            );
            assert!(help.contains("## Layout"), "{} missing layout", tab.label());
            assert!(
                help.contains("## Controls"),
                "{} missing controls",
                tab.label()
            );
            assert!(
                help.contains("## Workflows"),
                "{} missing workflows",
                tab.label()
            );
            assert!(super::tab_help_lines(tab).len() > 12);
        }
    }

    #[test]
    fn help_open_and_close_preserve_active_tab() {
        let mut app = new_test_app();
        app.active_tab = TabKind::Query;

        app.open_help_for_active_tab();
        assert!(app.help_open);
        assert_eq!(app.help_tab, TabKind::Query);
        assert_eq!(app.active_tab, TabKind::Query);

        app.handle_help_key(KeyEvent::from(KeyCode::Char('h')));
        assert!(!app.help_open);
        assert_eq!(app.active_tab, TabKind::Query);
    }

    #[test]
    fn help_scroll_is_clamped_to_rendered_content() {
        let mut app = new_test_app();
        app.active_tab = TabKind::Embeddings;
        app.open_help_for_active_tab();
        let frame = ratatui::layout::Rect::new(0, 0, 100, 24);
        let max_scroll = super::help_max_scroll(app.help_tab, frame);
        assert!(max_scroll > 0);

        app.scroll_help_in_area(500, frame);
        assert_eq!(app.help_scroll, max_scroll);

        app.scroll_help_in_area(-500, frame);
        assert_eq!(app.help_scroll, 0);
    }

    #[test]
    fn help_can_open_when_backend_is_unavailable() {
        let mut app = new_test_app();
        app.health_ok = false;
        app.active_tab = TabKind::Errors;

        app.open_help_for_active_tab();

        assert!(app.help_open);
        assert_eq!(app.help_tab, TabKind::Errors);
    }

    #[test]
    fn h_is_no_longer_previous_tab_alias() {
        let mut app = new_test_app();
        app.active_tab = TabKind::Query;

        app.open_help_for_active_tab();

        assert_eq!(app.active_tab, TabKind::Query);
        assert_eq!(app.help_tab, TabKind::Query);
    }

    #[test]
    fn query_citation_numbers_render_bracketed_result_ids() {
        assert_eq!(format_query_citation_numbers(&[]), "none");
        assert_eq!(format_query_citation_numbers(&[1, 3]), "[1] [3]");
    }

    fn test_query_response_with_timings() -> QueryResponse {
        QueryResponse {
            answer: "Use the selected memory. [1]".to_string(),
            confidence: 0.82,
            results: vec![QueryResult {
                memory_id: Uuid::new_v4(),
                summary: "Cached implementation memory".to_string(),
                memory_type: MemoryType::Implementation,
                score: 12.5,
                snippet: "Cached result snippet".to_string(),
                match_kind: QueryMatchKind::Hybrid,
                score_explanation: vec!["strong cached match".to_string()],
                debug: QueryResultDebug::default(),
                tags: vec!["implementation".to_string()],
                sources: Vec::new(),
                graph_connections: Vec::new(),
            }],
            insufficient_evidence: false,
            answer_generation: QueryAnswerGeneration {
                method: QueryAnswerMethod::Llm,
                cited_result_numbers: vec![1],
                evidence_count: 1,
                duration_ms: 80,
                fallback_reason: None,
                token_usage: None,
            },
            answer_citations: Vec::new(),
            diagnostics: QueryDiagnostics {
                total_duration_ms: 300,
                lexical_duration_ms: 40,
                semantic_duration_ms: 70,
                graph_duration_ms: 120,
                rerank_duration_ms: 30,
                lexical_candidates: 11,
                semantic_candidates: 7,
                graph_candidates: 3,
                semantic_status: "active_space_ok".to_string(),
                graph_status: "active".to_string(),
                ..Default::default()
            },
        }
    }

    fn test_query_request(query: &str) -> QueryRequest {
        QueryRequest {
            project: "memory".to_string(),
            query: query.to_string(),
            filters: QueryFilters::default(),
            top_k: 8,
            min_confidence: None,
            history: false,
            retrieval_mode: None,
            answer_mode: None,
        }
    }

    #[test]
    fn query_timing_breakdown_saturates_derived_values() {
        let response = test_query_response_with_timings();
        let timing = QueryRoundtripTiming {
            query_api_ms: 360,
            initial_detail_ms: Some(25),
            ui_ready_ms: 390,
        };

        let breakdown = query_timing_breakdown(&response, timing);

        assert_eq!(breakdown.backend_reported_ms, 380);
        assert_eq!(breakdown.transport_overhead_ms, 0);
        assert_eq!(breakdown.retrieval_other_ms, 40);
    }

    #[test]
    fn query_timing_percent_formats_consistently() {
        assert_eq!(format_query_timing_with_percent(25, 100), "25 ms (25%)");
        assert_eq!(format_query_timing_with_percent(25, 0), "25 ms");
    }

    #[test]
    fn query_timing_lines_render_roundtrip_and_phase_labels() {
        let response = test_query_response_with_timings();
        let timing = QueryRoundtripTiming {
            query_api_ms: 420,
            initial_detail_ms: Some(30),
            ui_ready_ms: 455,
        };

        let rendered = query_timing_breakdown_lines(&response, Some(timing))
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("Timing Breakdown"));
        assert!(rendered.contains("UI ready"));
        assert!(rendered.contains("Query API"));
        assert!(rendered.contains("Initial detail"));
        assert!(rendered.contains("Backend"));
        assert!(rendered.contains("Answer"));
        assert!(rendered.contains("Lexical"));
        assert!(rendered.contains("Semantic"));
        assert!(rendered.contains("Graph"));
        assert!(rendered.contains("Rerank/relation"));
    }

    #[test]
    fn latest_plan_display_shows_recent_plan_thread() {
        let mut app = new_test_app();
        let older = Utc.with_ymd_and_hms(2026, 4, 1, 12, 0, 0).unwrap();
        let newer = Utc.with_ymd_and_hms(2026, 5, 1, 12, 0, 0).unwrap();
        app.all_memories = vec![
            test_project_memory_list_item("Implemented work", MemoryType::Implementation, newer),
            test_project_memory_list_item("Older Plan", MemoryType::Plan, older),
            test_project_memory_list_item("Latest Plan", MemoryType::Plan, newer),
        ];
        app.all_memories[2]
            .tags
            .push("plan-thread:latest-plan".to_string());

        assert_eq!(latest_plan_display(&app), "Latest Plan (latest-plan)");
    }

    #[test]
    fn query_input_display_renders_placeholder_and_cursor_start() {
        let display = query_input_display("", 12);
        assert!(display.placeholder);
        assert_eq!(display.text, "Ask project ");
        assert_eq!(display.cursor_col, 0);
    }

    #[test]
    fn query_input_display_keeps_short_cursor_after_text() {
        let display = query_input_display("hello", 12);
        assert!(!display.placeholder);
        assert_eq!(display.text, "hello");
        assert_eq!(display.cursor_col, 5);
    }

    #[test]
    fn query_input_display_truncates_long_text_from_left() {
        let display = query_input_display("long question", 8);
        assert!(!display.placeholder);
        assert_eq!(display.text, "<uestion");
        assert_eq!(display.cursor_col, 7);
    }

    #[test]
    fn stale_query_completion_does_not_replace_current_query_state() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut app = App::new(
            "memory".to_string(),
            PathBuf::from("/tmp/memory"),
            ToolVersions {
                mem_cli: "0.6.2".to_string(),
                mem_service: "0.6.2".to_string(),
                watch_manager: "0.6.2".to_string(),
                memory_watch: "0.6.2".to_string(),
            },
            false,
            Profile::Prod,
            tx,
        );
        app.query_request_id = 2;
        app.query_loading = true;
        app.query_pending_question = Some("newer query".to_string());
        let request = QueryRequest {
            project: "memory".to_string(),
            query: "older query".to_string(),
            filters: QueryFilters::default(),
            top_k: 8,
            min_confidence: None,
            history: false,
            retrieval_mode: None,
            answer_mode: None,
        };

        app.apply_query_completed(
            1,
            request,
            QueryRoundtripTiming {
                query_api_ms: 12,
                initial_detail_ms: None,
                ui_ready_ms: 12,
            },
            Err("older query failed".to_string()),
            None,
        );

        assert!(app.query_loading);
        assert_eq!(app.query_pending_question.as_deref(), Some("newer query"));
        assert!(app.query_error.is_none());
    }

    #[test]
    fn query_completion_stores_roundtrip_timing() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut app = App::new(
            "memory".to_string(),
            PathBuf::from("/tmp/memory"),
            ToolVersions {
                mem_cli: "0.8.6".to_string(),
                mem_service: "0.8.6".to_string(),
                watch_manager: "0.8.6".to_string(),
                memory_watch: "0.8.6".to_string(),
            },
            false,
            Profile::Prod,
            tx,
        );
        app.query_request_id = 1;
        app.query_loading = true;
        let request = QueryRequest {
            project: "memory".to_string(),
            query: "timing".to_string(),
            filters: QueryFilters::default(),
            top_k: 8,
            min_confidence: None,
            history: false,
            retrieval_mode: None,
            answer_mode: None,
        };
        let timing = QueryRoundtripTiming {
            query_api_ms: 420,
            initial_detail_ms: Some(35),
            ui_ready_ms: 460,
        };

        app.apply_query_completed(
            1,
            request,
            timing,
            Ok(test_query_response_with_timings()),
            None,
        );

        assert!(!app.query_loading);
        assert_eq!(app.query_roundtrip_timing, Some(timing));
        assert_eq!(app.query_last_duration_ms, Some(460));
    }

    #[test]
    fn query_completion_stores_success_snapshot_in_history() {
        let mut app = new_test_app();
        app.query_request_id = 1;
        app.query_text = "cached success".to_string();
        app.start_query_history_run("cached success");
        let timing = QueryRoundtripTiming {
            query_api_ms: 210,
            initial_detail_ms: Some(20),
            ui_ready_ms: 235,
        };
        let detail = test_memory_detail("Cached canonical detail");

        app.apply_query_completed(
            1,
            test_query_request("cached success"),
            timing,
            Ok(test_query_response_with_timings()),
            Some(Ok(detail.clone())),
        );

        assert_eq!(app.query_history.len(), 1);
        let entry = &app.query_history[0];
        assert_eq!(entry.question, "cached success");
        assert!(entry.response.is_some());
        assert!(entry.error.is_none());
        assert_eq!(entry.timing, Some(timing));
        assert_eq!(
            entry
                .initial_detail
                .as_ref()
                .map(|detail| detail.canonical_text.as_str()),
            Some("Cached canonical detail")
        );
        assert!(!entry.running);
    }

    #[test]
    fn query_history_up_restores_cached_success_results() {
        let mut app = new_test_app();
        let timing = QueryRoundtripTiming {
            query_api_ms: 210,
            initial_detail_ms: Some(20),
            ui_ready_ms: 235,
        };
        app.query_history.push(QueryHistoryEntry {
            question: "cached success".to_string(),
            response: Some(test_query_response_with_timings()),
            error: None,
            timing: Some(timing),
            initial_detail: Some(test_memory_detail("Restored canonical detail")),
            running: false,
        });
        app.clear_visible_query_state();

        let mut buffer = String::new();
        app.apply_query_history_delta(&mut buffer, -1);

        assert_eq!(buffer, "cached success");
        assert_eq!(app.query_text, "cached success");
        assert!(app.query_response.is_some());
        assert!(app.query_error.is_none());
        assert_eq!(app.query_roundtrip_timing, Some(timing));
        assert_eq!(app.query_table_state.selected(), Some(0));
        assert_eq!(
            app.query_selected_detail
                .as_ref()
                .map(|detail| detail.canonical_text.as_str()),
            Some("Restored canonical detail")
        );
        assert!(app.status_message.contains("with cached results"));
    }

    #[test]
    fn query_history_up_restores_cached_error() {
        let mut app = new_test_app();
        let timing = QueryRoundtripTiming {
            query_api_ms: 90,
            initial_detail_ms: None,
            ui_ready_ms: 90,
        };
        app.query_history.push(QueryHistoryEntry {
            question: "broken query".to_string(),
            response: None,
            error: Some("provider unavailable".to_string()),
            timing: Some(timing),
            initial_detail: Some(test_memory_detail("stale detail should not show")),
            running: false,
        });
        app.query_response = Some(test_query_response_with_timings());

        let mut buffer = String::new();
        app.apply_query_history_delta(&mut buffer, -1);

        assert_eq!(buffer, "broken query");
        assert!(app.query_response.is_none());
        assert_eq!(app.query_error.as_deref(), Some("provider unavailable"));
        assert_eq!(app.query_roundtrip_timing, Some(timing));
        assert!(app.query_selected_detail.is_none());
        assert_eq!(app.query_table_state.selected(), None);
        assert!(app.status_message.contains("with cached error"));
    }

    #[test]
    fn query_history_down_to_empty_clears_visible_results() {
        let mut app = new_test_app();
        app.query_history.push(QueryHistoryEntry {
            question: "cached success".to_string(),
            response: Some(test_query_response_with_timings()),
            error: None,
            timing: Some(QueryRoundtripTiming {
                query_api_ms: 210,
                initial_detail_ms: Some(20),
                ui_ready_ms: 235,
            }),
            initial_detail: Some(test_memory_detail("Restored canonical detail")),
            running: false,
        });
        let mut buffer = String::new();
        app.apply_query_history_delta(&mut buffer, -1);
        assert!(app.query_response.is_some());

        app.apply_query_history_delta(&mut buffer, 1);

        assert_eq!(buffer, "");
        assert!(app.query_response.is_none());
        assert!(app.query_error.is_none());
        assert!(app.query_roundtrip_timing.is_none());
        assert!(app.query_selected_detail.is_none());
        assert_eq!(app.query_table_state.selected(), None);
    }

    #[test]
    fn empty_query_submit_clears_results_and_prompts_for_question() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut app = App::new(
            "memory".to_string(),
            PathBuf::from("/tmp/memory"),
            ToolVersions {
                mem_cli: "0.6.2".to_string(),
                mem_service: "0.6.2".to_string(),
                watch_manager: "0.6.2".to_string(),
                memory_watch: "0.6.2".to_string(),
            },
            false,
            Profile::Prod,
            tx,
        );
        app.query_text = "   ".to_string();
        app.query_loading = true;

        assert!(app.clear_empty_query_if_needed());

        assert!(!app.query_loading);
        assert_eq!(app.status_message, "Enter a query before running search.");
        assert!(app.query_response.is_none());
        assert!(app.query_error.is_none());
    }

    #[test]
    fn query_input_starts_empty_and_history_navigates_with_arrows() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut app = App::new(
            "memory".to_string(),
            PathBuf::from("/tmp/memory"),
            ToolVersions {
                mem_cli: "0.6.3".to_string(),
                mem_service: "0.6.3".to_string(),
                watch_manager: "0.6.3".to_string(),
                memory_watch: "0.6.3".to_string(),
            },
            false,
            Profile::Prod,
            tx,
        );

        app.query_text = "previous visible query".to_string();
        app.start_query_input();
        assert_eq!(current_query_display(&app), "");

        app.query_text = "first query".to_string();
        app.remember_query_history_entry();
        app.query_text = "second query".to_string();
        app.remember_query_history_entry();

        let mut buffer = String::new();
        app.apply_query_history_delta(&mut buffer, -1);
        assert_eq!(buffer, "second query");
        app.apply_query_history_delta(&mut buffer, -1);
        assert_eq!(buffer, "first query");
        app.apply_query_history_delta(&mut buffer, 1);
        assert_eq!(buffer, "second query");
        app.apply_query_history_delta(&mut buffer, 1);
        assert_eq!(buffer, "");
    }

    #[test]
    fn footer_statuses_do_not_use_stale_service_or_watcher_state_when_health_is_down() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut app = App::new(
            "memory".to_string(),
            PathBuf::from("/tmp/memory"),
            ToolVersions {
                mem_cli: "0.4.3".to_string(),
                mem_service: "0.4.3".to_string(),
                watch_manager: "0.4.3".to_string(),
                memory_watch: "0.4.3".to_string(),
            },
            false,
            Profile::Prod,
            tx,
        );
        app.ui_status = UiStatus::Error;
        app.health_ok = false;
        app.backend_connection_state = BackendConnectionState::Unavailable;
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
    fn footer_service_status_treats_healthy_relay_as_up() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut app = App::new(
            "memory".to_string(),
            PathBuf::from("/tmp/memory"),
            ToolVersions {
                mem_cli: "0.4.3".to_string(),
                mem_service: "0.4.3".to_string(),
                watch_manager: "0.4.3".to_string(),
                memory_watch: "0.4.3".to_string(),
            },
            false,
            Profile::Prod,
            tx,
        );
        app.health_ok = true;
        app.service_role = Some("relay".to_string());
        app.service_health_state = Some("ok".to_string());
        app.service_database_state = Some("down".to_string());
        app.overview.service_status = "ok".to_string();
        app.overview.database_status = "down".to_string();

        assert_eq!(service_status_label(&app), "up");
        assert_eq!(service_status_detail(&app), Some("relay".to_string()));
    }

    #[test]
    fn backend_connection_state_starts_connecting_then_tracks_health() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut app = App::new(
            "memory".to_string(),
            PathBuf::from("/tmp/memory"),
            ToolVersions {
                mem_cli: "0.6.3".to_string(),
                mem_service: "0.6.3".to_string(),
                watch_manager: "0.6.3".to_string(),
                memory_watch: "0.6.3".to_string(),
            },
            false,
            Profile::Prod,
            tx,
        );

        assert_eq!(
            app.backend_connection_state,
            BackendConnectionState::Connecting
        );

        let mut result = ProjectRefreshResult {
            mode: RefreshMode::Startup,
            health: Ok(serde_json::json!({
                "status": "ok",
                "database": "up",
                "role": "primary",
                "version": "0.6.3"
            })),
            overview: Ok(empty_overview("memory".to_string())),
            memories: Ok(ProjectMemoriesResponse {
                project: "memory".to_string(),
                total: 0,
                items: Vec::new(),
            }),
            proposals: Ok(ReplacementProposalListResponse {
                project: "memory".to_string(),
                proposals: Vec::new(),
            }),
            skill_inventory: project_skill_inventory(Path::new("."), false),
        };
        app.apply_project_refresh(result.clone());
        assert_eq!(
            app.backend_connection_state,
            BackendConnectionState::Connected
        );

        result.health = Err("connection refused".to_string());
        app.apply_project_refresh(result);
        assert_eq!(
            app.backend_connection_state,
            BackendConnectionState::Unavailable
        );
    }

    #[test]
    fn project_refresh_selects_first_memory_for_detail_loading() {
        let mut app = new_test_app();
        let updated_at = Utc.with_ymd_and_hms(2026, 5, 3, 18, 0, 0).unwrap();
        let first = test_project_memory_list_item("First memory", MemoryType::Project, updated_at);
        let first_id = first.id;
        let second =
            test_project_memory_list_item("Second memory", MemoryType::Implementation, updated_at);

        let loaded = app.apply_project_refresh(ProjectRefreshResult {
            mode: RefreshMode::Startup,
            health: Ok(serde_json::json!({
                "status": "ok",
                "database": "up",
                "role": "primary",
                "version": "0.8.2"
            })),
            overview: Ok(empty_overview("memory".to_string())),
            memories: Ok(ProjectMemoriesResponse {
                project: "memory".to_string(),
                total: 2,
                items: vec![first, second],
            }),
            proposals: Ok(ReplacementProposalListResponse {
                project: "memory".to_string(),
                proposals: Vec::new(),
            }),
            skill_inventory: project_skill_inventory(Path::new("."), false),
        });

        assert!(loaded);
        assert_eq!(app.selected_index, 0);
        assert_eq!(app.table_state.selected(), Some(0));
        assert_eq!(
            app.filtered_memories.first().map(|item| item.id),
            Some(first_id)
        );
    }

    #[test]
    fn manager_footer_status_mapping_prefers_active_then_installed_then_off() {
        assert_eq!(
            derive_manager_state(true, true, true, false, true),
            ManagerState::Active
        );
        assert_eq!(
            derive_manager_state(true, false, false, false, true),
            ManagerState::Installed
        );
        assert_eq!(
            derive_manager_state(false, false, false, true, true),
            ManagerState::Active
        );
        assert_eq!(
            derive_manager_state(false, false, false, false, true),
            ManagerState::Off
        );
        assert_eq!(
            derive_manager_state(false, false, false, false, false),
            ManagerState::Error
        );
    }

    #[test]
    fn dev_manager_status_ignores_installed_service_probe() {
        assert_eq!(manager_unit_path(Profile::Dev), None);
        assert!(!manager_service_enabled(Profile::Dev));
        assert!(!manager_service_running(Profile::Dev));
    }

    #[test]
    fn manager_process_detection_is_profile_scoped() {
        let prod = "/home/user/.local/bin/memory --config /home/user/.config/memory-layer/memory-layer.toml watcher manager run";
        let dev = "/home/user/project/target/debug/memory watcher manager run";
        let explicit_dev =
            "MEMORY_LAYER_PROFILE=dev /home/user/.local/bin/memory watcher manager run";

        assert!(super::command_is_manager_for_profile(prod, Profile::Prod));
        assert!(!super::command_is_manager_for_profile(prod, Profile::Dev));
        assert!(super::command_is_manager_for_profile(dev, Profile::Dev));
        assert!(!super::command_is_manager_for_profile(dev, Profile::Prod));
        assert!(super::command_is_manager_for_profile(
            explicit_dev,
            Profile::Dev
        ));
    }

    #[test]
    fn manager_footer_detail_includes_session_and_warning_counts() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut app = App::new(
            "memory".to_string(),
            PathBuf::from("/tmp/memory"),
            ToolVersions {
                mem_cli: "0.4.3".to_string(),
                mem_service: "0.4.3".to_string(),
                watch_manager: "0.4.3".to_string(),
                memory_watch: "0.4.3".to_string(),
            },
            false,
            Profile::Prod,
            tx,
        );
        app.manager_status = Some(super::ManagerFooterStatus {
            state: ManagerState::Active,
            tracked_sessions: 2,
            warning_count: 1,
            mode: Some(super::ManagerMode::Foreground),
            runtime_mode: Some("event-driven".to_string()),
            last_reconcile_reason: Some("session-file-event".to_string()),
            event_count: 3,
            fallback_scan_count: 1,
        });

        assert_eq!(manager_status_label(&app), "active");
        assert_eq!(
            manager_status_detail(&app),
            Some(
                "manual, event-driven, last session-file-event, 2 sessions, 1 warn, 3 events, 1 fallback"
                    .to_string()
            )
        );
    }

    #[test]
    fn stream_reconnect_attempts_are_rate_limited() {
        let just_attempted = Instant::now();
        assert!(!should_attempt_stream_reconnect(
            false,
            false,
            just_attempted
        ));

        let overdue = Instant::now() - Duration::from_secs(2);
        assert!(should_attempt_stream_reconnect(false, false, overdue));
        assert!(!should_attempt_stream_reconnect(true, false, overdue));
        assert!(!should_attempt_stream_reconnect(false, true, overdue));
    }

    #[test]
    fn stream_disconnect_does_not_mark_backend_unavailable() {
        let mut app = new_test_app();
        app.health_ok = true;
        app.backend_connection_state = BackendConnectionState::Connected;
        app.ui_status = UiStatus::Ready;
        app.overview.service_status = "ok".to_string();
        app.overview.database_status = "up".to_string();

        app.handle_stream_disconnect("stream connection closed");

        assert!(app.health_ok);
        assert_eq!(
            app.backend_connection_state,
            BackendConnectionState::Connected
        );
        assert_eq!(app.overview.service_status, "ok");
        assert_eq!(app.overview.database_status, "up");
        assert_eq!(app.ui_status, UiStatus::Ready);
        assert!(app.status_message.contains("backend health is unchanged"));
    }

    #[test]
    fn tui_restart_notice_forces_red_restart_status() {
        let mut app = new_test_app();
        app.ui_status = UiStatus::Ready;
        app.restart_notice = Some(TuiRestartNotice {
            marker_path: PathBuf::from("/tmp/tui-restart-required.json"),
            version: "0.9.0".to_string(),
            reason: "install-or-upgrade".to_string(),
        });

        assert_eq!(tui_status_label(&app), "restart");
        assert_eq!(tui_status_color(&app), Theme::DANGER);
    }

    #[test]
    fn context_percent_display_caps_over_budget_sessions() {
        assert_eq!(format_context_percent(68.4), "68%");
        assert_eq!(format_context_percent(100.0), "100%");
        assert_eq!(format_context_percent(182.3), "100%+");
    }

    #[test]
    fn bar_helpers_normalize_and_cap_percentages() {
        assert_eq!(normalized_percent(-10.0), 0.0);
        assert_eq!(normalized_percent(42.5), 42.5);
        assert_eq!(normalized_percent(182.3), 100.0);
        assert_eq!(filled_bar_cells(0.0, 20), 0);
        assert_eq!(filled_bar_cells(50.0, 20), 10);
        assert_eq!(filled_bar_cells(182.3, 20), 20);
        assert_eq!(remaining_bar_cells(0.0, 20), 20);
        assert_eq!(remaining_bar_cells(50.0, 20), 10);
        assert_eq!(remaining_bar_cells(100.0, 20), 0);
    }

    #[test]
    fn epoch_reset_time_formats_in_local_timezone() {
        let epoch_seconds = 1_775_352_000_u64;
        let timestamp = Utc.timestamp_opt(epoch_seconds as i64, 0).unwrap();
        assert_eq!(
            format_epoch_reset_time(epoch_seconds),
            format_timestamp_short(timestamp)
        );
    }

    #[test]
    fn context_gradient_spans_success_to_danger() {
        assert_eq!(context_gradient_color(0.0), Theme::SUCCESS);
        assert_eq!(context_gradient_color(100.0), Theme::DANGER);
    }

    #[test]
    fn skill_bundle_status_colors_match_footer_severity() {
        assert_eq!(
            skill_bundle_status_color(SkillBundleStatus::Ok),
            Theme::SUCCESS
        );
        assert_eq!(
            skill_bundle_status_color(SkillBundleStatus::Warn),
            Theme::WARNING
        );
        assert_eq!(
            skill_bundle_status_color(SkillBundleStatus::Error),
            Theme::DANGER
        );
    }

    fn test_agent_session(project_name: &str, session_id: &str) -> AgentSession {
        AgentSession {
            agent_cli: "codex",
            pid: 123,
            session_id: session_id.to_string(),
            cwd: format!("/tmp/{project_name}"),
            project_name: project_name.to_string(),
            started_at: 0,
            status: AgentSessionStatus::Waiting,
            model: "gpt-5.4".to_string(),
            context_percent: 42.0,
            total_input_tokens: 100,
            total_output_tokens: 20,
            total_cache_read: 0,
            total_cache_create: 0,
            turn_count: 1,
            current_tasks: vec!["waiting for input".to_string()],
            mem_mb: 128,
            version: "0.4.3".to_string(),
            git_branch: "main".to_string(),
            git_added: 0,
            git_modified: 0,
            token_history: vec![],
            subagents: vec![],
            mem_file_count: 0,
            mem_line_count: 0,
            children: vec![],
            initial_prompt: String::new(),
            first_assistant_text: String::new(),
        }
    }

    fn test_memory_detail(canonical_text: &str) -> MemoryEntryResponse {
        let timestamp = Utc.with_ymd_and_hms(2026, 4, 11, 8, 0, 0).unwrap();
        MemoryEntryResponse {
            id: Uuid::nil(),
            project: "memory".to_string(),
            canonical_text: canonical_text.to_string(),
            summary: "Improved TUI detail rendering".to_string(),
            memory_type: MemoryType::Implementation,
            importance: 8,
            confidence: 0.92,
            status: MemoryStatus::Active,
            tags: vec!["implementation".to_string(), "tui".to_string()],
            sources: Vec::new(),
            related_memories: Vec::new(),
            embedding_spaces: Vec::new(),
            created_at: timestamp,
            updated_at: timestamp,
            canonical_id: Uuid::nil(),
            version_no: 1,
            is_tombstone: false,
        }
    }

    fn test_project_memory_list_item(
        summary: &str,
        memory_type: MemoryType,
        updated_at: chrono::DateTime<Utc>,
    ) -> mem_api::ProjectMemoryListItem {
        let id = Uuid::new_v4();
        mem_api::ProjectMemoryListItem {
            id,
            summary: summary.to_string(),
            preview: summary.to_string(),
            memory_type,
            status: MemoryStatus::Active,
            confidence: 0.95,
            importance: 4,
            updated_at,
            tags: Vec::new(),
            tag_count: 0,
            source_count: 0,
            canonical_id: id,
            version_no: 1,
            is_tombstone: false,
        }
    }

    #[test]
    fn agents_tab_initial_selection_prefers_current_project() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut app = App::new(
            "memory".to_string(),
            PathBuf::from("/tmp/memory"),
            ToolVersions {
                mem_cli: "0.4.3".to_string(),
                mem_service: "0.4.3".to_string(),
                watch_manager: "0.4.3".to_string(),
                memory_watch: "0.4.3".to_string(),
            },
            false,
            Profile::Prod,
            tx,
        );
        let snapshot = AgentSnapshot {
            collected_at: Utc::now(),
            sessions: vec![
                test_agent_session("other-project", "session-a"),
                test_agent_session("memory", "session-b"),
            ],
            orphan_ports: vec![],
            rate_limits: vec![],
        };

        app.apply_background_event(BackgroundEvent::AgentsLoaded {
            snapshot: Ok(snapshot),
        });

        assert_eq!(app.agent_selected_index, 1);
        assert_eq!(app.agent_table_state.selected(), Some(1));
    }

    #[test]
    fn agents_tab_initial_selection_falls_back_to_first_row_without_match() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut app = App::new(
            "memory".to_string(),
            PathBuf::from("/tmp/memory"),
            ToolVersions {
                mem_cli: "0.4.3".to_string(),
                mem_service: "0.4.3".to_string(),
                watch_manager: "0.4.3".to_string(),
                memory_watch: "0.4.3".to_string(),
            },
            false,
            Profile::Prod,
            tx,
        );
        let snapshot = AgentSnapshot {
            collected_at: Utc::now(),
            sessions: vec![
                test_agent_session("alpha", "session-a"),
                test_agent_session("beta", "session-b"),
            ],
            orphan_ports: vec![],
            rate_limits: vec![],
        };

        app.apply_background_event(BackgroundEvent::AgentsLoaded {
            snapshot: Ok(snapshot),
        });

        assert_eq!(app.agent_selected_index, 0);
        assert_eq!(app.agent_table_state.selected(), Some(0));
    }

    #[test]
    fn tab_order_restores_activity_after_query() {
        assert_eq!(TabKind::Memories.index(), 0);
        assert_eq!(TabKind::Agents.index(), 1);
        assert_eq!(TabKind::Query.index(), 2);
        assert_eq!(TabKind::Activity.index(), 3);
        assert_eq!(TabKind::Errors.index(), 4);
        assert_eq!(TabKind::Project.index(), 5);
        assert_eq!(TabKind::Review.index(), 6);
        assert_eq!(TabKind::Watchers.index(), 7);
        assert_eq!(TabKind::Embeddings.index(), 8);
        assert_eq!(TabKind::Resume.index(), 9);

        assert_eq!(TabKind::Memories.prev(), TabKind::Resume);
        assert_eq!(TabKind::Memories.next(), TabKind::Agents);
        assert_eq!(TabKind::Query.next(), TabKind::Activity);
        assert_eq!(TabKind::Activity.prev(), TabKind::Query);
        assert_eq!(TabKind::Activity.next(), TabKind::Errors);
        assert_eq!(TabKind::Errors.prev(), TabKind::Activity);
        assert_eq!(TabKind::Errors.next(), TabKind::Project);
        assert_eq!(TabKind::Project.prev(), TabKind::Errors);
        assert_eq!(TabKind::Project.next(), TabKind::Review);
        assert_eq!(TabKind::Review.prev(), TabKind::Project);
        assert_eq!(TabKind::Review.next(), TabKind::Watchers);
        assert_eq!(TabKind::Watchers.prev(), TabKind::Review);
        assert_eq!(TabKind::Watchers.next(), TabKind::Embeddings);
        assert_eq!(TabKind::Embeddings.prev(), TabKind::Watchers);
        assert_eq!(TabKind::Embeddings.next(), TabKind::Resume);
        assert_eq!(TabKind::Resume.prev(), TabKind::Embeddings);
        assert_eq!(TabKind::Resume.next(), TabKind::Memories);
    }

    #[test]
    fn backend_activity_dedupes_by_persisted_event_id_and_formats_tokens() {
        let mut app = new_test_app();
        let id = Uuid::new_v4();
        let event = ActivityEvent {
            id,
            recorded_at: Utc::now(),
            project: "memory".to_string(),
            kind: ActivityKind::Query,
            memory_id: None,
            summary: "Query: activity model".to_string(),
            details: None,
            actor_id: None,
            actor_name: None,
            source: Some("query".to_string()),
            operation_id: None,
            duration_ms: Some(42),
            provider: Some("openai_compatible".to_string()),
            model: Some("gpt-test".to_string()),
            token_usage: Some(TokenUsage {
                input_tokens: 1000,
                output_tokens: 250,
                cache_read_tokens: 100,
                cache_write_tokens: 0,
                total_tokens: 1350,
            }),
        };

        app.record_backend_activity(event.clone());
        app.record_backend_activity(event);

        assert_eq!(app.activity_events.len(), 1);
        assert_eq!(activity_tokens(&app.activity_events[0]), "1.4k");
        assert_eq!(activity_duration(&app.activity_events[0]), "42");
    }

    #[test]
    fn llm_audit_status_lines_render_current_state() {
        let mut app = new_test_app();
        app.llm_audit_status = Some(LlmAuditStatusResponse {
            enabled: true,
            redacted: true,
            max_message_chars: 8000,
            max_total_chars: 32000,
            profile: "dev".to_string(),
            config_path: Some("/repo/.mem/config.dev.toml".to_string()),
        });

        let rendered = llm_audit_status_lines(&app)
            .into_iter()
            .map(|line| {
                line.spans
                    .into_iter()
                    .map(|span| span.content.into_owned())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("LLM audit: on"));
        assert!(rendered.contains("redaction=on"));
        assert!(rendered.contains("profile=dev"));
        assert!(rendered.contains("A disable"));
        assert!(rendered.contains("/repo/.mem/config.dev.toml"));
    }

    #[test]
    fn llm_audit_toggle_event_updates_status_message() {
        let mut app = new_test_app();

        app.apply_background_event(BackgroundEvent::LlmAuditToggled {
            enabled: true,
            response: Ok(LlmAuditStatusResponse {
                enabled: true,
                redacted: true,
                max_message_chars: 8000,
                max_total_chars: 32000,
                profile: "prod".to_string(),
                config_path: Some("/config/memory-layer.toml".to_string()),
            }),
        });

        assert!(!app.llm_audit_toggling);
        assert!(app.llm_audit_error.is_none());
        assert!(
            app.llm_audit_status
                .as_ref()
                .is_some_and(|status| status.enabled)
        );
        assert_eq!(app.status_message, "LLM audit/debug logging enabled.");
        assert_eq!(app.ui_status, UiStatus::Ready);
    }

    #[test]
    fn activity_help_mentions_llm_audit_toggle() {
        let help = super::tab_help_markdown(TabKind::Activity);
        assert!(help.contains("Shift+A"));
        assert!(help.contains("LLM audit/debug"));
    }

    #[test]
    fn errors_tab_collects_persisted_diagnostics() {
        let mut app = new_test_app();
        app.health_ok = true;
        app.record_backend_activity(ActivityEvent {
            id: Uuid::new_v4(),
            recorded_at: Utc::now(),
            project: "memory".to_string(),
            kind: ActivityKind::Diagnostic,
            memory_id: None,
            summary: "embedding quota exceeded".to_string(),
            details: Some(ActivityDetails::Diagnostic {
                diagnostic: DiagnosticInfo {
                    code: "embedding_quota_exceeded".to_string(),
                    source: "provider".to_string(),
                    component: "embeddings".to_string(),
                    operation: "automatic_embedding_creation".to_string(),
                    severity: DiagnosticSeverity::Warning,
                    message: "embedding quota exceeded".to_string(),
                    raw_error: Some("429 insufficient_quota".to_string()),
                    explanation: Some("provider quota was exhausted".to_string()),
                    fix_hint: Some("restore quota or disable automatic creation".to_string()),
                    doctor_hint: Some("memory doctor".to_string()),
                    command_hint: Some("memory embeddings list".to_string()),
                },
            }),
            actor_id: None,
            actor_name: None,
            source: Some("service".to_string()),
            operation_id: None,
            duration_ms: None,
            provider: None,
            model: None,
            token_usage: None,
        });

        let items = collect_error_items(&app);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].diagnostic.code, "embedding_quota_exceeded");
        assert_eq!(tui_status_detail(&app), Some("1 error".to_string()));
    }

    #[test]
    fn backend_query_activity_detail_renders_graph_metadata() {
        let event = ActivityEvent {
            id: Uuid::new_v4(),
            recorded_at: Utc::now(),
            project: "memory".to_string(),
            kind: ActivityKind::Query,
            memory_id: None,
            summary: "Query: graph retrieval".to_string(),
            details: Some(ActivityDetails::Query {
                query: "GraphTarget".to_string(),
                top_k: 8,
                result_count: 2,
                confidence: 0.82,
                insufficient_evidence: false,
                total_duration_ms: 91,
                graph_status: Some("active".to_string()),
                graph_candidates: 4,
                graph_augmented_candidates: 2,
                graph_duration_ms: 17,
                graph_result_count: 1,
                graph_connection_count: 2,
                graph_connections: vec![mem_api::QueryGraphConnection {
                    file_path: "src/lib.rs".to_string(),
                    symbol: Some("GraphTarget".to_string()),
                    symbol_kind: Some("function".to_string()),
                    edge_kind: Some("calls".to_string()),
                    neighbor_symbol: Some("caller".to_string()),
                    direction: Some("incoming".to_string()),
                    score_boost: 1.25,
                    reason: "code symbol match".to_string(),
                }],
                answer: Some("Use the graph-aware result.".to_string()),
                error: None,
            }),
            actor_id: None,
            actor_name: None,
            source: Some("query".to_string()),
            operation_id: None,
            duration_ms: Some(91),
            provider: None,
            model: None,
            token_usage: None,
        };

        let rendered = backend_activity_detail_lines(&event)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("Graph Retrieval"));
        assert!(rendered.contains("Status: active"));
        assert!(rendered.contains("Candidates: 4"));
        assert!(rendered.contains("Augmented results: 2"));
        assert!(rendered.contains("Graph Connections"));
        assert!(rendered.contains("code symbol match | src/lib.rs"));
    }

    #[test]
    fn historical_query_activity_without_graph_metadata_omits_graph_section() {
        let details: ActivityDetails = serde_json::from_value(serde_json::json!({
            "type": "query",
            "query": "old query",
            "top_k": 8,
            "result_count": 1,
            "confidence": 0.7,
            "insufficient_evidence": false,
            "total_duration_ms": 42,
            "answer": "old answer"
        }))
        .expect("historical query details should deserialize");
        let event = ActivityEvent {
            id: Uuid::new_v4(),
            recorded_at: Utc::now(),
            project: "memory".to_string(),
            kind: ActivityKind::Query,
            memory_id: None,
            summary: "Query: old query".to_string(),
            details: Some(details),
            actor_id: None,
            actor_name: None,
            source: Some("query".to_string()),
            operation_id: None,
            duration_ms: Some(42),
            provider: None,
            model: None,
            token_usage: None,
        };

        let rendered = backend_activity_detail_lines(&event)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("Query: old query"));
        assert!(!rendered.contains("Graph Retrieval"));
    }

    #[test]
    fn backend_llm_audit_activity_detail_renders_prompt_messages() {
        let event = ActivityEvent {
            id: Uuid::new_v4(),
            recorded_at: Utc::now(),
            project: "memory".to_string(),
            kind: ActivityKind::LlmAudit,
            memory_id: None,
            summary: "LLM audit: query_answer success".to_string(),
            details: Some(ActivityDetails::LlmAudit {
                operation: "query_answer".to_string(),
                request_summary: "Question: audit".to_string(),
                status: "success".to_string(),
                redacted: true,
                truncated: false,
                messages: vec![LlmAuditMessage {
                    role: "user".to_string(),
                    content: "Question: audit".to_string(),
                    truncated: false,
                }],
                error: None,
            }),
            actor_id: None,
            actor_name: None,
            source: Some("llm_audit".to_string()),
            operation_id: None,
            duration_ms: Some(12),
            provider: Some("openai_compatible".to_string()),
            model: Some("gpt-test".to_string()),
            token_usage: None,
        };

        let rendered = backend_activity_detail_lines(&event)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("Kind: llm-audit"));
        assert!(rendered.contains("Operation: query_answer"));
        assert!(rendered.contains("LLM Messages"));
        assert!(rendered.contains("Role user: Question: audit"));
    }

    #[test]
    fn backend_graph_extract_activity_detail_renders_counts() {
        let run_id = Uuid::new_v4();
        let event = ActivityEvent {
            id: Uuid::new_v4(),
            recorded_at: Utc::now(),
            project: "memory".to_string(),
            kind: ActivityKind::GraphExtract,
            memory_id: None,
            summary: "Extracted code graph: 10 symbols, 20 references, 9 graph edge(s)."
                .to_string(),
            details: Some(ActivityDetails::GraphExtract {
                repo_root: "/repo".to_string(),
                git_head: Some("abc123".to_string()),
                since: None,
                extraction_run_id: Some(run_id),
                dry_run: false,
                reused_existing_run: false,
                index_reused: true,
                analyzer_version: "mem-analyze-v2".to_string(),
                strategy_version: "code-graph-resolution-v1".to_string(),
                symbol_count: 10,
                reference_count: 20,
                resolved_reference_count: 12,
                unresolved_reference_count: 7,
                ambiguous_reference_count: 1,
                graph_node_count: 10,
                graph_edge_count: 9,
                evidence_count: 19,
            }),
            actor_id: None,
            actor_name: None,
            source: Some("service".to_string()),
            operation_id: None,
            duration_ms: None,
            provider: None,
            model: None,
            token_usage: None,
        };

        let rendered = backend_activity_detail_lines(&event)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("Kind: graph"));
        assert!(rendered.contains("Extraction run:"));
        assert!(rendered.contains("Symbols: 10"));
        assert!(rendered.contains("Graph edges: 9"));
        assert!(rendered.contains("Analyzer: mem-analyze-v2"));
    }

    #[test]
    fn markdown_renderer_formats_rich_memory_text_readably() {
        let lines = render_markdown_lines(
            "# Heading\n\n- [x] shipped\n1. numbered\n> quoted\n\nVisit [docs](https://example.com) and use `cargo test`.\n\n```rust\nfn main() {}\n```",
        );
        let rendered = lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("Heading"));
        assert!(rendered.contains("[x] shipped"));
        assert!(rendered.contains("1. numbered"));
        assert!(rendered.contains("quoted"));
        assert!(rendered.contains("docs (https://example.com)"));
        assert!(rendered.contains("cargo test"));
        assert!(rendered.contains("fn main() {}"));
    }

    #[test]
    fn build_memory_detail_lines_includes_rendered_canonical_text() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut app = App::new(
            "memory".to_string(),
            PathBuf::from("/tmp/memory"),
            ToolVersions {
                mem_cli: "0.4.5".to_string(),
                mem_service: "0.4.5".to_string(),
                watch_manager: "0.4.5".to_string(),
                memory_watch: "0.4.5".to_string(),
            },
            false,
            Profile::Prod,
            tx,
        );
        app.selected_detail = Some(test_memory_detail(
            "# Canonical\n\n- [ ] readable\n\n```text\nblock\n```",
        ));

        let rendered = build_memory_detail_lines(&app)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("Canonical Text"));
        assert!(rendered.contains("Canonical"));
        assert!(rendered.contains("[ ] readable"));
        assert!(rendered.contains("block"));
    }

    #[test]
    fn build_memory_detail_lines_lists_each_embedding_space() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut app = App::new(
            "memory".to_string(),
            PathBuf::from("/tmp/memory"),
            ToolVersions {
                mem_cli: "0.6.0".to_string(),
                mem_service: "0.6.0".to_string(),
                watch_manager: "0.6.0".to_string(),
                memory_watch: "0.6.0".to_string(),
            },
            false,
            Profile::Prod,
            tx,
        );
        let mut detail = test_memory_detail("body");
        let updated = Utc.with_ymd_and_hms(2026, 4, 22, 23, 37, 0).unwrap();
        detail.embedding_spaces = vec![
            MemoryEmbeddingSpace {
                provider: "openai".to_string(),
                model: "text-embedding-3-small".to_string(),
                base_url: "https://api.openai.com/v1".to_string(),
                chunk_count: 12,
                last_updated: Some(updated),
            },
            MemoryEmbeddingSpace {
                provider: "voyage".to_string(),
                model: "voyage-3".to_string(),
                base_url: "https://proxy.internal/voyage".to_string(),
                chunk_count: 1,
                last_updated: None,
            },
        ];
        app.selected_detail = Some(detail);

        let rendered = build_memory_detail_lines(&app)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("Embeddings"));
        assert!(rendered.contains("openai"));
        assert!(rendered.contains("text-embedding-3-small"));
        assert!(rendered.contains("12 chunks"));
        assert!(rendered.contains("voyage"));
        assert!(rendered.contains("voyage-3"));
        assert!(rendered.contains("1 chunk"));
        // OpenAI uses the default base URL, so it should not appear in the rendered output.
        assert!(!rendered.contains("https://api.openai.com/v1"));
        // Voyage is on a non-default base URL, so the URL appears on its own line.
        assert!(rendered.contains("https://proxy.internal/voyage"));
    }

    #[test]
    fn build_memory_detail_lines_puts_embeddings_section_above_canonical_text() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut app = App::new(
            "memory".to_string(),
            PathBuf::from("/tmp/memory"),
            ToolVersions {
                mem_cli: "0.6.2".to_string(),
                mem_service: "0.6.2".to_string(),
                watch_manager: "0.6.2".to_string(),
                memory_watch: "0.6.2".to_string(),
            },
            false,
            Profile::Prod,
            tx,
        );
        app.selected_detail = Some(test_memory_detail("body text"));

        let lines = build_memory_detail_lines(&app);
        let rendered = lines.iter().map(ToString::to_string).collect::<Vec<_>>();
        let embeddings_idx = rendered
            .iter()
            .position(|line| line.contains("Embeddings"))
            .expect("Embeddings header present");
        let canonical_idx = rendered
            .iter()
            .position(|line| line.contains("Canonical Text"))
            .expect("Canonical Text header present");
        assert!(
            embeddings_idx < canonical_idx,
            "Embeddings section must render above Canonical Text (embeddings at {embeddings_idx}, canonical at {canonical_idx})"
        );
    }

    #[test]
    fn build_memory_detail_lines_shows_empty_state_when_no_embeddings() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut app = App::new(
            "memory".to_string(),
            PathBuf::from("/tmp/memory"),
            ToolVersions {
                mem_cli: "0.6.0".to_string(),
                mem_service: "0.6.0".to_string(),
                watch_manager: "0.6.0".to_string(),
                memory_watch: "0.6.0".to_string(),
            },
            false,
            Profile::Prod,
            tx,
        );
        app.selected_detail = Some(test_memory_detail("body"));

        let rendered = build_memory_detail_lines(&app)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("Embeddings"));
        assert!(rendered.contains("No embeddings for this memory yet."));
    }

    fn embeddings_test_response() -> mem_api::EmbeddingBackendsResponse {
        mem_api::EmbeddingBackendsResponse {
            backends: vec![
                mem_api::EmbeddingBackendInfo {
                    name: "openai-3-small".to_string(),
                    provider: "openai_compatible".to_string(),
                    base_url: "https://api.openai.com/v1".to_string(),
                    model: "text-embedding-3-small".to_string(),
                    active: false,
                    ready: true,
                    create_enabled: true,
                    project_chunk_count: Some(12),
                    project_memory_count: Some(4),
                },
                mem_api::EmbeddingBackendInfo {
                    name: "voyage-code".to_string(),
                    provider: "voyage".to_string(),
                    base_url: "https://api.voyageai.com".to_string(),
                    model: "voyage-code-3".to_string(),
                    active: true,
                    ready: true,
                    create_enabled: true,
                    project_chunk_count: Some(12),
                    project_memory_count: Some(4),
                },
            ],
            active: Some("voyage-code".to_string()),
            create_enabled: true,
        }
    }

    fn new_test_app() -> App {
        let (tx, _rx) = mpsc::unbounded_channel();
        App::new(
            "memory".to_string(),
            PathBuf::from("/tmp/memory"),
            ToolVersions {
                mem_cli: "0.6.2".to_string(),
                mem_service: "0.6.2".to_string(),
                watch_manager: "0.6.2".to_string(),
                memory_watch: "0.6.2".to_string(),
            },
            false,
            Profile::Prod,
            tx,
        )
    }

    #[test]
    fn embeddings_loaded_event_populates_snapshot_and_clamps_selection() {
        let mut app = new_test_app();
        app.embeddings_selected_index = 5; // out of range
        app.apply_background_event(BackgroundEvent::EmbeddingBackendsLoaded {
            snapshot: Ok(embeddings_test_response()),
        });
        let snapshot = app.embedding_backends_snapshot.as_ref().expect("loaded");
        assert_eq!(snapshot.backends.len(), 2);
        assert_eq!(app.embeddings_selected_index, 1);
        assert_eq!(app.embeddings_table_state.selected(), Some(1));
        assert!(app.embedding_backends_error.is_none());
    }

    #[test]
    fn embeddings_loaded_event_selects_active_backend() {
        let mut app = new_test_app();
        app.embeddings_selected_index = 0;

        app.apply_background_event(BackgroundEvent::EmbeddingBackendsLoaded {
            snapshot: Ok(embeddings_test_response()),
        });

        assert_eq!(app.embeddings_selected_index, 1);
        assert_eq!(app.embeddings_table_state.selected(), Some(1));
        assert_eq!(
            app.selected_embedding_backend_name().as_deref(),
            Some("voyage-code")
        );
    }

    #[test]
    fn embeddings_selection_wraps_cyclically() {
        let mut app = new_test_app();
        app.apply_background_event(BackgroundEvent::EmbeddingBackendsLoaded {
            snapshot: Ok(embeddings_test_response()),
        });
        app.embeddings_selected_index = 0;
        app.move_embeddings_selection(1);
        assert_eq!(app.embeddings_selected_index, 1);
        assert_eq!(
            app.selected_embedding_backend_name().as_deref(),
            Some("voyage-code")
        );
        app.move_embeddings_selection(1);
        assert_eq!(app.embeddings_selected_index, 0);
        app.move_embeddings_selection(-1);
        assert_eq!(app.embeddings_selected_index, 1);
    }

    #[test]
    fn embedding_backend_toggle_sets_success_message_and_updates_snapshot() {
        let mut app = new_test_app();
        // First load the initial list so selection is primed.
        app.apply_background_event(BackgroundEvent::EmbeddingBackendsLoaded {
            snapshot: Ok(embeddings_test_response()),
        });
        app.embeddings_toggling = Some("openai-3-small".to_string());

        // Simulate the activate POST returning a response where openai is now active.
        let mut response = embeddings_test_response();
        response.active = Some("openai-3-small".to_string());
        response.backends[0].active = true;
        response.backends[1].active = false;
        app.apply_background_event(BackgroundEvent::EmbeddingBackendToggled {
            name: "openai-3-small".to_string(),
            result: Ok(response),
        });

        assert_eq!(app.embeddings_toggling, None);
        assert_eq!(
            app.embeddings_toggle_message.as_deref(),
            Some("Activated openai-3-small")
        );
        assert_eq!(
            app.embedding_backends_snapshot
                .as_ref()
                .and_then(|s| s.active.as_deref()),
            Some("openai-3-small")
        );
    }

    #[test]
    fn embedding_backend_toggle_off_sets_success_message() {
        let mut app = new_test_app();
        app.embeddings_toggling = Some("turning off voyage-code".to_string());
        let mut response = embeddings_test_response();
        response.active = None;
        response.backends[1].active = false;

        app.apply_background_event(BackgroundEvent::EmbeddingBackendToggled {
            name: "voyage-code".to_string(),
            result: Ok(response),
        });

        assert_eq!(app.embeddings_toggling, None);
        assert_eq!(
            app.embeddings_toggle_message.as_deref(),
            Some("Embeddings off")
        );
        assert_eq!(
            app.embedding_backends_snapshot
                .as_ref()
                .and_then(|s| s.active.as_deref()),
            None
        );
    }

    #[test]
    fn embedding_creation_toggle_updates_snapshot_and_status() {
        let mut app = new_test_app();
        app.embeddings_creation_toggling = true;
        let mut response = embeddings_test_response();
        response.backends[1].create_enabled = false;

        app.apply_background_event(BackgroundEvent::EmbeddingCreationToggled {
            name: "voyage-code".to_string(),
            enabled: false,
            result: Ok(response),
        });

        assert!(!app.embeddings_creation_toggling);
        assert_eq!(
            app.embeddings_toggle_message.as_deref(),
            Some("Automatic embedding creation off for voyage-code")
        );
        assert_eq!(
            app.embedding_backends_snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.backends.get(1))
                .map(|backend| backend.create_enabled),
            Some(false)
        );
    }

    #[test]
    fn embedding_reembed_completion_updates_snapshot_and_status() {
        let mut app = new_test_app();
        app.embeddings_selected_index = 0;
        app.embeddings_operation = Some("creating embeddings for openai-3-small".to_string());
        let mut snapshot = embeddings_test_response();
        snapshot.backends[0].project_chunk_count = Some(18);

        app.apply_background_event(BackgroundEvent::EmbeddingReembedCompleted {
            name: "openai-3-small".to_string(),
            result: Ok((
                mem_api::ReembedResponse {
                    reembedded_chunks: 6,
                    dry_run: false,
                },
                snapshot,
            )),
        });

        assert_eq!(app.embeddings_operation, None);
        assert_eq!(
            app.embeddings_toggle_message.as_deref(),
            Some("Created 6 chunk embedding(s) for openai-3-small")
        );
        assert_eq!(app.embeddings_selected_index, 0);
        assert_eq!(
            app.embedding_backends_snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.backends.first())
                .and_then(|backend| backend.project_chunk_count),
            Some(18)
        );
    }

    #[test]
    fn embedding_reindex_completion_updates_snapshot_and_status() {
        let mut app = new_test_app();
        app.embeddings_selected_index = 1;
        app.embeddings_operation = Some("reindexing all backends".to_string());
        let mut snapshot = embeddings_test_response();
        snapshot.backends[1].project_chunk_count = Some(20);

        app.apply_background_event(BackgroundEvent::EmbeddingReindexCompleted {
            name: "all backends".to_string(),
            result: Ok((
                mem_api::ReindexResponse {
                    reindexed_entries: 4,
                    dry_run: false,
                },
                snapshot,
            )),
        });

        assert_eq!(app.embeddings_operation, None);
        assert_eq!(
            app.embeddings_toggle_message.as_deref(),
            Some("Reindexed 4 memory entries for all backends")
        );
        assert_eq!(app.embeddings_selected_index, 1);
        assert_eq!(
            app.embedding_backends_snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.backends.get(1))
                .and_then(|backend| backend.project_chunk_count),
            Some(20)
        );
    }

    #[test]
    fn embedding_reembed_failure_shows_error_message() {
        let mut app = new_test_app();
        app.embeddings_operation = Some("creating embeddings for broken".to_string());

        app.apply_background_event(BackgroundEvent::EmbeddingReembedCompleted {
            name: "broken".to_string(),
            result: Err("provider unavailable".to_string()),
        });

        assert_eq!(app.embeddings_operation, None);
        assert_eq!(
            app.embeddings_toggle_message.as_deref(),
            Some("Embedding creation failed for broken: provider unavailable")
        );
    }

    #[test]
    fn embedding_backend_toggle_failure_shows_error_message() {
        let mut app = new_test_app();
        app.embeddings_toggling = Some("broken".to_string());
        app.apply_background_event(BackgroundEvent::EmbeddingBackendToggled {
            name: "broken".to_string(),
            result: Err("400 unknown backend".to_string()),
        });
        assert_eq!(app.embeddings_toggling, None);
        let msg = app.embeddings_toggle_message.as_deref().unwrap_or("");
        assert!(msg.starts_with("Toggle failed:"));
        assert!(msg.contains("400 unknown backend"));
    }

    #[test]
    fn memories_focus_toggle_and_escape_return_to_list() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut app = App::new(
            "memory".to_string(),
            PathBuf::from("/tmp/memory"),
            ToolVersions {
                mem_cli: "0.4.5".to_string(),
                mem_service: "0.4.5".to_string(),
                watch_manager: "0.4.5".to_string(),
                memory_watch: "0.4.5".to_string(),
            },
            false,
            Profile::Prod,
            tx,
        );
        app.selected_detail = Some(test_memory_detail("detail"));

        assert_eq!(app.memories_focus, MemoriesFocus::List);
        app.toggle_memories_focus();
        assert_eq!(app.memories_focus, MemoriesFocus::Detail);
        app.focus_memories_list();
        assert_eq!(app.memories_focus, MemoriesFocus::List);
    }

    #[test]
    fn memory_detail_scroll_is_clamped_to_rendered_content() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut app = App::new(
            "memory".to_string(),
            PathBuf::from("/tmp/memory"),
            ToolVersions {
                mem_cli: "0.4.5".to_string(),
                mem_service: "0.4.5".to_string(),
                watch_manager: "0.4.5".to_string(),
                memory_watch: "0.4.5".to_string(),
            },
            false,
            Profile::Prod,
            tx,
        );
        let canonical = (0..40)
            .map(|idx| format!("- [x] item {idx} with enough text to wrap in the detail pane"))
            .collect::<Vec<_>>()
            .join("\n");
        app.selected_detail = Some(test_memory_detail(&canonical));
        let frame = ratatui::layout::Rect::new(0, 0, 100, 24);

        let max_scroll = memory_detail_max_scroll(&app, frame);
        assert!(max_scroll > 0);

        app.scroll_memory_detail_in_area(500, frame);
        assert_eq!(app.memory_detail_scroll, max_scroll);

        app.scroll_memory_detail_in_area(-500, frame);
        assert_eq!(app.memory_detail_scroll, 0);
    }
}
