use crate::prelude::*;
use crate::*;

pub(crate) const QUERY_ACTIVITY_GRAPH_CONNECTION_LIMIT: usize = 5;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) role: ServiceRole,
    pub(crate) instance_id: String,
    pub(crate) startup_at: chrono::DateTime<chrono::Utc>,
    pub(crate) pool: Arc<RwLock<Option<PgPool>>>,
    pub(crate) offline: Option<OfflineRuntime>,
    pub(crate) api_token: String,
    pub(crate) config: AppConfig,
    pub(crate) web_root: Option<PathBuf>,
    pub(crate) http_client: reqwest::Client,
    pub(crate) embedders: Arc<tokio::sync::RwLock<EmbeddingRegistry>>,
    pub(crate) automated_embedding_creation_enabled: Arc<AtomicBool>,
    pub(crate) llm_audit: Arc<RwLock<LlmAuditConfig>>,
    pub(crate) events: broadcast::Sender<ServiceEvent>,
    pub(crate) recent_activity: Arc<Mutex<VecDeque<ServiceEvent>>>,
    pub(crate) watchers: Arc<Mutex<HashMap<String, WatcherPresence>>>,
    pub(crate) provenance: Arc<Mutex<ProvenanceRuntimeState>>,
    pub(crate) cluster: ClusterRuntime,
    pub(crate) shutdown: Arc<Mutex<Option<oneshot::Sender<()>>>>,
}

#[derive(Clone, Debug)]
pub(crate) struct ServiceEvent {
    pub(crate) id: Uuid,
    pub(crate) project: String,
    pub(crate) memory_id: Option<Uuid>,
    pub(crate) kind: ActivityKind,
    pub(crate) summary: String,
    pub(crate) details: Option<ActivityDetails>,
    pub(crate) recorded_at: chrono::DateTime<chrono::Utc>,
    pub(crate) actor_id: Option<String>,
    pub(crate) actor_name: Option<String>,
    pub(crate) source: Option<String>,
    pub(crate) operation_id: Option<String>,
    pub(crate) duration_ms: Option<u64>,
    pub(crate) provider: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) token_usage: Option<TokenUsage>,
    pub(crate) include_activity: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct ProvenanceRuntimeState {
    pub(crate) status: String,
    pub(crate) last_started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub(crate) last_finished_at: Option<chrono::DateTime<chrono::Utc>>,
    pub(crate) last_project: Option<String>,
    pub(crate) checked_count: usize,
    pub(crate) stale_count: usize,
    pub(crate) error: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) enum ServiceRole {
    Primary,
    Relay,
}

#[derive(Clone, Debug)]
pub(crate) struct ClusterRuntime {
    pub(crate) peers: Arc<Mutex<HashMap<String, ClusterPeer>>>,
}

#[derive(Clone, Debug, Serialize, SerdeDeserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DiscoveryKind {
    Discover,
    Announce,
}

#[derive(Clone, Debug, Serialize, SerdeDeserialize)]
pub(crate) struct DiscoveryPacket {
    pub(crate) kind: DiscoveryKind,
    pub(crate) service_id: String,
    pub(crate) advertise_addr: String,
    pub(crate) version: String,
    pub(crate) priority: i32,
    pub(crate) sent_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Clone, Debug)]
pub(crate) struct ClusterPeer {
    pub(crate) service_id: String,
    pub(crate) advertise_addr: String,
    pub(crate) version: String,
    pub(crate) priority: i32,
    pub(crate) last_seen: chrono::DateTime<chrono::Utc>,
}

pub(crate) const WATCHER_STALE_AFTER_SECONDS: u64 = 90;
pub(crate) const WATCHER_RESTART_BACKOFF_SECONDS: u64 = 120;
pub(crate) const WATCHER_EXPIRY_AFTER_SECONDS: u64 = 600;
pub(crate) const WATCHER_MAX_RESTART_ATTEMPTS: u32 = 3;

impl AppState {
    pub(crate) fn is_primary(&self) -> bool {
        matches!(self.role, ServiceRole::Primary)
    }

    pub(crate) fn role_name(&self) -> &'static str {
        match self.role {
            ServiceRole::Primary => "primary",
            ServiceRole::Relay => "relay",
        }
    }

    pub(crate) fn pool(&self) -> Result<PgPool, ApiError> {
        self.pool
            .read()
            .expect("database pool lock poisoned")
            .clone()
            .ok_or_else(|| {
                if self.offline.is_some() {
                    ApiError::service_unavailable(
                        "PostgreSQL is unavailable; service is running in offline degraded mode",
                    )
                } else {
                    ApiError::service_unavailable("relay has no local database connection")
                }
            })
    }

    pub(crate) fn pool_available(&self) -> bool {
        self.pool
            .read()
            .expect("database pool lock poisoned")
            .is_some()
    }

    pub(crate) fn set_pool(&self, pool: PgPool) {
        *self.pool.write().expect("database pool lock poisoned") = Some(pool);
    }

    pub(crate) fn offline_store(&self) -> Option<OfflineStore> {
        self.offline.as_ref().map(|runtime| runtime.store.clone())
    }
}
