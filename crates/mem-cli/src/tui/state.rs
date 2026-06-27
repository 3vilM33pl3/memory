use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool, mpsc as std_mpsc},
    time::Instant,
};

use chrono::{DateTime, Utc};
use mem_agenttop::AgentSnapshot;
use mem_api::{
    ActivityEvent, EffectiveLoopSettings, LlmAuditStatusResponse, LoopApprovalRequestRecord,
    LoopDefinitionRecord, LoopGlobalStateResponse, LoopRunSummary, MemoryEntryResponse,
    MemoryStatus, MemoryType, Profile, ProjectMemoriesResponse, ProjectMemoryListItem,
    ProjectOverviewResponse, QueryRequest, QueryResponse, ReplacementPolicy,
    ReplacementProposalListResponse, ReplacementProposalRecord, ResumeResponse, UpToSpeedResponse,
};
use ratatui::widgets::TableState;
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::commands::service_support::TuiRestartNotice;
use mem_skills::SkillInventoryReport;

use super::app::StreamSession;

pub(super) struct App {
    pub(in crate::tui) project: String,
    pub(in crate::tui) repo_root: PathBuf,
    pub(in crate::tui) active_tab: TabKind,
    pub(in crate::tui) service: ServiceState,
    pub(in crate::tui) chrome: UiChrome,
    pub(in crate::tui) meta: RuntimeMeta,
    pub(in crate::tui) memories: MemoriesTabState,
    pub(in crate::tui) query: QueryTabState,
    pub(in crate::tui) agents: AgentsTabState,
    pub(in crate::tui) resume: ResumeTabState,
    pub(in crate::tui) activity: ActivityTabState,
    pub(in crate::tui) errors: ErrorsTabState,
    pub(in crate::tui) project_tab: ProjectTabState,
    pub(in crate::tui) review: ReviewTabState,
    pub(in crate::tui) watchers: WatchersTabState,
    pub(in crate::tui) skills: SkillsTabState,
    pub(in crate::tui) automations: AutomationsTabState,
    pub(in crate::tui) embeddings: EmbeddingsTabState,
    pub(in crate::tui) filters: Filters,
    pub(in crate::tui) background_tx: mpsc::UnboundedSender<BackgroundEvent>,
}

pub(super) struct ServiceState {
    pub(in crate::tui) health_ok: bool,
    pub(in crate::tui) backend_connection_state: BackendConnectionState,
    pub(in crate::tui) service_role: Option<String>,
    pub(in crate::tui) service_health_state: Option<String>,
    pub(in crate::tui) service_database_state: Option<String>,
    pub(in crate::tui) offline_pending_count: Option<u64>,
    pub(in crate::tui) offline_database_path: Option<String>,
    pub(in crate::tui) manager_status: Option<ManagerFooterStatus>,
    pub(in crate::tui) restart_notice: Option<TuiRestartNotice>,
    pub(in crate::tui) stream_connecting: bool,
    pub(in crate::tui) relay_discovery_enabled: bool,
}

pub(super) struct UiChrome {
    pub(in crate::tui) help: HelpState,
    pub(in crate::tui) ui_status: UiStatus,
    pub(in crate::tui) status_message: String,
    pub(in crate::tui) input_mode: InputMode,
    pub(in crate::tui) needs_redraw: bool,
}

pub(super) struct RuntimeMeta {
    pub(in crate::tui) overview: ProjectOverviewResponse,
    pub(in crate::tui) versions: ToolVersions,
    pub(in crate::tui) skill_inventory: SkillInventoryReport,
    pub(in crate::tui) startup_at: DateTime<Utc>,
    pub(in crate::tui) profile: Profile,
    pub(in crate::tui) dev_commit_label: Option<String>,
}

pub(super) struct MemoriesTabState {
    pub(in crate::tui) all_memories: Vec<ProjectMemoryListItem>,
    pub(in crate::tui) filtered_memories: Vec<ProjectMemoryListItem>,
    pub(in crate::tui) total_memories: i64,
    pub(in crate::tui) selected_detail: Option<MemoryEntryResponse>,
    pub(in crate::tui) selected_history: Option<mem_api::MemoryHistoryResponse>,
    pub(in crate::tui) selected_index: usize,
    pub(in crate::tui) table_state: TableState,
    pub(in crate::tui) memories_focus: MemoriesFocus,
    pub(in crate::tui) memory_detail_scroll: u16,
}

pub(super) struct QueryTabState {
    pub(in crate::tui) query_text: String,
    pub(in crate::tui) query_history: Vec<QueryHistoryEntry>,
    pub(in crate::tui) query_history_cursor: Option<usize>,
    pub(in crate::tui) query_response: Option<QueryResponse>,
    pub(in crate::tui) query_last_duration_ms: Option<u64>,
    pub(in crate::tui) query_roundtrip_timing: Option<QueryRoundtripTiming>,
    pub(in crate::tui) query_selected_detail: Option<MemoryEntryResponse>,
    pub(in crate::tui) query_selected_index: usize,
    pub(in crate::tui) query_table_state: TableState,
    pub(in crate::tui) query_loading: bool,
    pub(in crate::tui) query_started_at: Option<Instant>,
    pub(in crate::tui) query_pending_question: Option<String>,
    pub(in crate::tui) query_error: Option<String>,
    pub(in crate::tui) query_request_id: u64,
    pub(in crate::tui) query_detail_loading: bool,
    pub(in crate::tui) query_detail_request_id: u64,
}

pub(super) struct AgentsTabState {
    pub(in crate::tui) agent_snapshot: Option<AgentSnapshot>,
    pub(in crate::tui) agent_loading: bool,
    pub(in crate::tui) agent_error: Option<String>,
    pub(in crate::tui) agent_selected_index: usize,
    pub(in crate::tui) agent_table_state: TableState,
    pub(in crate::tui) agent_detail_scroll: u16,
    pub(in crate::tui) agent_initial_selection_done: bool,
    pub(in crate::tui) agents_tab_visible: Arc<AtomicBool>,
    pub(in crate::tui) agent_wake_tx: std_mpsc::Sender<()>,
    pub(in crate::tui) agent_wake_rx: Option<std_mpsc::Receiver<()>>,
}

pub(super) struct ResumeTabState {
    pub(in crate::tui) resume_response: Option<ResumeResponse>,
    pub(in crate::tui) resume_loading: bool,
    pub(in crate::tui) resume_loaded: bool,
    pub(in crate::tui) resume_error: Option<String>,
    pub(in crate::tui) resume_scroll: u16,
    pub(in crate::tui) startup_resume_autoselect_pending: bool,
}

pub(super) struct ActivityTabState {
    pub(in crate::tui) activity_events: Vec<ActivityEntry>,
    pub(in crate::tui) activity_selected_index: usize,
    pub(in crate::tui) activity_table_state: TableState,
    pub(in crate::tui) activity_loading: bool,
    pub(in crate::tui) activity_error: Option<String>,
    pub(in crate::tui) activity_detail_scroll: u16,
    pub(in crate::tui) llm_audit_status: Option<LlmAuditStatusResponse>,
    pub(in crate::tui) llm_audit_loading: bool,
    pub(in crate::tui) llm_audit_toggling: bool,
    pub(in crate::tui) llm_audit_error: Option<String>,
    pub(in crate::tui) up_to_speed_response: Option<UpToSpeedResponse>,
    pub(in crate::tui) up_to_speed_loading: bool,
    pub(in crate::tui) up_to_speed_error: Option<String>,
}

pub(super) struct ErrorsTabState {
    pub(in crate::tui) errors_selected_index: usize,
    pub(in crate::tui) errors_table_state: TableState,
    pub(in crate::tui) errors_detail_scroll: u16,
}

pub(super) struct ProjectTabState {
    pub(in crate::tui) project_scroll: u16,
}

pub(super) struct WatchersTabState {
    pub(in crate::tui) watcher_scroll: u16,
}

pub(super) struct SkillsTabState {
    pub(in crate::tui) selected_index: usize,
    pub(in crate::tui) table_state: TableState,
    pub(in crate::tui) detail_scroll: u16,
    pub(in crate::tui) operation: Option<String>,
    pub(in crate::tui) message: Option<String>,
}

pub(super) struct AutomationsTabState {
    pub(in crate::tui) snapshot: Option<AutomationSnapshot>,
    pub(in crate::tui) error: Option<String>,
    pub(in crate::tui) selected_index: usize,
    pub(in crate::tui) table_state: TableState,
    pub(in crate::tui) detail_scroll: u16,
}

#[derive(Clone)]
pub(super) struct AutomationSnapshot {
    pub(in crate::tui) items: Vec<AutomationListItem>,
    pub(in crate::tui) pending_approvals: Vec<LoopApprovalRequestRecord>,
    pub(in crate::tui) global_state: Option<LoopGlobalStateResponse>,
    pub(in crate::tui) warnings: Vec<String>,
}

#[derive(Clone)]
pub(super) struct AutomationListItem {
    pub(in crate::tui) definition: LoopDefinitionRecord,
    pub(in crate::tui) effective_settings: Option<EffectiveLoopSettings>,
    pub(in crate::tui) latest_run: Option<LoopRunSummary>,
}

pub(super) struct HelpState {
    pub(in crate::tui) help_open: bool,
    pub(in crate::tui) help_tab: TabKind,
    pub(in crate::tui) help_scroll: u16,
}

pub(super) struct ReviewTabState {
    pub(in crate::tui) replacement_policy: ReplacementPolicy,
    pub(in crate::tui) replacement_proposals: Vec<ReplacementProposalRecord>,
    pub(in crate::tui) replacement_selected_index: usize,
    pub(in crate::tui) review_table_state: TableState,
}

pub(super) struct EmbeddingsTabState {
    pub(in crate::tui) embedding_backends_snapshot: Option<mem_api::EmbeddingBackendsResponse>,
    pub(in crate::tui) embedding_backends_error: Option<String>,
    pub(in crate::tui) embeddings_selected_index: usize,
    pub(in crate::tui) embeddings_table_state: TableState,
    pub(in crate::tui) embeddings_tab_visible: Arc<AtomicBool>,
    pub(in crate::tui) embeddings_wake_tx: std_mpsc::Sender<()>,
    pub(in crate::tui) embeddings_wake_rx: Option<std_mpsc::Receiver<()>>,
    pub(in crate::tui) embeddings_toggle_message: Option<String>,
    pub(in crate::tui) embeddings_toggling: Option<String>,
    pub(in crate::tui) embeddings_creation_toggling: bool,
    pub(in crate::tui) embeddings_operation: Option<String>,
}

pub(super) struct ToolVersions {
    pub(in crate::tui) mem_cli: String,
    pub(in crate::tui) mem_service: String,
    pub(in crate::tui) watch_manager: String,
    pub(in crate::tui) memory_watch: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BackendConnectionState {
    Connecting,
    Connected,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum UiStatus {
    Loading,
    Busy,
    Ready,
    Restart,
    Error,
}

#[derive(Clone, Debug)]
pub(super) struct ManagerFooterStatus {
    pub(in crate::tui) state: ManagerState,
    pub(in crate::tui) tracked_sessions: usize,
    pub(in crate::tui) warning_count: usize,
    pub(in crate::tui) mode: Option<ManagerMode>,
    pub(in crate::tui) runtime_mode: Option<String>,
    pub(in crate::tui) last_reconcile_reason: Option<String>,
    pub(in crate::tui) event_count: u64,
    pub(in crate::tui) fallback_scan_count: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ManagerState {
    Active,
    Installed,
    Off,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ManagerMode {
    Service,
    Foreground,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct ManagerStateFile {
    #[serde(default)]
    pub(in crate::tui) mode: String,
    #[serde(default)]
    pub(in crate::tui) last_reconcile_reason: String,
    #[serde(default)]
    pub(in crate::tui) event_count: u64,
    #[serde(default)]
    pub(in crate::tui) fallback_scan_count: u64,
    #[serde(default)]
    pub(in crate::tui) sessions: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub(in crate::tui) warnings: Vec<String>,
}

pub(super) enum ActivityEntry {
    Backend(Box<ActivityEvent>),
    Query(QueryActivityEntry),
}

pub(super) struct QueryActivityEntry {
    pub(in crate::tui) recorded_at: DateTime<Utc>,
    pub(in crate::tui) project: String,
    pub(in crate::tui) request: QueryRequest,
    pub(in crate::tui) duration_ms: u64,
    pub(in crate::tui) outcome: QueryLogOutcome,
}

#[derive(Clone)]
pub(super) enum QueryLogOutcome {
    Success(Box<QueryResponse>),
    Error(String),
}

#[derive(Clone)]
pub(super) struct QueryHistoryEntry {
    pub(in crate::tui) question: String,
    pub(in crate::tui) response: Option<QueryResponse>,
    pub(in crate::tui) error: Option<String>,
    pub(in crate::tui) timing: Option<QueryRoundtripTiming>,
    pub(in crate::tui) initial_detail: Option<MemoryEntryResponse>,
    pub(in crate::tui) running: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct QueryRoundtripTiming {
    pub(in crate::tui) query_api_ms: u64,
    pub(in crate::tui) initial_detail_ms: Option<u64>,
    pub(in crate::tui) ui_ready_ms: u64,
}

#[derive(Clone)]
pub(super) struct ProjectRefreshResult {
    pub(in crate::tui) mode: RefreshMode,
    pub(in crate::tui) health: Result<serde_json::Value, String>,
    pub(in crate::tui) overview: Result<ProjectOverviewResponse, String>,
    pub(in crate::tui) memories: Result<ProjectMemoriesResponse, String>,
    pub(in crate::tui) proposals: Result<ReplacementProposalListResponse, String>,
    pub(in crate::tui) automations: Result<AutomationSnapshot, String>,
    pub(in crate::tui) skill_inventory: SkillInventoryReport,
}

pub(super) enum BackgroundEvent {
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
    SkillsRepairCompleted {
        result: Result<mem_skills::SkillUpgradeReport, String>,
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
pub(super) enum RefreshMode {
    Startup,
    Full,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MemoriesFocus {
    List,
    Detail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TabKind {
    Memories,
    Agents,
    Query,
    Activity,
    Errors,
    Project,
    Review,
    Watchers,
    Skills,
    Automations,
    Embeddings,
    Resume,
}

pub(super) const VISIBLE_TABS: [TabKind; 12] = [
    TabKind::Memories,
    TabKind::Agents,
    TabKind::Query,
    TabKind::Activity,
    TabKind::Errors,
    TabKind::Project,
    TabKind::Review,
    TabKind::Watchers,
    TabKind::Skills,
    TabKind::Automations,
    TabKind::Embeddings,
    TabKind::Resume,
];

impl TabKind {
    pub(in crate::tui) fn label(self) -> &'static str {
        match self {
            Self::Memories => "Memories",
            Self::Agents => "Agents",
            Self::Query => "Query",
            Self::Activity => "Activity",
            Self::Errors => "Errors",
            Self::Project => "Project",
            Self::Review => "Review",
            Self::Watchers => "Watchers",
            Self::Skills => "Skills",
            Self::Automations => "Automations",
            Self::Embeddings => "Embeddings",
            Self::Resume => "Resume",
        }
    }

    pub(in crate::tui) fn next(self) -> Self {
        match self {
            Self::Memories => Self::Agents,
            Self::Agents => Self::Query,
            Self::Query => Self::Activity,
            Self::Activity => Self::Errors,
            Self::Errors => Self::Project,
            Self::Project => Self::Review,
            Self::Review => Self::Watchers,
            Self::Watchers => Self::Skills,
            Self::Skills => Self::Automations,
            Self::Automations => Self::Embeddings,
            Self::Embeddings => Self::Resume,
            Self::Resume => Self::Memories,
        }
    }

    pub(in crate::tui) fn prev(self) -> Self {
        match self {
            Self::Memories => Self::Resume,
            Self::Agents => Self::Memories,
            Self::Query => Self::Agents,
            Self::Activity => Self::Query,
            Self::Errors => Self::Activity,
            Self::Project => Self::Errors,
            Self::Review => Self::Project,
            Self::Watchers => Self::Review,
            Self::Skills => Self::Watchers,
            Self::Automations => Self::Skills,
            Self::Embeddings => Self::Automations,
            Self::Resume => Self::Embeddings,
        }
    }

    pub(in crate::tui) fn index(self) -> usize {
        match self {
            Self::Memories => 0,
            Self::Agents => 1,
            Self::Query => 2,
            Self::Activity => 3,
            Self::Errors => 4,
            Self::Project => 5,
            Self::Review => 6,
            Self::Watchers => 7,
            Self::Skills => 8,
            Self::Automations => 9,
            Self::Embeddings => 10,
            Self::Resume => 11,
        }
    }
}

#[derive(Clone, Default)]
pub(super) struct Filters {
    pub(in crate::tui) text: String,
    pub(in crate::tui) tag: String,
    pub(in crate::tui) status: StatusFilter,
    pub(in crate::tui) memory_type: TypeFilter,
}

impl Filters {
    pub(in crate::tui) fn matches(&self, item: &ProjectMemoryListItem) -> bool {
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
pub(super) enum InputMode {
    #[default]
    Normal,
    Search(String),
    Tag(String),
    Query(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TextInputKind {
    Search,
    Tag,
    Query,
}

impl TextInputKind {
    pub(in crate::tui) fn wrap(self, value: String) -> InputMode {
        match self {
            Self::Search => InputMode::Search(value),
            Self::Tag => InputMode::Tag(value),
            Self::Query => InputMode::Query(value),
        }
    }
}

#[derive(Clone, Default)]
pub(super) enum StatusFilter {
    #[default]
    All,
    Active,
    Archived,
}

impl StatusFilter {
    pub(in crate::tui) fn next(&self) -> Self {
        match self {
            Self::All => Self::Active,
            Self::Active => Self::Archived,
            Self::Archived => Self::All,
        }
    }

    pub(in crate::tui) fn matches(&self, status: MemoryStatus) -> bool {
        matches!(
            (self, status),
            (Self::All, _)
                | (Self::Active, MemoryStatus::Active)
                | (Self::Archived, MemoryStatus::Archived)
        )
    }

    pub(in crate::tui) fn label(&self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Active => "active",
            Self::Archived => "archived",
        }
    }
}

#[derive(Clone, Default)]
pub(super) enum TypeFilter {
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
    Refactor,
}

impl TypeFilter {
    pub(in crate::tui) fn next(&self) -> Self {
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
            Self::Implementation => Self::Refactor,
            Self::Refactor => Self::All,
        }
    }

    pub(in crate::tui) fn matches(&self, memory_type: &MemoryType) -> bool {
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
                | (Self::Refactor, MemoryType::Refactor)
        )
    }

    pub(in crate::tui) fn label(&self) -> &'static str {
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
            Self::Refactor => "refactor",
        }
    }
}
