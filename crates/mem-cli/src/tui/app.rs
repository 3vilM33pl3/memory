use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self as std_mpsc, RecvTimeoutError},
    },
    time::{Duration, Instant},
};

use anyhow::Result;
use chrono::{DateTime, Utc};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use mem_agenttop::AgentSnapshot;
use mem_api::{
    ActivityDetails, ActivityEvent, MemoryEntryResponse, Profile, ProjectMemoriesResponse,
    QueryFilters, QueryRequest, QueryResponse, QueryResult, ReplacementPolicy, ResumeCheckpoint,
    ResumeRequest, StreamRequest, StreamResponse, UpToSpeedRequest, load_repo_replacement_policy,
    read_capnp_text_frame, write_capnp_text_frame,
};
use ratatui::{layout::Rect, widgets::TableState};
use tokio::{
    net::{TcpStream, UnixStream},
    sync::mpsc,
};

use crate::{
    commands::{
        api::ApiClient,
        service_support::{enable_relay_discovery_and_restart_backend, load_tui_restart_notice},
        skill_support::project_skill_inventory,
    },
    resume,
};

#[cfg(test)]
pub(in crate::tui) use super::markdown::render_markdown_lines;
pub(in crate::tui) use super::render::*;
pub(in crate::tui) use super::runtime::*;
pub(in crate::tui) use super::state::*;
#[cfg(test)]
pub(in crate::tui) use super::theme::Theme;

pub(super) const STREAM_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

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
        app.agents.agents_tab_visible.clone(),
        app.agents
            .agent_wake_rx
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
        app.embeddings.embeddings_tab_visible.clone(),
        app.embeddings
            .embeddings_wake_rx
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
pub(super) const AGENT_POLL_ACTIVE: Duration = Duration::from_secs(5);
/// Slow cadence used when no tab displays agent_snapshot. Switching to the
/// Agents tab sends a wake signal so the user doesn't wait this long.
pub(super) const AGENT_POLL_IDLE: Duration = Duration::from_secs(30);

pub(super) fn start_agent_snapshot_worker(
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

pub(super) const EMBEDDINGS_POLL_ACTIVE: Duration = Duration::from_secs(5);
pub(super) const EMBEDDINGS_POLL_IDLE: Duration = Duration::from_secs(60);

pub(super) fn start_embedding_backends_worker(
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

pub(super) fn start_manager_status_worker(
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

pub(super) fn spawn_stream_connect(
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
            memories: MemoriesTabState {
                all_memories: Vec::new(),
                filtered_memories: Vec::new(),
                total_memories: 0,
                selected_detail: None,
                selected_history: None,
                selected_index: 0,
                table_state,
                memories_focus: MemoriesFocus::List,
                memory_detail_scroll: 0,
            },
            query: QueryTabState {
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
            },
            agents: AgentsTabState {
                agent_snapshot: None,
                agent_loading: true,
                agent_error: None,
                agent_selected_index: 0,
                agent_table_state,
                agent_detail_scroll: 0,
                agent_initial_selection_done: false,
                agents_tab_visible: Arc::new(AtomicBool::new(false)),
                agent_wake_tx,
                agent_wake_rx: Some(agent_wake_rx),
            },
            resume: ResumeTabState {
                resume_response: None,
                resume_loading: false,
                resume_loaded: false,
                resume_error: None,
                resume_scroll: 0,
                startup_resume_autoselect_pending: true,
            },
            activity: ActivityTabState {
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
                up_to_speed_response: None,
                up_to_speed_loading: false,
                up_to_speed_error: None,
            },
            errors: ErrorsTabState {
                errors_selected_index: 0,
                errors_table_state,
                errors_detail_scroll: 0,
            },
            project_tab: ProjectTabState { project_scroll: 0 },
            review: ReviewTabState {
                replacement_policy: load_repo_replacement_policy(&repo_root).unwrap_or_default(),
                replacement_proposals: Vec::new(),
                replacement_selected_index: 0,
                review_table_state,
            },
            watchers: WatchersTabState { watcher_scroll: 0 },
            embeddings: EmbeddingsTabState {
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
            },
            overview: empty_overview(project),
            help: HelpState {
                help_open: false,
                help_tab: TabKind::Memories,
                help_scroll: 0,
            },
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
            background_tx,
            needs_redraw: true,
        }
    }

    fn set_active_tab(&mut self, tab: TabKind) {
        let became_agents = tab == TabKind::Agents && self.active_tab != TabKind::Agents;
        let became_embeddings =
            tab == TabKind::Embeddings && self.active_tab != TabKind::Embeddings;
        self.active_tab = tab;
        self.agents
            .agents_tab_visible
            .store(tab == TabKind::Agents, Ordering::Relaxed);
        self.embeddings
            .embeddings_tab_visible
            .store(tab == TabKind::Embeddings, Ordering::Relaxed);
        if became_agents {
            // Wake the worker so the newly-opened tab shows fresh data
            // rather than whatever the idle cadence last produced.
            let _ = self.agents.agent_wake_tx.send(());
        }
        if became_embeddings {
            let _ = self.embeddings.embeddings_wake_tx.send(());
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
        self.memories.selected_detail = None;
        self.review.replacement_policy =
            load_repo_replacement_policy(&self.repo_root).unwrap_or_default();
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
            .memories
            .filtered_memories
            .get(self.memories.selected_index)
            .map(|item| item.id);
        spawn_stream_connect(
            api.clone(),
            self.project.clone(),
            memory_id,
            self.background_tx.clone(),
        );
    }

    fn request_activities(&mut self, api: &ApiClient) {
        if self.activity.activity_loading {
            return;
        }
        self.activity.activity_loading = true;
        self.activity.activity_error = None;
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
        if self.activity.llm_audit_loading {
            return;
        }
        self.activity.llm_audit_loading = true;
        self.activity.llm_audit_error = None;
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
        if self.activity.llm_audit_toggling {
            return;
        }
        let enabled = !self
            .activity
            .llm_audit_status
            .as_ref()
            .map(|status| status.enabled)
            .unwrap_or(false);
        self.activity.llm_audit_toggling = true;
        self.activity.llm_audit_error = None;
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
        if self.activity.up_to_speed_loading {
            return;
        }
        self.activity.up_to_speed_loading = true;
        self.activity.up_to_speed_error = None;
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
                self.memories.total_memories = total;
                self.memories.all_memories = items;
                self.apply_filters();
                self.status_message = format!(
                    "Loaded {} visible memories ({} total).",
                    self.memories.filtered_memories.len(),
                    self.memories.total_memories
                );
                loaded_memories = true;
            }
            Err(error) => {
                had_error = true;
                self.memories.all_memories.clear();
                self.memories.filtered_memories.clear();
                self.memories.total_memories = 0;
                self.memories.selected_detail = None;
                self.memories.table_state.select(None);
                if self.health_ok {
                    self.status_message = error.to_string();
                }
            }
        }

        match result.proposals {
            Ok(response) => {
                self.review.replacement_proposals = response.proposals;
                if self.review.replacement_proposals.is_empty() {
                    self.review.replacement_selected_index = 0;
                    self.review.review_table_state.select(None);
                } else {
                    self.review.replacement_selected_index = self
                        .review
                        .replacement_selected_index
                        .min(self.review.replacement_proposals.len() - 1);
                    self.review
                        .review_table_state
                        .select(Some(self.review.replacement_selected_index));
                }
            }
            Err(error) => {
                had_error = true;
                self.review.replacement_proposals.clear();
                self.review.replacement_selected_index = 0;
                self.review.review_table_state.select(None);
                self.status_message = error.to_string();
            }
        }

        self.ui_status = if had_error {
            UiStatus::Error
        } else if self.resume.resume_loading || self.query.query_loading {
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
        if self.resume.resume_loading {
            return;
        }
        let checkpoint = self.resume_checkpoint();
        self.resume.resume_loading = true;
        self.resume.resume_error = None;
        if self.resume.resume_response.is_some() {
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
                self.resume.resume_loading = false;
                match *response {
                    Ok(response) => {
                        self.resume.resume_response = Some(response);
                        self.resume.resume_loaded = true;
                        self.resume.resume_error = None;
                        if allow_autoselect
                            && self.resume.startup_resume_autoselect_pending
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
                        self.resume.resume_error = Some(error.clone());
                        if self.resume.resume_response.is_none() {
                            self.resume.resume_loaded = false;
                        }
                        self.status_message = format!("Resume unavailable: {error}");
                        self.ui_status = UiStatus::Error;
                    }
                }
            }
            BackgroundEvent::AgentsLoaded { snapshot } => match snapshot {
                Ok(snapshot) => {
                    self.agents.agent_loading = false;
                    self.agents.agent_error = None;
                    self.agents.agent_snapshot = Some(snapshot);
                    let session_count = self
                        .agents
                        .agent_snapshot
                        .as_ref()
                        .map(|snapshot| snapshot.sessions.len())
                        .unwrap_or(0);
                    if session_count == 0 {
                        self.agents.agent_selected_index = 0;
                        self.agents.agent_table_state.select(None);
                    } else {
                        if !self.agents.agent_initial_selection_done {
                            self.agents.agent_selected_index =
                                self.agents
                                    .agent_snapshot
                                    .as_ref()
                                    .and_then(|snapshot| {
                                        snapshot.sessions.iter().position(|session| {
                                            session.project_name == self.project
                                        })
                                    })
                                    .unwrap_or(0);
                            self.agents.agent_initial_selection_done = true;
                        } else {
                            self.agents.agent_selected_index = self
                                .agents
                                .agent_selected_index
                                .min(session_count.saturating_sub(1));
                        }
                        self.agents
                            .agent_table_state
                            .select(Some(self.agents.agent_selected_index));
                    }
                }
                Err(error) => {
                    self.agents.agent_loading = false;
                    self.agents.agent_error = Some(error);
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
                self.activity.activity_loading = false;
                match *response {
                    Ok(response) => {
                        self.activity.activity_error = None;
                        self.activity.activity_events = response
                            .items
                            .into_iter()
                            .map(|event| ActivityEntry::Backend(Box::new(event)))
                            .collect();
                        self.finish_activity_insert();
                        self.status_message = format!(
                            "Loaded {} persisted activity event(s).",
                            self.activity.activity_events.len()
                        );
                    }
                    Err(error) => {
                        self.activity.activity_error = Some(error.clone());
                        self.status_message = format!("Activities unavailable: {error}");
                    }
                }
            }
            BackgroundEvent::LlmAuditStatusLoaded { response } => {
                self.activity.llm_audit_loading = false;
                match response {
                    Ok(status) => {
                        self.activity.llm_audit_error = None;
                        self.activity.llm_audit_status = Some(status);
                    }
                    Err(error) => {
                        self.activity.llm_audit_error = Some(error.clone());
                        self.status_message = format!("LLM audit status unavailable: {error}");
                    }
                }
            }
            BackgroundEvent::LlmAuditToggled { enabled, response } => {
                self.activity.llm_audit_toggling = false;
                match response {
                    Ok(status) => {
                        self.activity.llm_audit_error = None;
                        self.activity.llm_audit_status = Some(status);
                        self.status_message = format!(
                            "LLM audit/debug logging {}.",
                            if enabled { "enabled" } else { "disabled" }
                        );
                        self.ui_status = UiStatus::Ready;
                    }
                    Err(error) => {
                        self.activity.llm_audit_error = Some(error.clone());
                        self.status_message = format!("LLM audit toggle failed: {error}");
                        self.ui_status = UiStatus::Error;
                    }
                }
            }
            BackgroundEvent::UpToSpeedLoaded { response } => {
                self.activity.up_to_speed_loading = false;
                match *response {
                    Ok(response) => {
                        self.activity.up_to_speed_error = None;
                        self.activity.up_to_speed_response = Some(response);
                        self.status_message = "Get-up-to-speed briefing generated.".to_string();
                        self.ui_status = UiStatus::Ready;
                    }
                    Err(error) => {
                        self.activity.up_to_speed_error = Some(error.clone());
                        self.status_message = format!("Get-up-to-speed failed: {error}");
                        self.ui_status = UiStatus::Error;
                    }
                }
            }
            BackgroundEvent::EmbeddingBackendsLoaded { snapshot } => match snapshot {
                Ok(snapshot) => {
                    self.embeddings.embedding_backends_error = None;
                    let selected_index = active_embedding_backend_index(&snapshot).or_else(|| {
                        clamped_embedding_backend_index(
                            self.embeddings.embeddings_selected_index,
                            &snapshot,
                        )
                    });
                    self.embeddings.embedding_backends_snapshot = Some(snapshot);
                    if let Some(index) = selected_index {
                        self.embeddings.embeddings_selected_index = index;
                        self.embeddings
                            .embeddings_table_state
                            .select(Some(self.embeddings.embeddings_selected_index));
                    } else {
                        self.embeddings.embeddings_selected_index = 0;
                        self.embeddings.embeddings_table_state.select(None);
                    }
                }
                Err(error) => {
                    self.embeddings.embedding_backends_error = Some(error);
                }
            },
            BackgroundEvent::EmbeddingBackendToggled { name, result } => {
                self.embeddings.embeddings_toggling = None;
                match result {
                    Ok(snapshot) => {
                        self.embeddings.embedding_backends_error = None;
                        self.embeddings.embeddings_toggle_message =
                            if snapshot.active.as_deref() == Some(name.as_str()) {
                                Some(format!("Activated {name}"))
                            } else {
                                Some("Embeddings off".to_string())
                            };
                        let selected_index =
                            active_embedding_backend_index(&snapshot).or_else(|| {
                                clamped_embedding_backend_index(
                                    self.embeddings.embeddings_selected_index,
                                    &snapshot,
                                )
                            });
                        self.embeddings.embedding_backends_snapshot = Some(snapshot);
                        if let Some(index) = selected_index {
                            self.embeddings.embeddings_selected_index = index;
                            self.embeddings
                                .embeddings_table_state
                                .select(Some(self.embeddings.embeddings_selected_index));
                        } else {
                            self.embeddings.embeddings_selected_index = 0;
                            self.embeddings.embeddings_table_state.select(None);
                        }
                    }
                    Err(error) => {
                        self.embeddings.embeddings_toggle_message =
                            Some(format!("Toggle failed: {error}"));
                    }
                }
            }
            BackgroundEvent::EmbeddingCreationToggled {
                name,
                enabled,
                result,
            } => {
                self.embeddings.embeddings_creation_toggling = false;
                match result {
                    Ok(snapshot) => {
                        self.embeddings.embedding_backends_error = None;
                        self.embeddings.embeddings_toggle_message = Some(format!(
                            "Automatic embedding creation {} for {name}",
                            if enabled { "on" } else { "off" },
                        ));
                        let selected_index =
                            active_embedding_backend_index(&snapshot).or_else(|| {
                                clamped_embedding_backend_index(
                                    self.embeddings.embeddings_selected_index,
                                    &snapshot,
                                )
                            });
                        self.embeddings.embedding_backends_snapshot = Some(snapshot);
                        if let Some(index) = selected_index {
                            self.embeddings.embeddings_selected_index = index;
                            self.embeddings
                                .embeddings_table_state
                                .select(Some(self.embeddings.embeddings_selected_index));
                        } else {
                            self.embeddings.embeddings_selected_index = 0;
                            self.embeddings.embeddings_table_state.select(None);
                        }
                    }
                    Err(error) => {
                        self.embeddings.embeddings_toggle_message =
                            Some(format!("Creation toggle failed: {error}"));
                    }
                }
            }
            BackgroundEvent::EmbeddingReembedCompleted { name, result } => {
                self.embeddings.embeddings_operation = None;
                match result {
                    Ok((response, snapshot)) => {
                        self.embeddings.embedding_backends_error = None;
                        self.embeddings.embeddings_toggle_message = Some(format!(
                            "Created {} chunk embedding(s) for {name}",
                            response.reembedded_chunks
                        ));
                        let selected_index = embedding_backend_index_by_name(&snapshot, &name)
                            .or_else(|| {
                                clamped_embedding_backend_index(
                                    self.embeddings.embeddings_selected_index,
                                    &snapshot,
                                )
                            });
                        self.embeddings.embedding_backends_snapshot = Some(snapshot);
                        if let Some(index) = selected_index {
                            self.embeddings.embeddings_selected_index = index;
                            self.embeddings
                                .embeddings_table_state
                                .select(Some(self.embeddings.embeddings_selected_index));
                        } else {
                            self.embeddings.embeddings_selected_index = 0;
                            self.embeddings.embeddings_table_state.select(None);
                        }
                    }
                    Err(error) => {
                        self.embeddings.embeddings_toggle_message =
                            Some(format!("Embedding creation failed for {name}: {error}"));
                    }
                }
            }
            BackgroundEvent::EmbeddingReindexCompleted { name, result } => {
                self.embeddings.embeddings_operation = None;
                match result {
                    Ok((response, snapshot)) => {
                        self.embeddings.embedding_backends_error = None;
                        let target = if name == "all backends" {
                            "all backends"
                        } else {
                            name.as_str()
                        };
                        self.embeddings.embeddings_toggle_message = Some(format!(
                            "Reindexed {} memory entries for {target}",
                            response.reindexed_entries
                        ));
                        let selected_index = embedding_backend_index_by_name(&snapshot, &name)
                            .or_else(|| {
                                clamped_embedding_backend_index(
                                    self.embeddings.embeddings_selected_index,
                                    &snapshot,
                                )
                            });
                        self.embeddings.embedding_backends_snapshot = Some(snapshot);
                        if let Some(index) = selected_index {
                            self.embeddings.embeddings_selected_index = index;
                            self.embeddings
                                .embeddings_table_state
                                .select(Some(self.embeddings.embeddings_selected_index));
                        } else {
                            self.embeddings.embeddings_selected_index = 0;
                            self.embeddings.embeddings_table_state.select(None);
                        }
                    }
                    Err(error) => {
                        self.embeddings.embeddings_toggle_message =
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
        let Some(snapshot) = &self.agents.agent_snapshot else {
            self.agents.agent_selected_index = 0;
            self.agents.agent_table_state.select(None);
            return;
        };
        let len = snapshot.sessions.len();
        if len == 0 {
            self.agents.agent_selected_index = 0;
            self.agents.agent_table_state.select(None);
            return;
        }
        let next = (self.agents.agent_selected_index as isize + delta).clamp(0, len as isize - 1);
        self.agents.agent_selected_index = next as usize;
        self.agents
            .agent_table_state
            .select(Some(self.agents.agent_selected_index));
    }

    fn move_embeddings_selection(&mut self, delta: isize) {
        let len = self
            .embeddings
            .embedding_backends_snapshot
            .as_ref()
            .map(|s| s.backends.len())
            .unwrap_or(0);
        if len == 0 {
            self.embeddings.embeddings_selected_index = 0;
            self.embeddings.embeddings_table_state.select(None);
            return;
        }
        // Cyclic wrap so j/k loops within the list.
        let cur = self.embeddings.embeddings_selected_index as isize;
        let next = ((cur + delta) % len as isize + len as isize) % len as isize;
        self.embeddings.embeddings_selected_index = next as usize;
        self.embeddings
            .embeddings_table_state
            .select(Some(self.embeddings.embeddings_selected_index));
    }

    fn selected_embedding_backend_name(&self) -> Option<String> {
        self.embeddings
            .embedding_backends_snapshot
            .as_ref()
            .and_then(|snapshot| {
                snapshot
                    .backends
                    .get(self.embeddings.embeddings_selected_index)
                    .map(|b| b.name.clone())
            })
    }

    fn selected_embedding_backend_is_active(&self) -> bool {
        self.embeddings
            .embedding_backends_snapshot
            .as_ref()
            .and_then(|snapshot| {
                snapshot
                    .backends
                    .get(self.embeddings.embeddings_selected_index)
            })
            .is_some_and(|backend| backend.active)
    }

    fn selected_embedding_backend_create_enabled(&self) -> Option<bool> {
        self.embeddings
            .embedding_backends_snapshot
            .as_ref()
            .and_then(|snapshot| {
                snapshot
                    .backends
                    .get(self.embeddings.embeddings_selected_index)
            })
            .map(|backend| backend.create_enabled)
    }

    fn scroll_agent_detail(&mut self, delta: i16) {
        self.agents.agent_detail_scroll =
            self.agents.agent_detail_scroll.saturating_add_signed(delta);
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

        self.resume.startup_resume_autoselect_pending = false;

        if key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL {
            return Ok(true);
        }

        if self.help.help_open {
            self.handle_help_key(key);
            return Ok(false);
        }

        match key.code {
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') if key.modifiers.is_empty() => {
                self.set_active_tab(self.active_tab.next());
                if self.active_tab == TabKind::Resume && !self.resume.resume_loaded {
                    self.request_resume_refresh(api, false);
                }
            }
            KeyCode::BackTab
                if key.modifiers == KeyModifiers::SHIFT || key.modifiers.is_empty() =>
            {
                self.set_active_tab(self.active_tab.prev());
                if self.active_tab == TabKind::Resume && !self.resume.resume_loaded {
                    self.request_resume_refresh(api, false);
                }
            }
            KeyCode::Left if key.modifiers.is_empty() => {
                self.set_active_tab(self.active_tab.prev());
                if self.active_tab == TabKind::Resume && !self.resume.resume_loaded {
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
                if self.memories.memories_focus == MemoriesFocus::Detail {
                    self.scroll_memory_detail(1);
                } else {
                    self.move_selection(1, api, stream).await;
                }
            }
            KeyCode::Up | KeyCode::Char('k') if self.active_tab == TabKind::Memories => {
                if self.memories.memories_focus == MemoriesFocus::Detail {
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
                self.errors.errors_detail_scroll =
                    self.errors.errors_detail_scroll.saturating_add(8);
            }
            KeyCode::PageUp if self.active_tab == TabKind::Errors => {
                self.errors.errors_detail_scroll =
                    self.errors.errors_detail_scroll.saturating_sub(8);
            }
            KeyCode::Home if self.active_tab == TabKind::Errors => {
                self.errors.errors_detail_scroll = 0;
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
                self.activity.activity_detail_scroll =
                    self.activity.activity_detail_scroll.saturating_add(8);
            }
            KeyCode::PageUp if self.active_tab == TabKind::Activity => {
                self.activity.activity_detail_scroll =
                    self.activity.activity_detail_scroll.saturating_sub(8);
            }
            KeyCode::Home if self.active_tab == TabKind::Activity => {
                self.activity.activity_detail_scroll = 0;
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
                    self.embeddings.embeddings_toggling = Some(if deactivate {
                        format!("turning off {name}")
                    } else {
                        name.clone()
                    });
                    self.embeddings.embeddings_toggle_message = None;
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
                    self.embeddings.embeddings_creation_toggling = true;
                    self.embeddings.embeddings_toggle_message = None;
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
                if self.embeddings.embeddings_operation.is_none()
                    && let Some(name) = self.selected_embedding_backend_name()
                {
                    self.embeddings.embeddings_operation =
                        Some(format!("creating embeddings for {name}"));
                    self.embeddings.embeddings_toggle_message = None;
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
                    && self.embeddings.embeddings_operation.is_none() =>
            {
                let name = "all backends".to_string();
                self.embeddings.embeddings_operation = Some("reindexing all backends".to_string());
                self.embeddings.embeddings_toggle_message = None;
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
                let _ = self.embeddings.embeddings_wake_tx.send(());
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
                self.agents.agent_detail_scroll = 0;
            }
            KeyCode::PageDown if self.active_tab == TabKind::Resume => {
                self.scroll_resume(8);
            }
            KeyCode::PageUp if self.active_tab == TabKind::Resume => {
                self.scroll_resume(-8);
            }
            KeyCode::Home if self.active_tab == TabKind::Resume => {
                self.resume.resume_scroll = 0;
            }
            KeyCode::PageDown if self.active_tab == TabKind::Project => {
                self.scroll_project(8);
            }
            KeyCode::PageUp if self.active_tab == TabKind::Project => {
                self.scroll_project(-8);
            }
            KeyCode::Home if self.active_tab == TabKind::Project => {
                self.project_tab.project_scroll = 0;
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
                let len = self.review.replacement_proposals.len();
                self.jump_replacement_proposal(len.saturating_sub(1));
            }
            KeyCode::PageDown if self.active_tab == TabKind::Watchers => {
                self.scroll_watchers(8);
            }
            KeyCode::PageUp if self.active_tab == TabKind::Watchers => {
                self.scroll_watchers(-8);
            }
            KeyCode::Home if self.active_tab == TabKind::Watchers => {
                self.watchers.watcher_scroll = 0;
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
                self.query.query_history_cursor = None;
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
                    .curate(&self.project, self.review.replacement_policy, false)
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
            KeyCode::Home => self.help.help_scroll = 0,
            KeyCode::End => self.scroll_help_end(),
            _ => {}
        }
    }

    async fn toggle_selected_history(&mut self, api: &ApiClient) {
        // Second press hides the chain and returns to the single-version
        // detail view — cheap UX for users who don't want a dedicated
        // close key.
        if self.memories.selected_history.is_some() {
            self.memories.selected_history = None;
            self.memories.memory_detail_scroll = 0;
            self.status_message = "Hid version history.".to_string();
            return;
        }
        let Some(item) = self
            .memories
            .filtered_memories
            .get(self.memories.selected_index)
        else {
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
                self.memories.selected_history = Some(history);
                self.memories.memory_detail_scroll = 0;
                self.memories.memories_focus = MemoriesFocus::Detail;
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
                    self.query.query_history_cursor = None;
                }
                self.status_message = "Cancelled input mode.".to_string();
            }
            KeyCode::Enter => {
                match kind {
                    TextInputKind::Search => self.filters.text = buffer.clone(),
                    TextInputKind::Tag => self.filters.tag = buffer.clone(),
                    TextInputKind::Query => self.query.query_text = buffer.clone(),
                }
                self.input_mode = InputMode::Normal;
                match kind {
                    TextInputKind::Query => {
                        self.query.query_history_cursor = None;
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
                    self.query.query_history_cursor = None;
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
                    self.query.query_history_cursor = None;
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
        self.query.query_history_cursor = None;
        self.status_message =
            "Type a question, Enter to run, Up/Down for history, Esc to cancel.".to_string();
    }

    fn remember_query_history_entry(&mut self) {
        let question = self.query.query_text.trim();
        if question.is_empty() {
            return;
        }
        if self
            .query
            .query_history
            .iter()
            .any(|previous| previous.question == question)
        {
            return;
        }
        self.query.query_history.push(QueryHistoryEntry {
            question: question.to_string(),
            response: None,
            error: None,
            timing: None,
            initial_detail: None,
            running: false,
        });
        if self.query.query_history.len() > 50 {
            self.query.query_history.remove(0);
        }
    }

    fn start_query_history_run(&mut self, question: &str) {
        self.query.query_text = question.to_string();
        self.remember_query_history_entry();
        if let Some(entry) = self
            .query
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
            .query
            .query_history
            .iter()
            .all(|previous| previous.question != question)
        {
            self.query.query_text = question.to_string();
            self.remember_query_history_entry();
        }
        if let Some(entry) = self
            .query
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
            .query
            .query_history
            .iter()
            .all(|previous| previous.question != question)
        {
            self.query.query_text = question.to_string();
            self.remember_query_history_entry();
        }
        if let Some(entry) = self
            .query
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
        if self.query.query_history.is_empty() {
            self.status_message = "No previous queries in this TUI session.".to_string();
            return;
        }

        let last = self.query.query_history.len().saturating_sub(1);
        let next = match (self.query.query_history_cursor, delta) {
            (None, value) if value < 0 => Some(last),
            (None, value) if value > 0 => None,
            (Some(index), value) if value < 0 => Some(index.saturating_sub(1)),
            (Some(index), value) if value > 0 && index >= last => None,
            (Some(index), value) if value > 0 => Some(index + 1),
            (current, _) => current,
        };

        self.query.query_history_cursor = next;
        match next {
            Some(index) => {
                *buffer = self.query.query_history[index].question.clone();
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
        self.query.query_loading = false;
        self.query.query_started_at = None;
        self.query.query_pending_question = None;
        self.query.query_error = None;
        self.query.query_detail_loading = false;
        self.query.query_response = None;
        self.query.query_last_duration_ms = None;
        self.query.query_roundtrip_timing = None;
        self.query.query_selected_detail = None;
        self.query.query_selected_index = 0;
        self.query.query_table_state.select(None);
    }

    fn restore_query_history_entry(&mut self, index: usize) {
        let Some(entry) = self.query.query_history.get(index).cloned() else {
            self.clear_visible_query_state();
            self.status_message = "Query history item is unavailable.".to_string();
            return;
        };
        self.query.query_text = entry.question.clone();
        self.query.query_loading = entry.running;
        self.query.query_started_at = None;
        self.query.query_pending_question = entry.running.then_some(entry.question.clone());
        self.query.query_error = entry.error.clone();
        self.query.query_response = entry.response.clone();
        self.query.query_roundtrip_timing = entry.timing;
        self.query.query_last_duration_ms = entry.timing.map(|timing| timing.ui_ready_ms);
        self.query.query_selected_detail = if entry.response.is_some() {
            entry.initial_detail.clone()
        } else {
            None
        };
        self.query.query_detail_loading = false;
        self.query.query_selected_index = 0;
        if self.query_results().is_empty() {
            self.query.query_table_state.select(None);
        } else {
            self.query.query_table_state.select(Some(0));
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
            self.query.query_history.len()
        );
    }

    async fn move_selection(
        &mut self,
        delta: isize,
        api: &ApiClient,
        stream: Option<&mut StreamSession>,
    ) {
        if self.memories.filtered_memories.is_empty() {
            return;
        }
        let next = (self.memories.selected_index as isize + delta).clamp(
            0,
            self.memories.filtered_memories.len().saturating_sub(1) as isize,
        ) as usize;
        if next != self.memories.selected_index {
            self.memories.selected_index = next;
            self.memories
                .table_state
                .select(Some(self.memories.selected_index));
            self.fetch_selected_detail(api, stream).await;
        }
    }

    async fn fetch_selected_detail(
        &mut self,
        api: &ApiClient,
        mut stream: Option<&mut StreamSession>,
    ) {
        self.memories.selected_detail = None;
        self.memories.selected_history = None;
        self.memories.memory_detail_scroll = 0;
        self.memories.memories_focus = MemoriesFocus::List;
        if let Some(item) = self
            .memories
            .filtered_memories
            .get(self.memories.selected_index)
        {
            if let Some(stream) = stream.as_mut() {
                if let Err(error) = stream
                    .send(StreamRequest::SubscribeMemory { memory_id: item.id })
                    .await
                {
                    self.status_message = error.to_string();
                }
            } else {
                match api.memory_detail(&item.id.to_string()).await {
                    Ok(detail) => self.memories.selected_detail = Some(detail),
                    Err(error) => self.status_message = error.to_string(),
                }
            }
        }
    }

    fn apply_filters(&mut self) {
        self.memories.filtered_memories = self
            .memories
            .all_memories
            .iter()
            .filter(|item| self.filters.matches(item))
            .cloned()
            .collect();

        if self.memories.filtered_memories.is_empty() {
            self.memories.selected_index = 0;
            self.memories.table_state.select(None);
            self.memories.selected_detail = None;
            self.memories.selected_history = None;
            self.memories.memories_focus = MemoriesFocus::List;
        } else {
            self.memories.selected_index = self
                .memories
                .selected_index
                .min(self.memories.filtered_memories.len() - 1);
            self.memories
                .table_state
                .select(Some(self.memories.selected_index));
        }
    }

    fn apply_stream_response(&mut self, response: StreamResponse) {
        self.needs_redraw = true;
        match response {
            StreamResponse::ProjectSnapshot { overview, memories }
            | StreamResponse::ProjectChanged { overview, memories } => {
                self.overview = overview;
                self.memories.total_memories = memories.total;
                self.memories.all_memories = memories.items;
                self.apply_filters();
                self.resume.resume_loaded = false;
                self.status_message = format!(
                    "Streaming update: {} visible memories ({} total).",
                    self.memories.filtered_memories.len(),
                    self.memories.total_memories
                );
                self.ui_status = UiStatus::Ready;
            }
            StreamResponse::MemorySnapshot { detail }
            | StreamResponse::MemoryChanged { detail } => {
                self.memories.selected_detail = detail;
                self.memories.memory_detail_scroll = 0;
                self.memories.memories_focus = MemoriesFocus::List;
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

        let question = self.query.query_text.trim();
        self.query.query_request_id = self.query.query_request_id.saturating_add(1);
        let request_id = self.query.query_request_id;
        let question = question.to_string();
        self.start_query_history_run(&question);
        self.query.query_loading = true;
        self.query.query_started_at = Some(Instant::now());
        self.query.query_pending_question = Some(question.clone());
        self.query.query_error = None;
        self.query.query_selected_detail = None;
        self.query.query_roundtrip_timing = None;
        self.query.query_detail_loading = false;
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
        if self.query.query_text.trim().is_empty() {
            self.query.query_loading = false;
            self.query.query_started_at = None;
            self.query.query_pending_question = None;
            self.query.query_error = None;
            self.query.query_detail_loading = false;
            self.query.query_response = None;
            self.query.query_last_duration_ms = None;
            self.query.query_roundtrip_timing = None;
            self.query.query_selected_detail = None;
            self.query.query_selected_index = 0;
            self.query.query_table_state.select(None);
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
        if request_id != self.query.query_request_id {
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
        self.query.query_loading = false;
        self.query.query_started_at = None;
        self.query.query_pending_question = None;
        self.query.query_detail_request_id = self.query.query_detail_request_id.saturating_add(1);
        self.query.query_detail_loading = false;
        match response {
            Ok(response) => {
                self.record_query_activity(
                    request.clone(),
                    timing.ui_ready_ms,
                    QueryLogOutcome::Success(Box::new(response.clone())),
                );
                self.resume.resume_loaded = false;
                self.query.query_error = None;
                self.query.query_last_duration_ms = Some(timing.ui_ready_ms);
                self.query.query_roundtrip_timing = Some(timing);
                let response_for_history = response.clone();
                self.query.query_response = Some(response);
                self.query.query_selected_index = 0;
                let mut loaded_initial_detail = None;
                if self.query_results().is_empty() {
                    self.query.query_selected_detail = None;
                    self.query.query_table_state.select(None);
                } else {
                    self.query.query_table_state.select(Some(0));
                    match initial_detail {
                        Some(Ok(detail)) => {
                            loaded_initial_detail = Some(detail.clone());
                            self.query.query_selected_detail = Some(detail);
                        }
                        Some(Err(error)) => {
                            self.query.query_selected_detail = None;
                            self.status_message = format!("Query detail unavailable: {error}");
                        }
                        None => self.query.query_selected_detail = None,
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
                self.resume.resume_loaded = false;
                self.query.query_response = None;
                self.query.query_last_duration_ms = Some(timing.ui_ready_ms);
                self.query.query_roundtrip_timing = Some(timing);
                self.query.query_selected_detail = None;
                self.query.query_table_state.select(None);
                self.query.query_error = Some(error.clone());
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
        let next = (self.query.query_selected_index as isize + delta)
            .clamp(0, self.query_results().len().saturating_sub(1) as isize)
            as usize;
        if next != self.query.query_selected_index {
            self.query.query_selected_index = next;
            self.query
                .query_table_state
                .select(Some(self.query.query_selected_index));
            self.fetch_selected_query_detail(api);
        }
    }

    fn fetch_selected_query_detail(&mut self, api: &ApiClient) {
        self.query.query_selected_detail = None;
        self.query.query_detail_loading = false;
        if let Some(memory_id) = self
            .query_results()
            .get(self.query.query_selected_index)
            .map(|result| result.memory_id.to_string())
        {
            self.query.query_detail_request_id =
                self.query.query_detail_request_id.saturating_add(1);
            let request_id = self.query.query_detail_request_id;
            self.query.query_detail_loading = true;
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
        if request_id != self.query.query_detail_request_id {
            return;
        }
        let selected_memory_id = self
            .query_results()
            .get(self.query.query_selected_index)
            .map(|result| result.memory_id.to_string());
        if selected_memory_id.as_deref() != Some(memory_id.as_str()) {
            return;
        }
        self.query.query_detail_loading = false;
        match detail {
            Ok(detail) => self.query.query_selected_detail = Some(detail),
            Err(error) => {
                self.query.query_selected_detail = None;
                self.status_message = format!("Query detail unavailable: {error}");
            }
        }
    }

    pub(in crate::tui) fn query_results(&self) -> &[QueryResult] {
        self.query
            .query_response
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
        self.activity.activity_events.insert(
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
        self.activity.activity_events.retain(|entry| match entry {
            ActivityEntry::Backend(existing) => existing.id != event.id,
            ActivityEntry::Query(_) => true,
        });
        self.activity
            .activity_events
            .insert(0, ActivityEntry::Backend(Box::new(event)));
        self.finish_activity_insert();
    }

    fn finish_activity_insert(&mut self) {
        if self.activity.activity_events.len() > 200 {
            self.activity.activity_events.truncate(200);
        }
        self.activity.activity_selected_index = 0;
        if self.activity.activity_events.is_empty() {
            self.activity.activity_table_state.select(None);
        } else {
            self.activity.activity_table_state.select(Some(0));
        }
        self.activity.activity_detail_scroll = 0;
    }

    fn move_activity_selection(&mut self, delta: isize) {
        if self.activity.activity_events.is_empty() {
            return;
        }
        let next = (self.activity.activity_selected_index as isize + delta).clamp(
            0,
            self.activity.activity_events.len().saturating_sub(1) as isize,
        ) as usize;
        if next != self.activity.activity_selected_index {
            self.activity.activity_selected_index = next;
            self.activity
                .activity_table_state
                .select(Some(self.activity.activity_selected_index));
        }
    }

    fn move_error_selection(&mut self, delta: isize) {
        let len = collect_error_items(self).len();
        if len == 0 {
            self.errors.errors_selected_index = 0;
            self.errors.errors_table_state.select(None);
            return;
        }
        let next = (self.errors.errors_selected_index as isize + delta)
            .clamp(0, len.saturating_sub(1) as isize) as usize;
        if next != self.errors.errors_selected_index {
            self.errors.errors_selected_index = next;
            self.errors.errors_table_state.select(Some(next));
            self.errors.errors_detail_scroll = 0;
        }
    }

    fn select_replacement_proposal(&mut self, delta: isize) {
        let len = self.review.replacement_proposals.len();
        if len == 0 {
            self.review.replacement_selected_index = 0;
            self.review.review_table_state.select(None);
            return;
        }
        // Cyclic wrap so j/k/[ ] loops within the list.
        let cur = self.review.replacement_selected_index as isize;
        let next = ((cur + delta) % len as isize + len as isize) % len as isize;
        self.review.replacement_selected_index = next as usize;
        self.review
            .review_table_state
            .select(Some(self.review.replacement_selected_index));
    }

    fn jump_replacement_proposal(&mut self, index: usize) {
        let len = self.review.replacement_proposals.len();
        if len == 0 {
            self.review.replacement_selected_index = 0;
            self.review.review_table_state.select(None);
            return;
        }
        self.review.replacement_selected_index = index.min(len - 1);
        self.review
            .review_table_state
            .select(Some(self.review.replacement_selected_index));
    }

    async fn cycle_replacement_policy(&mut self) -> Result<()> {
        self.review.replacement_policy = match self.review.replacement_policy {
            ReplacementPolicy::Conservative => ReplacementPolicy::Balanced,
            ReplacementPolicy::Balanced => ReplacementPolicy::Aggressive,
            ReplacementPolicy::Aggressive => ReplacementPolicy::Conservative,
        };
        write_replacement_policy(&self.repo_root, self.review.replacement_policy)?;
        self.status_message = format!(
            "Curation replacement policy set to {}.",
            self.review.replacement_policy
        );
        Ok(())
    }

    async fn approve_selected_replacement_proposal(&mut self, api: &ApiClient) -> Result<()> {
        let Some(proposal) = self
            .review
            .replacement_proposals
            .get(self.review.replacement_selected_index)
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
            .review
            .replacement_proposals
            .get(self.review.replacement_selected_index)
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
        let Some(item) = self
            .memories
            .filtered_memories
            .get(self.memories.selected_index)
        else {
            self.status_message = "No selected memory to delete.".to_string();
            return Ok(());
        };
        let response = api.delete_memory(item.id).await?;
        self.status_message = format!("Deleted memory: {}", response.summary);
        self.refresh(api, RefreshMode::Full).await;
        Ok(())
    }

    async fn delete_selected_query_memory(&mut self, api: &ApiClient) -> Result<()> {
        let Some(result) = self.query_results().get(self.query.query_selected_index) else {
            self.status_message = "No selected query result to delete.".to_string();
            return Ok(());
        };
        let response = api.delete_memory(result.memory_id).await?;
        self.status_message = format!("Deleted memory: {}", response.summary);
        self.query.query_selected_detail = None;
        self.run_query(api);
        self.refresh(api, RefreshMode::Full).await;
        Ok(())
    }

    fn scroll_project(&mut self, delta: i16) {
        self.project_tab.project_scroll = if delta.is_negative() {
            self.project_tab
                .project_scroll
                .saturating_sub(delta.unsigned_abs())
        } else {
            self.project_tab
                .project_scroll
                .saturating_add(u16::try_from(delta).unwrap_or(0))
        };
    }

    fn scroll_resume(&mut self, delta: i16) {
        self.resume.resume_scroll = if delta.is_negative() {
            self.resume
                .resume_scroll
                .saturating_sub(delta.unsigned_abs())
        } else {
            self.resume
                .resume_scroll
                .saturating_add(u16::try_from(delta).unwrap_or(0))
        };
    }

    fn scroll_watchers(&mut self, delta: i16) {
        self.watchers.watcher_scroll = if delta.is_negative() {
            self.watchers
                .watcher_scroll
                .saturating_sub(delta.unsigned_abs())
        } else {
            self.watchers
                .watcher_scroll
                .saturating_add(u16::try_from(delta).unwrap_or(0))
        };
    }

    fn open_help_for_active_tab(&mut self) {
        self.help.help_open = true;
        self.help.help_tab = self.active_tab;
        self.help.help_scroll = 0;
        self.status_message = format!(
            "Showing {} help. Press h or Esc to return.",
            self.help.help_tab.label()
        );
    }

    fn close_help(&mut self) {
        self.help.help_open = false;
        self.help.help_scroll = 0;
        self.status_message = "Help closed.".to_string();
    }

    fn scroll_help(&mut self, delta: i16) {
        let area = current_frame_area().unwrap_or_else(default_frame_area);
        self.scroll_help_in_area(delta, area);
    }

    fn scroll_help_in_area(&mut self, delta: i16, frame_area: Rect) {
        let max_scroll = help_max_scroll(self.help.help_tab, frame_area);
        self.help.help_scroll = if delta.is_negative() {
            self.help.help_scroll.saturating_sub(delta.unsigned_abs())
        } else {
            self.help
                .help_scroll
                .saturating_add(u16::try_from(delta).unwrap_or(0))
        }
        .min(max_scroll);
    }

    fn scroll_help_end(&mut self) {
        let area = current_frame_area().unwrap_or_else(default_frame_area);
        self.help.help_scroll = help_max_scroll(self.help.help_tab, area);
    }

    fn scroll_memory_detail(&mut self, delta: i16) {
        let area = current_frame_area().unwrap_or_else(default_frame_area);
        self.scroll_memory_detail_in_area(delta, area);
    }

    fn scroll_memory_detail_in_area(&mut self, delta: i16, frame_area: Rect) {
        let max_scroll = memory_detail_max_scroll(self, frame_area);
        self.memories.memory_detail_scroll = if delta.is_negative() {
            self.memories
                .memory_detail_scroll
                .saturating_sub(delta.unsigned_abs())
        } else {
            self.memories
                .memory_detail_scroll
                .saturating_add(u16::try_from(delta).unwrap_or(0))
        }
        .min(max_scroll);
    }

    fn scroll_memory_detail_home(&mut self) {
        self.memories.memory_detail_scroll = 0;
    }

    fn scroll_memory_detail_end(&mut self) {
        let area = current_frame_area().unwrap_or_else(default_frame_area);
        self.memories.memory_detail_scroll = memory_detail_max_scroll(self, area);
    }

    fn toggle_memories_focus(&mut self) {
        self.memories.memories_focus = match self.memories.memories_focus {
            MemoriesFocus::List if self.memories.selected_detail.is_some() => MemoriesFocus::Detail,
            MemoriesFocus::Detail => MemoriesFocus::List,
            MemoriesFocus::List => MemoriesFocus::List,
        };
    }

    fn focus_memories_list(&mut self) {
        self.memories.memories_focus = MemoriesFocus::List;
    }
}

pub(super) struct StreamSession {
    writer: tokio::io::WriteHalf<StreamTransport>,
    rx: mpsc::UnboundedReceiver<StreamResponse>,
}

pub(super) enum StreamTransport {
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

#[cfg(test)]
mod tests;
