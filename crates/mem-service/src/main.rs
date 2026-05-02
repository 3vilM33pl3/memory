use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    io::ErrorKind,
    io::Read,
    net::SocketAddr,
    path::{Path as FsPath, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration as StdDuration, SystemTime},
};

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::{any, delete, get, post},
};
use futures_util::{SinkExt, StreamExt};
use mem_api::{
    ActivateEmbeddingBackendRequest, ActivityDetails, ActivityEvent, ActivityKind,
    ActivityListResponse, AppConfig, ArchiveRequest, ArchiveResponse, CaptureTaskRequest,
    CheckpointActivityRequest, CommitDetailResponse, CommitSyncRequest, CommitSyncResponse,
    CurateRequest, DeleteMemoryRequest, DeleteMemoryResponse, EmbeddingBackendInfo,
    EmbeddingBackendsResponse, GraphActivityRequest, MemoryEntryResponse, MemoryHistoryResponse,
    MemorySourceRecord, PlanActivityAction, PlanActivityRequest, ProjectCommitsResponse,
    ProjectMemoriesResponse, ProjectMemoryBundleEntry, ProjectMemoryBundleEntryRelation,
    ProjectMemoryBundleManifest, ProjectMemoryBundlePreview, ProjectMemoryBundleSource,
    ProjectMemoryExportOptions, ProjectMemoryImportPreview, ProjectMemoryImportResponse,
    ProjectMemoryListItem, ProjectOverviewResponse, PruneEmbeddingsRequest,
    PruneEmbeddingsResponse, PruneHistoryRequest, PruneHistoryResponse, QueryAnswerCitation,
    QueryAnswerGeneration, QueryAnswerMethod, QueryAnswerMode, QueryGraphConnection, QueryRequest,
    QueryResponse, ReembedRequest, ReembedResponse, ReindexRequest, ReindexResponse,
    RelatedMemorySummary, ReplacementPolicy, ReplacementPolicyRequest, ReplacementPolicyResponse,
    ReplacementProposalListResponse, ReplacementProposalResolutionResponse, ResumeAction,
    ResumeCheckpoint, ResumeRequest, ResumeResponse, ScanActivityRequest,
    SetEmbeddingCreationRequest, SourceKind, StatsResponse, StreamRequest, StreamResponse,
    TokenUsage, TokenUsageSummary, UpToSpeedRequest, UpToSpeedResponse, ValidationError,
    WatcherHealth, WatcherHeartbeatRequest, WatcherPresence, WatcherPresenceSummary,
    WatcherRestartRequest, WatcherRestartResponse, WatcherUnregisterRequest,
    load_repo_replacement_policy, read_capnp_text_frame, repo_agent_settings_path,
    write_capnp_text_frame,
};
use mem_curate::{
    approve_replacement_proposal, curate, list_replacement_proposals, preview_capture,
    preview_curate, refresh_memory_relations, reject_replacement_proposal, store_capture,
};
use mem_platform::{
    managed_watch_service_name, restart_local_watcher_service_name, watch_service_unit_name,
};
use mem_search::{
    EmbeddingRegistry, effective_embedding_base_url, parse_memory_type, parse_relation_type,
    parse_source_kind, prune_project_embeddings, query_memory, rebuild_chunks,
    rebuild_chunks_for_automatic_creation, reembed_project_chunks,
};
use mem_service::{
    fetch_project_commit, fetch_project_commits, fetch_project_memories, fetch_project_overview,
    parse_status_filter, preview_project_commit_sync, sync_project_commits,
};
use regex::Regex;
use serde::Deserialize;
use serde::{Deserialize as SerdeDeserialize, Serialize};
use sha2::{Digest, Sha256};
use socket2::{Domain, Protocol, Socket, Type};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use tokio::{
    net::{TcpListener, UdpSocket, UnixListener},
    sync::{broadcast, oneshot},
    task::JoinHandle,
    time::Duration,
};
use tokio_tungstenite::{connect_async, tungstenite::client::IntoClientRequest};
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

const QUERY_ACTIVITY_GRAPH_CONNECTION_LIMIT: usize = 5;

#[derive(Clone)]
struct AppState {
    role: ServiceRole,
    instance_id: String,
    pool: Option<PgPool>,
    api_token: String,
    config: AppConfig,
    web_root: Option<PathBuf>,
    http_client: reqwest::Client,
    embedders: Arc<tokio::sync::RwLock<EmbeddingRegistry>>,
    automated_embedding_creation_enabled: Arc<AtomicBool>,
    events: broadcast::Sender<ServiceEvent>,
    recent_activity: Arc<Mutex<VecDeque<ServiceEvent>>>,
    watchers: Arc<Mutex<HashMap<String, WatcherPresence>>>,
    cluster: ClusterRuntime,
    shutdown: Arc<Mutex<Option<oneshot::Sender<()>>>>,
}

#[derive(Clone, Debug)]
struct ServiceEvent {
    id: Uuid,
    project: String,
    memory_id: Option<Uuid>,
    kind: ActivityKind,
    summary: String,
    details: Option<ActivityDetails>,
    recorded_at: chrono::DateTime<chrono::Utc>,
    actor_id: Option<String>,
    actor_name: Option<String>,
    source: Option<String>,
    operation_id: Option<String>,
    duration_ms: Option<u64>,
    provider: Option<String>,
    model: Option<String>,
    token_usage: Option<TokenUsage>,
    include_activity: bool,
}

#[derive(Clone, Debug)]
enum ServiceRole {
    Primary,
    Relay,
}

#[derive(Clone, Debug)]
struct ClusterRuntime {
    peers: Arc<Mutex<HashMap<String, ClusterPeer>>>,
}

#[derive(Clone, Debug, Serialize, SerdeDeserialize)]
#[serde(rename_all = "snake_case")]
enum DiscoveryKind {
    Discover,
    Announce,
}

#[derive(Clone, Debug, Serialize, SerdeDeserialize)]
struct DiscoveryPacket {
    kind: DiscoveryKind,
    service_id: String,
    advertise_addr: String,
    version: String,
    priority: i32,
    sent_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Clone, Debug)]
struct ClusterPeer {
    service_id: String,
    advertise_addr: String,
    version: String,
    priority: i32,
    last_seen: chrono::DateTime<chrono::Utc>,
}

const WATCHER_STALE_AFTER_SECONDS: u64 = 90;
const WATCHER_RESTART_BACKOFF_SECONDS: u64 = 120;
const WATCHER_EXPIRY_AFTER_SECONDS: u64 = 600;
const WATCHER_MAX_RESTART_ATTEMPTS: u32 = 3;

pub async fn run_service(config_path: Option<PathBuf>) -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer().json())
        .init();

    let mut config_fingerprint = config_path_fingerprint(config_path.as_deref())
        .await
        .context("inspect config file")?;

    loop {
        let config = AppConfig::load_from_path(config_path.clone()).context("load config")?;
        let addr: SocketAddr = config
            .service
            .bind_addr
            .parse()
            .context("parse bind_addr")?;
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let shutdown_handle = Arc::new(Mutex::new(Some(shutdown_tx)));
        let state = build_state(config.clone(), shutdown_handle.clone()).await?;
        let app = build_http_app(state.clone());
        let listener = bind_http_listener_with_handoff(&config, addr)
            .await
            .context("bind http listener")?;
        let mut http_server = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
        });
        let mut proto_tasks = Vec::new();
        if state.is_primary() {
            let proto_servers = start_proto_servers(state.clone()).await?;
            proto_tasks.push(tokio::spawn(run_proto_unix(
                proto_servers.unix_listener,
                state.clone(),
            )));
            proto_tasks.push(tokio::spawn(run_proto_tcp(
                proto_servers.tcp_listener,
                state.clone(),
            )));
        }
        let mut cluster_tasks = start_cluster_tasks(state.clone()).await?;

        let version = config.profile.display_version(env!("CARGO_PKG_VERSION"));
        eprintln!(
            "memory-layer v{version} listening on {addr} (profile={profile}, role={role}, service_id={service_id}, cluster={cluster})",
            profile = config.profile,
            role = state.role_name(),
            service_id = config.cluster.service_id,
            cluster = if config.cluster.enabled {
                "enabled"
            } else {
                "disabled"
            },
        );
        if let Some(path) = config.resolved_config_path.as_deref() {
            eprintln!("  config: {}", path.display());
        }
        if let Some(path) = config.resolved_dev_overlay_path.as_deref() {
            eprintln!("  dev overlay: {}", path.display());
        }
        eprintln!(
            "  database: {}",
            if state.pool.is_some() {
                "connected"
            } else {
                "unavailable"
            }
        );
        eprintln!("  capnp unix: {}", config.service.capnp_unix_socket);
        eprintln!("  capnp tcp: {}", config.service.capnp_tcp_addr);

        tracing::info!(
            %addr,
            role = %state.role_name(),
            unix_socket = %config.service.capnp_unix_socket,
            tcp_addr = %config.service.capnp_tcp_addr,
            "memory-layer listening"
        );

        if let Some(path) = config_path.as_deref() {
            tokio::select! {
                result = &mut http_server => {
                    result.context("join mem-service task")??;
                    break;
                }
                result = tokio::signal::ctrl_c() => {
                    result.context("listen for ctrl-c")?;
                    request_runtime_shutdown(&shutdown_handle);
                    http_server.await.context("join mem-service task")??;
                    abort_tasks(&mut proto_tasks);
                    abort_tasks(&mut cluster_tasks);
                    break;
                }
                result = wait_for_config_change(path, config_fingerprint) => {
                    config_fingerprint = result.context("watch config file")?;
                    tracing::info!(path = %path.display(), "config changed; restarting backend");
                    request_runtime_shutdown(&shutdown_handle);
                    http_server.await.context("join mem-service task")??;
                    abort_tasks(&mut proto_tasks);
                    abort_tasks(&mut cluster_tasks);
                }
            }
        } else {
            tokio::select! {
                result = &mut http_server => {
                    result.context("join mem-service task")??;
                    break;
                }
                result = tokio::signal::ctrl_c() => {
                    result.context("listen for ctrl-c")?;
                    request_runtime_shutdown(&shutdown_handle);
                    http_server.await.context("join mem-service task")??;
                    abort_tasks(&mut proto_tasks);
                    abort_tasks(&mut cluster_tasks);
                    break;
                }
            }
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::args().any(|arg| arg == "--version" || arg == "-V") {
        println!(
            "memory service {}",
            mem_api::Profile::detect().display_version(env!("CARGO_PKG_VERSION"))
        );
        return Ok(());
    }

    run_service(std::env::args().nth(1).map(PathBuf::from)).await
}

async fn build_state(
    config: AppConfig,
    shutdown: Arc<Mutex<Option<oneshot::Sender<()>>>>,
) -> Result<AppState> {
    let http_client = reqwest::Client::builder()
        .timeout(config.service.request_timeout)
        .build()
        .context("build service http client")?;
    let pool_attempt = PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database.url)
        .await;
    let (events, _) = broadcast::channel(128);
    let (role, pool) = match pool_attempt {
        Ok(pool) => {
            sqlx::migrate!("../../migrations")
                .run(&pool)
                .await
                .context(
                    "run migrations (pgvector extension 'vector' must be installed in PostgreSQL)",
                )?;
            (ServiceRole::Primary, Some(pool))
        }
        Err(error) if config.cluster.enabled => {
            tracing::warn!(
                error = %error,
                "postgres unavailable; starting in relay mode"
            );
            (ServiceRole::Relay, None)
        }
        Err(error) => return Err(error).context("connect postgres"),
    };

    let embedders = Arc::new(tokio::sync::RwLock::new(EmbeddingRegistry::from_config(
        &config.embeddings,
    )));
    let automated_embedding_creation_enabled =
        Arc::new(AtomicBool::new(config.embeddings.create_enabled));

    Ok(AppState {
        role,
        instance_id: Uuid::new_v4().to_string(),
        pool,
        api_token: config.service.api_token.clone(),
        web_root: discover_web_root(&config),
        http_client,
        embedders,
        automated_embedding_creation_enabled,
        config,
        events,
        recent_activity: Arc::new(Mutex::new(VecDeque::with_capacity(20))),
        watchers: Arc::new(Mutex::new(HashMap::new())),
        cluster: ClusterRuntime {
            peers: Arc::new(Mutex::new(HashMap::new())),
        },
        shutdown,
    })
}

fn discover_web_root(config: &AppConfig) -> Option<PathBuf> {
    if let Some(root) = &config.service.web_root {
        let path = PathBuf::from(root);
        if path.join("index.html").is_file() {
            return Some(path);
        }
    }

    let candidates = vec![
        Some(PathBuf::from("web").join("dist")),
        mem_platform::current_exe_share_subdir("web"),
        mem_platform::preferred_user_state_dir().map(|dir| dir.join("web")),
        std::env::var("HOME")
            .ok()
            .map(|home| PathBuf::from(home).join(".local/share/memory-layer/web")),
        Some(PathBuf::from("/usr/share/memory-layer/web")),
    ];

    candidates
        .into_iter()
        .flatten()
        .find(|candidate| candidate.join("index.html").is_file())
}

impl AppState {
    fn is_primary(&self) -> bool {
        matches!(self.role, ServiceRole::Primary)
    }

    fn role_name(&self) -> &'static str {
        match self.role {
            ServiceRole::Primary => "primary",
            ServiceRole::Relay => "relay",
        }
    }

    fn pool(&self) -> Result<&PgPool, ApiError> {
        self.pool
            .as_ref()
            .ok_or_else(|| ApiError::service_unavailable("relay has no local database connection"))
    }
}

fn abort_tasks(tasks: &mut Vec<JoinHandle<Result<()>>>) {
    for task in tasks.drain(..) {
        task.abort();
    }
}

fn build_http_app(state: AppState) -> Router {
    let web_assets = state.web_root.clone();
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/ws", get(websocket))
        .route("/v1/admin/shutdown", post(admin_shutdown))
        .route("/v1/query", post(query))
        .route("/v1/checkpoint/activity", post(checkpoint_activity))
        .route("/v1/plan/activity", post(plan_activity))
        .route("/v1/scan/activity", post(scan_activity))
        .route("/v1/graph/activity", post(graph_activity))
        .route("/v1/commits/sync", post(sync_commits))
        .route("/v1/capture/task", post(capture_task))
        .route("/v1/curate", post(curate_memory))
        .route("/v1/reindex", post(reindex))
        .route("/v1/reembed", post(reembed))
        .route("/v1/prune-embeddings", post(prune_embeddings))
        .route("/v1/embeddings/backends", get(list_embedding_backends))
        .route("/v1/embeddings/activate", post(activate_embedding_backend))
        .route(
            "/v1/embeddings/deactivate",
            post(deactivate_embedding_backend),
        )
        .route(
            "/v1/embeddings/create-enabled",
            post(set_embedding_creation_enabled),
        )
        .route("/v1/memory/{id}", get(get_memory))
        .route("/v1/memory/{id}/history", get(get_memory_history))
        .route("/v1/memory", delete(delete_memory))
        .route("/v1/prune-history", post(prune_history))
        .route("/v1/stats", get(stats))
        .route("/v1/projects/{slug}/commits", get(project_commits))
        .route(
            "/v1/projects/{slug}/commits/{hash}",
            get(project_commit_detail),
        )
        .route(
            "/v1/projects/{slug}/bundle/export/preview",
            post(project_bundle_export_preview),
        )
        .route(
            "/v1/projects/{slug}/bundle/export",
            post(project_bundle_export),
        )
        .route(
            "/v1/projects/{slug}/bundle/import/preview",
            post(project_bundle_import_preview),
        )
        .route(
            "/v1/projects/{slug}/bundle/import",
            post(project_bundle_import),
        )
        .route(
            "/v1/projects/{slug}/replacement-proposals",
            get(project_replacement_proposals),
        )
        .route(
            "/v1/projects/{slug}/replacement-proposals/{proposal_id}/approve",
            post(project_replacement_proposal_approve),
        )
        .route(
            "/v1/projects/{slug}/replacement-proposals/{proposal_id}/reject",
            post(project_replacement_proposal_reject),
        )
        .route(
            "/v1/projects/{slug}/replacement-policy",
            get(project_replacement_policy)
                .put(project_replacement_policy_update)
                .post(project_replacement_policy_update),
        )
        .route("/v1/projects/{slug}/memories", get(project_memories))
        .route("/v1/projects/{slug}/overview", get(project_overview))
        .route("/v1/projects/{slug}/resume", post(project_resume))
        .route("/v1/projects/{slug}/activities", get(project_activities))
        .route("/v1/projects/{slug}/up-to-speed", post(project_up_to_speed))
        .route("/v1/watchers/heartbeat", post(watcher_heartbeat))
        .route("/v1/watchers/unregister", post(watcher_unregister))
        .route("/v1/watchers/restart-local", post(watcher_restart_local))
        .route("/v1/archive", post(archive))
        .route("/v1/agents", get(agents_snapshot))
        .with_state(state)
        .layer(TraceLayer::new_for_http());

    if let Some(root) = web_assets {
        let index = root.join("index.html");
        app.fallback_service(ServeDir::new(root).not_found_service(ServeFile::new(index)))
    } else {
        app.fallback(any(web_unavailable))
    }
}

fn request_runtime_shutdown(shutdown: &Arc<Mutex<Option<oneshot::Sender<()>>>>) {
    if let Some(sender) = shutdown.lock().expect("shutdown mutex poisoned").take() {
        let _ = sender.send(());
    }
}

async fn bind_http_listener_with_handoff(
    config: &AppConfig,
    addr: SocketAddr,
) -> Result<TcpListener> {
    match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => Ok(listener),
        Err(error) if error.kind() == ErrorKind::AddrInUse => {
            if !request_existing_instance_shutdown(config).await? {
                return Err(error).context("address already in use and handoff was refused");
            }
            wait_for_listener_release(addr).await?;
            bind_tcp_listener_with_addr_in_use_wait(addr, "http listener after handoff").await
        }
        Err(error) => Err(error).context("bind http listener"),
    }
}

async fn request_existing_instance_shutdown(config: &AppConfig) -> Result<bool> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .context("build handoff client")?;
    let response = match client
        .post(format!(
            "http://{}/v1/admin/shutdown",
            config.service.bind_addr
        ))
        .header("x-api-token", &config.service.api_token)
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(error = %error, "failed to contact existing backend for handoff");
            return Ok(false);
        }
    };
    Ok(response.status().is_success())
}

async fn wait_for_listener_release(addr: SocketAddr) -> Result<()> {
    for _ in 0..20 {
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => {
                drop(listener);
                return Ok(());
            }
            Err(error) if error.kind() == ErrorKind::AddrInUse => {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            Err(error) => return Err(error).context("wait for listener release"),
        }
    }
    anyhow::bail!("timed out waiting for existing backend to release {addr}");
}

async fn bind_tcp_listener_with_addr_in_use_wait(
    addr: SocketAddr,
    context_label: &'static str,
) -> Result<TcpListener> {
    match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => Ok(listener),
        Err(error) if error.kind() == ErrorKind::AddrInUse => {
            wait_for_listener_release(addr).await?;
            tokio::net::TcpListener::bind(addr)
                .await
                .with_context(|| format!("bind {context_label} after waiting for release"))
        }
        Err(error) => Err(error).with_context(|| format!("bind {context_label}")),
    }
}

async fn start_cluster_tasks(state: AppState) -> Result<Vec<JoinHandle<Result<()>>>> {
    let mut tasks = Vec::new();
    if state.is_primary() {
        tasks.push(tokio::spawn(run_watcher_watchdog(state.clone())));
    }
    if !state.config.cluster.enabled {
        return Ok(tasks);
    }

    let socket = Arc::new(bind_cluster_socket(
        &state.config.cluster.discovery_multicast_addr,
    )?);
    tasks.push(tokio::spawn(run_cluster_listener(
        socket.clone(),
        state.clone(),
    )));
    if state.is_primary() {
        tasks.push(tokio::spawn(run_cluster_announcer(socket, state)));
    } else {
        tasks.push(tokio::spawn(run_cluster_discoverer(socket, state)));
    }
    Ok(tasks)
}

fn bind_cluster_socket(multicast_addr: &str) -> Result<UdpSocket> {
    let addr: SocketAddr = multicast_addr
        .parse()
        .context("parse cluster multicast addr")?;
    let ip = match addr.ip() {
        std::net::IpAddr::V4(ip) => ip,
        std::net::IpAddr::V6(_) => {
            anyhow::bail!("cluster.discovery_multicast_addr must be an IPv4 multicast address")
        }
    };

    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
        .context("create discovery socket")?;
    socket
        .set_reuse_address(true)
        .context("set discovery SO_REUSEADDR")?;
    socket
        .bind(&SocketAddr::from(([0, 0, 0, 0], addr.port())).into())
        .context("bind discovery socket")?;
    socket
        .join_multicast_v4(&ip, &std::net::Ipv4Addr::UNSPECIFIED)
        .context("join discovery multicast group")?;
    socket
        .set_multicast_loop_v4(true)
        .context("enable multicast loopback")?;
    socket
        .set_nonblocking(true)
        .context("set discovery socket nonblocking")?;
    UdpSocket::from_std(socket.into()).context("convert discovery socket")
}

async fn run_cluster_listener(socket: Arc<UdpSocket>, state: AppState) -> Result<()> {
    let mut buffer = vec![0_u8; 4096];
    loop {
        let (len, from) = socket.recv_from(&mut buffer).await?;
        let Ok(packet) = serde_json::from_slice::<DiscoveryPacket>(&buffer[..len]) else {
            continue;
        };
        if packet.service_id == state.config.cluster.service_id {
            continue;
        }

        match packet.kind {
            DiscoveryKind::Discover if state.is_primary() => {
                let announce = build_discovery_packet(&state, DiscoveryKind::Announce);
                let payload = serde_json::to_vec(&announce)?;
                socket.send_to(&payload, from).await?;
            }
            DiscoveryKind::Announce => update_cluster_peer(&state.cluster, packet),
            DiscoveryKind::Discover => {}
        }
    }
}

async fn run_cluster_announcer(socket: Arc<UdpSocket>, state: AppState) -> Result<()> {
    let mut interval = tokio::time::interval(state.config.cluster.announce_interval);
    loop {
        interval.tick().await;
        let packet = build_discovery_packet(&state, DiscoveryKind::Announce);
        let payload = serde_json::to_vec(&packet)?;
        socket
            .send_to(&payload, &state.config.cluster.discovery_multicast_addr)
            .await?;
    }
}

async fn run_cluster_discoverer(socket: Arc<UdpSocket>, state: AppState) -> Result<()> {
    let mut interval = tokio::time::interval(state.config.cluster.announce_interval);
    loop {
        interval.tick().await;
        let packet = build_discovery_packet(&state, DiscoveryKind::Discover);
        let payload = serde_json::to_vec(&packet)?;
        socket
            .send_to(&payload, &state.config.cluster.discovery_multicast_addr)
            .await?;
        prune_cluster_peers(&state.cluster, state.config.cluster.peer_ttl);
    }
}

fn build_discovery_packet(state: &AppState, kind: DiscoveryKind) -> DiscoveryPacket {
    DiscoveryPacket {
        kind,
        service_id: state.config.cluster.service_id.clone(),
        advertise_addr: advertised_http_addr(&state.config),
        version: state
            .config
            .profile
            .display_version(env!("CARGO_PKG_VERSION")),
        priority: state.config.cluster.priority,
        sent_at: chrono::Utc::now(),
    }
}

fn advertised_http_addr(config: &AppConfig) -> String {
    config
        .cluster
        .advertise_addr
        .clone()
        .unwrap_or_else(|| config.service.bind_addr.clone())
}

fn update_cluster_peer(cluster: &ClusterRuntime, packet: DiscoveryPacket) {
    let mut peers = cluster.peers.lock().expect("cluster peer mutex poisoned");
    peers.insert(
        packet.service_id.clone(),
        ClusterPeer {
            service_id: packet.service_id,
            advertise_addr: packet.advertise_addr,
            version: packet.version,
            priority: packet.priority,
            last_seen: chrono::Utc::now(),
        },
    );
}

fn prune_cluster_peers(cluster: &ClusterRuntime, ttl: Duration) {
    let ttl = chrono::Duration::from_std(ttl).expect("valid cluster peer ttl");
    let now = chrono::Utc::now();
    let mut peers = cluster.peers.lock().expect("cluster peer mutex poisoned");
    peers.retain(|_, peer| now - peer.last_seen <= ttl);
}

fn selected_primary_peer(state: &AppState) -> Option<ClusterPeer> {
    prune_cluster_peers(&state.cluster, state.config.cluster.peer_ttl);
    let peers = state
        .cluster
        .peers
        .lock()
        .expect("cluster peer mutex poisoned");
    let mut peers = peers.values().cloned().collect::<Vec<_>>();
    peers.sort_by(|left, right| {
        right
            .priority
            .cmp(&left.priority)
            .then_with(|| right.last_seen.cmp(&left.last_seen))
            .then_with(|| left.service_id.cmp(&right.service_id))
    });
    peers.into_iter().next()
}

fn cluster_peer_by_service_id(state: &AppState, service_id: &str) -> Option<ClusterPeer> {
    prune_cluster_peers(&state.cluster, state.config.cluster.peer_ttl);
    let peers = state
        .cluster
        .peers
        .lock()
        .expect("cluster peer mutex poisoned");
    peers.get(service_id).cloned()
}

struct ProtoServers {
    unix_listener: UnixListener,
    tcp_listener: TcpListener,
}

async fn start_proto_servers(state: AppState) -> Result<ProtoServers> {
    let unix_path = PathBuf::from(&state.config.service.capnp_unix_socket);
    if let Some(parent) = unix_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("create {}", parent.display()))?;
    }
    if unix_path.exists() {
        tokio::fs::remove_file(&unix_path)
            .await
            .with_context(|| format!("remove stale socket {}", unix_path.display()))?;
    }

    let unix_listener = UnixListener::bind(&unix_path)
        .with_context(|| format!("bind unix socket {}", unix_path.display()))?;
    let tcp_addr: SocketAddr = state
        .config
        .service
        .capnp_tcp_addr
        .parse()
        .context("parse capnp tcp addr")?;
    let tcp_listener = bind_tcp_listener_with_addr_in_use_wait(tcp_addr, "capnp tcp addr")
        .await
        .context("bind capnp tcp addr")?;

    Ok(ProtoServers {
        unix_listener,
        tcp_listener,
    })
}

async fn run_proto_unix(listener: UnixListener, state: AppState) -> Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(handle_proto_connection(stream, state.clone()));
    }
}

async fn run_proto_tcp(listener: TcpListener, state: AppState) -> Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(handle_proto_connection(stream, state.clone()));
    }
}

#[derive(Default)]
struct ConnectionSubscriptions {
    project: Option<String>,
    memory_id: Option<Uuid>,
}

async fn websocket(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| async move {
        if state.is_primary() {
            handle_websocket_connection(socket, state).await;
        } else if let Err(error) = bridge_relay_websocket(socket, state).await {
            tracing::warn!(error = %error, "relay websocket bridge failed");
        }
    })
}

async fn handle_websocket_connection(mut socket: WebSocket, state: AppState) {
    let mut subscriptions = ConnectionSubscriptions::default();
    let mut events = state.events.subscribe();

    loop {
        tokio::select! {
            incoming = socket.recv() => {
                let Some(result) = incoming else {
                    break;
                };
                match result {
                    Ok(Message::Text(text)) => {
                        let request = match serde_json::from_str::<StreamRequest>(&text) {
                            Ok(request) => request,
                            Err(error) => {
                                if send_ws_response(
                                    &mut socket,
                                    StreamResponse::Error {
                                        message: format!("parse stream request: {error}"),
                                    },
                                )
                                .await
                                .is_err()
                                {
                                    break;
                                }
                                continue;
                            }
                        };
                        match process_stream_request(&state, &mut subscriptions, request).await {
                            Ok(responses) => {
                                for response in responses {
                                    if send_ws_response(&mut socket, response).await.is_err() {
                                        return;
                                    }
                                }
                            }
                            Err(error) => {
                                if send_ws_response(
                                    &mut socket,
                                    StreamResponse::Error {
                                        message: error.to_string(),
                                    },
                                )
                                .await
                                .is_err()
                                {
                                    break;
                                }
                            }
                        }
                    }
                    Ok(Message::Ping(payload)) => {
                        if socket.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Ok(Message::Close(_)) => break,
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
            event = events.recv() => {
                let Ok(event) = event else {
                    continue;
                };
                match render_subscription_updates(&state, &subscriptions, &event).await {
                    Ok(responses) => {
                        for response in responses {
                            if send_ws_response(&mut socket, response).await.is_err() {
                                return;
                            }
                        }
                    }
                    Err(error) => {
                        if send_ws_response(
                            &mut socket,
                            StreamResponse::Error {
                                message: error.to_string(),
                            },
                        )
                        .await
                        .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        }
    }
}

async fn bridge_relay_websocket(socket: WebSocket, state: AppState) -> Result<()> {
    let upstream = selected_primary_peer(&state)
        .ok_or_else(|| anyhow::anyhow!("no primary available for relay websocket"))?;
    let mut request = format!("ws://{}/ws", upstream.advertise_addr).into_client_request()?;
    request.headers_mut().insert(
        "x-api-token",
        state
            .api_token
            .parse()
            .context("parse relay api token header")?,
    );
    let (upstream_stream, _) = connect_async(request)
        .await
        .context("connect upstream websocket")?;

    let (mut client_sender, mut client_receiver) = socket.split();
    let (mut upstream_sender, mut upstream_receiver) = upstream_stream.split();

    let client_to_upstream = async {
        while let Some(message) = client_receiver.next().await {
            let message = message?;
            let mapped = match message {
                Message::Text(text) => {
                    tokio_tungstenite::tungstenite::Message::Text(text.to_string())
                }
                Message::Binary(binary) => {
                    tokio_tungstenite::tungstenite::Message::Binary(binary.to_vec())
                }
                Message::Ping(payload) => {
                    tokio_tungstenite::tungstenite::Message::Ping(payload.to_vec())
                }
                Message::Pong(payload) => {
                    tokio_tungstenite::tungstenite::Message::Pong(payload.to_vec())
                }
                Message::Close(frame) => {
                    let close =
                        frame.map(
                            |frame| tokio_tungstenite::tungstenite::protocol::CloseFrame {
                                code: frame.code.into(),
                                reason: frame.reason.to_string().into(),
                            },
                        );
                    tokio_tungstenite::tungstenite::Message::Close(close)
                }
            };
            upstream_sender.send(mapped).await?;
        }
        Result::<(), anyhow::Error>::Ok(())
    };

    let upstream_to_client = async {
        while let Some(message) = upstream_receiver.next().await {
            let message = message?;
            let mapped = match message {
                tokio_tungstenite::tungstenite::Message::Text(text) => Message::Text(text.into()),
                tokio_tungstenite::tungstenite::Message::Binary(binary) => {
                    Message::Binary(binary.into())
                }
                tokio_tungstenite::tungstenite::Message::Ping(payload) => {
                    Message::Ping(payload.into())
                }
                tokio_tungstenite::tungstenite::Message::Pong(payload) => {
                    Message::Pong(payload.into())
                }
                tokio_tungstenite::tungstenite::Message::Close(frame) => {
                    let close = frame.map(|frame| axum::extract::ws::CloseFrame {
                        code: frame.code.into(),
                        reason: frame.reason.to_string().into(),
                    });
                    Message::Close(close)
                }
                tokio_tungstenite::tungstenite::Message::Frame(_) => continue,
            };
            client_sender.send(mapped).await?;
        }
        Result::<(), anyhow::Error>::Ok(())
    };

    tokio::select! {
        result = client_to_upstream => result?,
        result = upstream_to_client => result?,
    }

    Ok(())
}

async fn send_ws_response(socket: &mut WebSocket, response: StreamResponse) -> Result<()> {
    socket
        .send(Message::Text(serde_json::to_string(&response)?.into()))
        .await
        .context("send websocket response")?;
    Ok(())
}

async fn handle_proto_connection<S>(stream: S, state: AppState) -> Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (mut reader, mut writer) = tokio::io::split(stream);
    let mut subscriptions = ConnectionSubscriptions::default();
    let mut events = state.events.subscribe();

    loop {
        tokio::select! {
            incoming = read_capnp_text_frame(&mut reader) => {
                let Some(text) = incoming? else {
                    break;
                };
                let request: StreamRequest = serde_json::from_str(&text)
                    .map_err(|error| anyhow::anyhow!("parse stream request: {error}"))?;
                for response in process_stream_request(&state, &mut subscriptions, request).await? {
                    let text = serde_json::to_string(&response)?;
                    write_capnp_text_frame(&mut writer, &text).await?;
                }
            }
            event = events.recv() => {
                let Ok(event) = event else {
                    continue;
                };
                for response in render_subscription_updates(&state, &subscriptions, &event).await? {
                    let text = serde_json::to_string(&response)?;
                    write_capnp_text_frame(&mut writer, &text).await?;
                }
            }
        }
    }

    Ok(())
}

async fn process_stream_request(
    state: &AppState,
    subscriptions: &mut ConnectionSubscriptions,
    request: StreamRequest,
) -> Result<Vec<StreamResponse>> {
    let pool = state
        .pool
        .as_ref()
        .context("primary state missing database pool")?;
    let mut responses = Vec::new();
    match request {
        StreamRequest::Health => responses.push(StreamResponse::Health {
            value: health_payload(state).await?,
        }),
        StreamRequest::ProjectOverview { project } => {
            responses.push(StreamResponse::ProjectOverview {
                value: fetch_project_overview_with_watchers(state, &project).await?,
            });
        }
        StreamRequest::ProjectMemories { project } => {
            responses.push(StreamResponse::ProjectMemories {
                value: fetch_project_memories(pool, &project, None, 500, 0).await?,
            });
        }
        StreamRequest::MemoryDetail { memory_id } => {
            responses.push(StreamResponse::MemoryDetail {
                value: fetch_memory_entry(pool, memory_id).await?,
            });
        }
        StreamRequest::SubscribeProject { project } => {
            subscriptions.project = Some(project.clone());
            let overview = fetch_project_overview_with_watchers(state, &project).await?;
            let memories = fetch_project_memories(pool, &project, None, 500, 0).await?;
            responses.push(StreamResponse::ProjectSnapshot { overview, memories });
            responses.extend(recent_activity_responses(&state.recent_activity, &project).await);
        }
        StreamRequest::SubscribeMemory { memory_id } => {
            subscriptions.memory_id = Some(memory_id);
            let detail = fetch_memory_entry(pool, memory_id).await?;
            responses.push(StreamResponse::MemorySnapshot { detail });
        }
        StreamRequest::UnsubscribeMemory => {
            subscriptions.memory_id = None;
            responses.push(StreamResponse::Ack {
                message: "memory subscription cleared".to_string(),
            });
        }
        StreamRequest::Ping => responses.push(StreamResponse::Pong),
    }
    Ok(responses)
}

async fn render_subscription_updates(
    state: &AppState,
    subscriptions: &ConnectionSubscriptions,
    event: &ServiceEvent,
) -> Result<Vec<StreamResponse>> {
    let pool = state
        .pool
        .as_ref()
        .context("primary state missing database pool")?;
    let mut responses = Vec::new();
    if let Some(project) = &subscriptions.project
        && project == &event.project
    {
        if event.include_activity {
            responses.push(stream_activity_response(event.clone()));
        }
        let overview = fetch_project_overview_with_watchers(state, project).await?;
        let memories = fetch_project_memories(pool, project, None, 500, 0).await?;
        responses.push(StreamResponse::ProjectChanged { overview, memories });
    }

    if let Some(memory_id) = subscriptions.memory_id
        && event.memory_id == Some(memory_id)
    {
        let detail = fetch_memory_entry(pool, memory_id).await?;
        responses.push(StreamResponse::MemoryChanged { detail });
    }

    Ok(responses)
}

async fn recent_activity_responses(
    recent_activity: &Mutex<VecDeque<ServiceEvent>>,
    project: &str,
) -> Vec<StreamResponse> {
    let history = recent_activity
        .lock()
        .expect("activity history mutex poisoned");
    history
        .iter()
        .filter(|event| event.project == project)
        .cloned()
        .map(stream_activity_response)
        .collect()
}

async fn health_payload(state: &AppState) -> Result<serde_json::Value> {
    if state.is_primary() {
        let pool = state
            .pool
            .as_ref()
            .context("primary state missing database pool")?;
        sqlx::query("SELECT 1").execute(pool).await?;
        Ok(serde_json::json!({
            "status": "ok",
            "role": "primary",
            "database": "up",
            "instance_id": state.instance_id,
            "service_id": state.config.cluster.service_id,
            "version": state.config.profile.display_version(env!("CARGO_PKG_VERSION"))
        }))
    } else {
        let upstream = relay_upstream_health(state).await?;
        Ok(serde_json::json!({
            "status": if upstream.is_some() { "ok" } else { "degraded" },
            "role": "relay",
            "database": "down",
            "instance_id": state.instance_id,
            "service_id": state.config.cluster.service_id,
            "version": state.config.profile.display_version(env!("CARGO_PKG_VERSION")),
            "upstream": upstream
        }))
    }
}

async fn relay_upstream_health(state: &AppState) -> Result<Option<serde_json::Value>> {
    let Some(peer) = selected_primary_peer(state) else {
        return Ok(None);
    };
    let health = state
        .http_client
        .get(format!("http://{}/healthz", peer.advertise_addr))
        .send()
        .await;
    let status = match health {
        Ok(response) => {
            let code = response.status();
            let body = response
                .json::<serde_json::Value>()
                .await
                .unwrap_or_else(|_| serde_json::json!({}));
            serde_json::json!({
                "service_id": peer.service_id,
                "address": peer.advertise_addr,
                "version": peer.version,
                "status": code.as_u16(),
                "health": body
            })
        }
        Err(error) => serde_json::json!({
            "service_id": peer.service_id,
            "address": peer.advertise_addr,
            "version": peer.version,
            "status": "unreachable",
            "error": error.to_string()
        }),
    };
    Ok(Some(status))
}

fn relay_target(state: &AppState) -> Result<ClusterPeer, ApiError> {
    selected_primary_peer(state).ok_or_else(|| {
        ApiError::service_unavailable("no primary memory service available on the local network")
    })
}

async fn proxy_get_json<T: serde::de::DeserializeOwned>(
    state: &AppState,
    path: &str,
) -> Result<T, ApiError> {
    let peer = relay_target(state)?;
    let response = state
        .http_client
        .get(format!("http://{}{}", peer.advertise_addr, path))
        .send()
        .await
        .map_err(|error| ApiError::io(error.into()))?;
    parse_proxy_json(response).await
}

async fn proxy_post_json<Req: serde::Serialize, Resp: serde::de::DeserializeOwned>(
    state: &AppState,
    path: &str,
    request: &Req,
    include_token: bool,
) -> Result<Resp, ApiError> {
    let peer = relay_target(state)?;
    let mut builder = state
        .http_client
        .post(format!("http://{}{}", peer.advertise_addr, path));
    if include_token {
        builder = builder.header("x-api-token", &state.api_token);
    }
    let response = builder
        .json(request)
        .send()
        .await
        .map_err(|error| ApiError::io(error.into()))?;
    parse_proxy_json(response).await
}

async fn proxy_delete_json<Req: serde::Serialize, Resp: serde::de::DeserializeOwned>(
    state: &AppState,
    path: &str,
    request: &Req,
) -> Result<Resp, ApiError> {
    let peer = relay_target(state)?;
    let response = state
        .http_client
        .delete(format!("http://{}{}", peer.advertise_addr, path))
        .header("x-api-token", &state.api_token)
        .json(request)
        .send()
        .await
        .map_err(|error| ApiError::io(error.into()))?;
    parse_proxy_json(response).await
}

async fn parse_proxy_json<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, ApiError> {
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| ApiError::io(error.into()))?;
    if !status.is_success() {
        return Err(ApiError::status_message(
            StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
            if body.trim().is_empty() {
                format!("upstream request failed with {status}")
            } else {
                body
            },
        ));
    }
    serde_json::from_str(&body).map_err(|error| ApiError::io(error.into()))
}

async fn fetch_memory_entry(
    pool: &PgPool,
    id: Uuid,
) -> Result<Option<MemoryEntryResponse>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT p.slug, m.id, m.canonical_text, m.summary, m.memory_type, m.importance, m.confidence,
               m.status, m.created_at, m.updated_at,
               m.canonical_id, m.version_no, m.is_tombstone
        FROM memory_entries m
        JOIN projects p ON p.id = m.project_id
        WHERE m.id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    let tags = sqlx::query("SELECT tag FROM memory_tags WHERE memory_entry_id = $1 ORDER BY tag")
        .bind(id)
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|row| row.try_get::<String, _>("tag"))
        .collect::<Result<Vec<_>, _>>()?;

    let sources = sqlx::query(
        r#"
        SELECT id, task_id, file_path, git_commit, source_kind, excerpt
        FROM memory_sources
        WHERE memory_entry_id = $1
        ORDER BY created_at ASC
        "#,
    )
    .bind(id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        Ok(MemorySourceRecord {
            id: row.try_get("id")?,
            task_id: row.try_get("task_id")?,
            file_path: row.try_get("file_path")?,
            git_commit: row.try_get("git_commit")?,
            source_kind: parse_source_kind(&row.try_get::<String, _>("source_kind")?),
            excerpt: row.try_get("excerpt")?,
        })
    })
    .collect::<Result<Vec<_>, sqlx::Error>>()?;

    let related_memories = sqlx::query(
        r#"
        SELECT mr.relation_type, m.id, m.summary, m.memory_type, m.confidence
        FROM memory_relations mr
        JOIN memory_entries m ON m.id = mr.dst_memory_id
        WHERE mr.src_memory_id = $1
        ORDER BY m.updated_at DESC, m.id
        LIMIT 12
        "#,
    )
    .bind(id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        Ok(RelatedMemorySummary {
            memory_id: row.try_get("id")?,
            relation_type: parse_relation_type(&row.try_get::<String, _>("relation_type")?),
            summary: row.try_get("summary")?,
            memory_type: parse_memory_type(&row.try_get::<String, _>("memory_type")?),
            confidence: row.try_get("confidence")?,
        })
    })
    .collect::<Result<Vec<_>, sqlx::Error>>()?;

    let embedding_spaces = fetch_memory_embedding_spaces(pool, id).await?;

    Ok(Some(MemoryEntryResponse {
        id,
        project: row.try_get("slug")?,
        canonical_text: row.try_get("canonical_text")?,
        summary: row.try_get("summary")?,
        memory_type: parse_memory_type(&row.try_get::<String, _>("memory_type")?),
        importance: row.try_get("importance")?,
        confidence: row.try_get("confidence")?,
        status: match row.try_get::<String, _>("status")?.as_str() {
            "archived" => mem_api::MemoryStatus::Archived,
            _ => mem_api::MemoryStatus::Active,
        },
        tags,
        sources,
        related_memories,
        embedding_spaces,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        canonical_id: row.try_get("canonical_id")?,
        version_no: row.try_get("version_no")?,
        is_tombstone: row.try_get("is_tombstone")?,
    }))
}

async fn fetch_memory_embedding_spaces(
    pool: &PgPool,
    memory_id: Uuid,
) -> Result<Vec<mem_api::MemoryEmbeddingSpace>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT mce.embedding_provider,
               mce.embedding_model,
               mce.embedding_base_url,
               COUNT(*)::bigint         AS chunk_count,
               MAX(mce.embedding_updated_at) AS last_updated
        FROM memory_chunk_embeddings mce
        JOIN memory_chunks mc ON mc.id = mce.chunk_id
        WHERE mc.memory_entry_id = $1
        GROUP BY mce.embedding_provider, mce.embedding_model, mce.embedding_base_url
        ORDER BY last_updated DESC NULLS LAST,
                 mce.embedding_provider,
                 mce.embedding_model
        "#,
    )
    .bind(memory_id)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(mem_api::MemoryEmbeddingSpace {
                provider: row.try_get("embedding_provider")?,
                model: row.try_get("embedding_model")?,
                base_url: row.try_get("embedding_base_url")?,
                chunk_count: row.try_get("chunk_count")?,
                last_updated: row.try_get("last_updated")?,
            })
        })
        .collect()
}

const MEMORY_BUNDLE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug)]
struct LoadedBundle {
    manifest: ProjectMemoryBundleManifest,
    warnings: Vec<String>,
}

#[derive(Debug)]
struct ImportAssessment {
    new_count: usize,
    unchanged_count: usize,
    replacing_count: usize,
}

fn entry_key_for_memory(memory: &MemoryEntryResponse) -> String {
    memory.id.to_string()
}

fn entry_hash(entry: &ProjectMemoryBundleEntry) -> Result<String, ApiError> {
    let bytes = serde_json::to_vec(entry).map_err(|error| ApiError::io(error.into()))?;
    Ok(hex_sha256(&bytes))
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn render_bundle_summary(
    source_project: &str,
    entries: &[ProjectMemoryBundleEntry],
    options: &ProjectMemoryExportOptions,
    warning_count: usize,
) -> String {
    let mut type_counts: HashMap<String, usize> = HashMap::new();
    let mut tag_counts: HashMap<String, usize> = HashMap::new();
    for entry in entries {
        *type_counts
            .entry(entry.memory_type.to_string())
            .or_default() += 1;
        for tag in &entry.tags {
            *tag_counts.entry(tag.clone()).or_default() += 1;
        }
    }
    let mut top_types = type_counts.into_iter().collect::<Vec<_>>();
    top_types.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut top_tags = tag_counts.into_iter().collect::<Vec<_>>();
    top_tags.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let sample = entries
        .iter()
        .take(5)
        .map(|entry| format!("- {}", entry.summary))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "# Memory Bundle: {source_project}\n\n\
        - Memories: {}\n\
        - Include archived: {}\n\
        - Include tags: {}\n\
        - Include relations: {}\n\
        - Include source paths: {}\n\
        - Include git commits: {}\n\
        - Include source excerpts: {}\n\
        - Warnings: {}\n\n\
        ## Top memory types\n{}\n\n\
        ## Top tags\n{}\n\n\
        ## Sample memories\n{}\n",
        entries.len(),
        options.include_archived,
        options.include_tags,
        options.include_relations,
        options.include_source_file_paths,
        options.include_git_commits,
        options.include_source_excerpts,
        warning_count,
        top_types
            .iter()
            .take(5)
            .map(|(name, count)| format!("- {name}: {count}"))
            .collect::<Vec<_>>()
            .join("\n"),
        top_tags
            .iter()
            .take(8)
            .map(|(name, count)| format!("- {name}: {count}"))
            .collect::<Vec<_>>()
            .join("\n"),
        if sample.is_empty() {
            "- No memories selected.".to_string()
        } else {
            sample
        },
    )
}

fn detect_bundle_warnings(
    entries: &[ProjectMemoryBundleEntry],
    options: &ProjectMemoryExportOptions,
) -> Vec<String> {
    let email_re = Regex::new(r"[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}").expect("email regex");
    let token_re =
        Regex::new(r"(sk-[A-Za-z0-9_-]{10,}|ghp_[A-Za-z0-9]{20,}|AIza[0-9A-Za-z_-]{20,})")
            .expect("token regex");
    let path_re = Regex::new(r"(/home/|/Users/|[A-Z]:\\)").expect("path regex");
    let phone_re = Regex::new(r"\+?\d[\d \-]{7,}\d").expect("phone regex");
    let mut warnings = Vec::new();

    for entry in entries {
        if email_re.is_match(&entry.canonical_text)
            || token_re.is_match(&entry.canonical_text)
            || path_re.is_match(&entry.canonical_text)
            || phone_re.is_match(&entry.canonical_text)
        {
            warnings.push(format!(
                "Memory '{}' contains text that looks sensitive; review canonical text before sharing.",
                entry.summary
            ));
        }
        if options.include_source_excerpts {
            for source in &entry.sources {
                if let Some(excerpt) = &source.excerpt
                    && (email_re.is_match(excerpt)
                        || token_re.is_match(excerpt)
                        || path_re.is_match(excerpt)
                        || phone_re.is_match(excerpt))
                {
                    warnings.push(format!(
                        "Memory '{}' includes a source excerpt that looks sensitive.",
                        entry.summary
                    ));
                    break;
                }
            }
        }
    }

    warnings.sort();
    warnings.dedup();
    warnings
}

async fn load_project_bundle_entries(
    pool: &PgPool,
    slug: &str,
    options: &ProjectMemoryExportOptions,
) -> Result<Vec<MemoryEntryResponse>, ApiError> {
    let status_filter = if options.include_archived {
        None
    } else {
        Some("active")
    };
    let memories = fetch_project_memories(pool, slug, status_filter, 10_000, 0)
        .await
        .map_err(ApiError::sql)?;
    let mut entries = Vec::with_capacity(memories.items.len());
    for item in memories.items {
        if let Some(detail) = fetch_memory_entry(pool, item.id)
            .await
            .map_err(ApiError::sql)?
        {
            entries.push(detail);
        }
    }
    Ok(entries)
}

fn build_bundle_manifest(
    slug: &str,
    options: &ProjectMemoryExportOptions,
    memories: &[MemoryEntryResponse],
) -> Result<(ProjectMemoryBundleManifest, Vec<String>), ApiError> {
    let key_map = memories
        .iter()
        .map(|memory| (memory.id, entry_key_for_memory(memory)))
        .collect::<HashMap<_, _>>();
    let mut entries = Vec::with_capacity(memories.len());

    for memory in memories {
        let mut relations = Vec::new();
        if options.include_relations {
            for relation in &memory.related_memories {
                if let Some(target_entry_key) = key_map.get(&relation.memory_id) {
                    relations.push(ProjectMemoryBundleEntryRelation {
                        relation_type: relation.relation_type.clone(),
                        target_entry_key: target_entry_key.clone(),
                    });
                }
            }
        }

        let mut sources = Vec::new();
        if options.include_source_file_paths
            || options.include_git_commits
            || options.include_source_excerpts
        {
            for source in &memory.sources {
                sources.push(ProjectMemoryBundleSource {
                    source_kind: source.source_kind.clone(),
                    file_path: options
                        .include_source_file_paths
                        .then(|| source.file_path.clone())
                        .flatten(),
                    git_commit: options
                        .include_git_commits
                        .then(|| source.git_commit.clone())
                        .flatten(),
                    excerpt: options
                        .include_source_excerpts
                        .then(|| source.excerpt.clone())
                        .flatten(),
                });
            }
        }

        entries.push(ProjectMemoryBundleEntry {
            entry_key: entry_key_for_memory(memory),
            canonical_text: memory.canonical_text.clone(),
            summary: memory.summary.clone(),
            memory_type: memory.memory_type.clone(),
            importance: memory.importance,
            confidence: memory.confidence,
            tags: if options.include_tags {
                memory.tags.clone()
            } else {
                Vec::new()
            },
            relations,
            sources,
            created_at: memory.created_at,
            updated_at: memory.updated_at,
        });
    }

    let warnings = detect_bundle_warnings(&entries, options);
    let summary_markdown = render_bundle_summary(slug, &entries, options, warnings.len());
    let bundle_id = format!("{slug}-{}", chrono::Utc::now().format("%Y%m%d%H%M%S"));
    let mut manifest = ProjectMemoryBundleManifest {
        schema_version: MEMORY_BUNDLE_SCHEMA_VERSION,
        bundle_id,
        source_project: slug.to_string(),
        exported_at: chrono::Utc::now(),
        summary_markdown,
        bundle_hash: String::new(),
        options: options.clone(),
        entries,
    };
    let hash_input = serde_json::to_vec(&manifest).map_err(|error| ApiError::io(error.into()))?;
    manifest.bundle_hash = hex_sha256(&hash_input);
    Ok((manifest, warnings))
}

fn build_export_preview(
    manifest: &ProjectMemoryBundleManifest,
    warnings: Vec<String>,
) -> ProjectMemoryBundlePreview {
    ProjectMemoryBundlePreview {
        bundle_id: manifest.bundle_id.clone(),
        source_project: manifest.source_project.clone(),
        exported_at: manifest.exported_at,
        summary_markdown: manifest.summary_markdown.clone(),
        memory_count: manifest.entries.len(),
        relation_count: manifest
            .entries
            .iter()
            .map(|entry| entry.relations.len())
            .sum(),
        warning_count: warnings.len(),
        warnings,
        options: manifest.options.clone(),
    }
}

fn bundle_filename(slug: &str, bundle_id: &str) -> String {
    format!("{slug}-{bundle_id}.mlbundle.zip")
}

fn serialize_bundle_archive(manifest: &ProjectMemoryBundleManifest) -> Result<Vec<u8>, ApiError> {
    let cursor = std::io::Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("manifest.json", options)
        .map_err(|error| ApiError::io(error.into()))?;
    let manifest_json =
        serde_json::to_vec_pretty(manifest).map_err(|error| ApiError::io(error.into()))?;
    std::io::Write::write_all(&mut zip, &manifest_json)
        .map_err(|error| ApiError::io(error.into()))?;
    zip.start_file("SUMMARY.md", options)
        .map_err(|error| ApiError::io(error.into()))?;
    std::io::Write::write_all(&mut zip, manifest.summary_markdown.as_bytes())
        .map_err(|error| ApiError::io(error.into()))?;
    let cursor = zip.finish().map_err(|error| ApiError::io(error.into()))?;
    Ok(cursor.into_inner())
}

fn load_bundle_archive(bytes: &[u8]) -> Result<LoadedBundle, ApiError> {
    let cursor = std::io::Cursor::new(bytes);
    let mut zip = ZipArchive::new(cursor).map_err(|error| ApiError::io(error.into()))?;
    let mut manifest_json = String::new();
    zip.by_name("manifest.json")
        .map_err(|error| ApiError::io(error.into()))?
        .read_to_string(&mut manifest_json)
        .map_err(|error| ApiError::io(error.into()))?;
    let manifest: ProjectMemoryBundleManifest =
        serde_json::from_str(&manifest_json).map_err(|error| ApiError::io(error.into()))?;
    if manifest.schema_version != MEMORY_BUNDLE_SCHEMA_VERSION {
        return Err(ApiError::validation(ValidationError::new(
            "unsupported memory bundle schema version",
        )));
    }
    let mut hashable = manifest.clone();
    let bundle_hash = std::mem::take(&mut hashable.bundle_hash);
    let recalculated =
        hex_sha256(&serde_json::to_vec(&hashable).map_err(|error| ApiError::io(error.into()))?);
    if bundle_hash != recalculated {
        return Err(ApiError::validation(ValidationError::new(
            "memory bundle hash verification failed",
        )));
    }
    let warnings = detect_bundle_warnings(&manifest.entries, &manifest.options);
    Ok(LoadedBundle { manifest, warnings })
}

async fn preview_bundle_import(
    pool: &PgPool,
    target_project: &str,
    bundle: &ProjectMemoryBundleManifest,
    warnings: Vec<String>,
) -> Result<ProjectMemoryImportPreview, ApiError> {
    let target_project_id = sqlx::query("SELECT id FROM projects WHERE slug = $1")
        .bind(target_project)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::sql)?
        .map(|row| row.try_get::<Uuid, _>("id"))
        .transpose()
        .map_err(ApiError::sql)?;

    let mut assessment = ImportAssessment {
        new_count: 0,
        unchanged_count: 0,
        replacing_count: 0,
    };

    if let Some(project_id) = target_project_id {
        for entry in &bundle.entries {
            let existing = sqlx::query(
                r#"
                SELECT entry_hash
                FROM imported_memory_entries
                WHERE target_project_id = $1
                  AND bundle_id = $2
                  AND exported_entry_key = $3
                "#,
            )
            .bind(project_id)
            .bind(&bundle.bundle_id)
            .bind(&entry.entry_key)
            .fetch_optional(pool)
            .await
            .map_err(ApiError::sql)?;
            if let Some(row) = existing {
                let existing_hash: String = row.try_get("entry_hash").map_err(ApiError::sql)?;
                if existing_hash == entry_hash(entry)? {
                    assessment.unchanged_count += 1;
                } else {
                    assessment.replacing_count += 1;
                }
            } else {
                assessment.new_count += 1;
            }
        }
    } else {
        assessment.new_count = bundle.entries.len();
    }

    Ok(ProjectMemoryImportPreview {
        bundle_id: bundle.bundle_id.clone(),
        bundle_hash: bundle.bundle_hash.clone(),
        source_project: bundle.source_project.clone(),
        target_project: target_project.to_string(),
        exported_at: bundle.exported_at,
        summary_markdown: bundle.summary_markdown.clone(),
        memory_count: bundle.entries.len(),
        relation_count: bundle
            .entries
            .iter()
            .map(|entry| entry.relations.len())
            .sum(),
        new_count: assessment.new_count,
        unchanged_count: assessment.unchanged_count,
        replacing_count: assessment.replacing_count,
        warning_count: warnings.len(),
        warnings,
        options: bundle.options.clone(),
    })
}

async fn upsert_project_slug(pool: &PgPool, slug: &str) -> Result<Uuid, ApiError> {
    let row = sqlx::query(
        r#"
        INSERT INTO projects (id, slug, name, root_path)
        VALUES (gen_random_uuid(), $1, $1, $1)
        ON CONFLICT (slug) DO UPDATE SET name = EXCLUDED.name
        RETURNING id
        "#,
    )
    .bind(slug)
    .fetch_one(pool)
    .await
    .map_err(ApiError::sql)?;
    row.try_get("id").map_err(ApiError::sql)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ConfigFingerprint {
    exists: bool,
    modified: Option<SystemTime>,
    len: Option<u64>,
}

async fn wait_for_config_change(
    path: &FsPath,
    previous: ConfigFingerprint,
) -> Result<ConfigFingerprint> {
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let current = config_path_fingerprint(Some(path)).await?;
        if current != previous {
            return Ok(current);
        }
    }
}

async fn config_path_fingerprint(path: Option<&FsPath>) -> Result<ConfigFingerprint> {
    let Some(path) = path else {
        return Ok(ConfigFingerprint {
            exists: false,
            modified: None,
            len: None,
        });
    };

    match tokio::fs::metadata(path).await {
        Ok(metadata) => Ok(ConfigFingerprint {
            exists: true,
            modified: metadata.modified().ok(),
            len: Some(metadata.len()),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ConfigFingerprint {
            exists: false,
            modified: None,
            len: None,
        }),
        Err(error) => Err(error).with_context(|| format!("read metadata for {}", path.display())),
    }
}

async fn healthz(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    Ok(Json(health_payload(&state).await.map_err(ApiError::io)?))
}

async fn agents_snapshot() -> Result<Json<serde_json::Value>, ApiError> {
    let snapshot = tokio::task::spawn_blocking(|| {
        let mut top = mem_agenttop::AgentTop::new();
        top.collect_snapshot()
    })
    .await
    .map_err(|e| ApiError::io(anyhow::anyhow!("agent snapshot task failed: {e}")))?;

    let sessions: Vec<serde_json::Value> = snapshot
        .sessions
        .iter()
        .map(|s| {
            let status = match s.status {
                mem_agenttop::SessionStatus::Working => "working",
                mem_agenttop::SessionStatus::Waiting => "waiting",
                mem_agenttop::SessionStatus::Done => "done",
            };
            let children: Vec<serde_json::Value> = s
                .children
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "pid": c.pid,
                        "command": c.command,
                        "mem_kb": c.mem_kb,
                        "port": c.port,
                    })
                })
                .collect();
            let subagents: Vec<serde_json::Value> = s
                .subagents
                .iter()
                .map(|a| {
                    serde_json::json!({
                        "name": a.name,
                        "status": a.status,
                        "tokens": a.tokens,
                    })
                })
                .collect();
            serde_json::json!({
                "agent_cli": s.agent_cli,
                "pid": s.pid,
                "session_id": s.session_id,
                "cwd": s.cwd,
                "project_name": s.project_name,
                "started_at": s.started_at,
                "status": status,
                "model": s.model,
                "context_percent": s.context_percent,
                "total_input_tokens": s.total_input_tokens,
                "total_output_tokens": s.total_output_tokens,
                "total_cache_read": s.total_cache_read,
                "total_cache_create": s.total_cache_create,
                "turn_count": s.turn_count,
                "current_tasks": s.current_tasks,
                "mem_mb": s.mem_mb,
                "version": s.version,
                "git_branch": s.git_branch,
                "git_added": s.git_added,
                "git_modified": s.git_modified,
                "token_history": s.token_history,
                "subagents": subagents,
                "mem_file_count": s.mem_file_count,
                "mem_line_count": s.mem_line_count,
                "children": children,
                "initial_prompt": s.initial_prompt,
                "first_assistant_text": s.first_assistant_text,
            })
        })
        .collect();

    let orphan_ports: Vec<serde_json::Value> = snapshot
        .orphan_ports
        .iter()
        .map(|o| {
            serde_json::json!({
                "port": o.port,
                "pid": o.pid,
                "command": o.command,
                "project_name": o.project_name,
            })
        })
        .collect();
    let rate_limits: Vec<serde_json::Value> = snapshot
        .rate_limits
        .iter()
        .map(|rate_limit| {
            serde_json::json!({
                "source": rate_limit.source,
                "five_hour_pct": rate_limit.five_hour_pct,
                "five_hour_resets_at": rate_limit.five_hour_resets_at,
                "seven_day_pct": rate_limit.seven_day_pct,
                "seven_day_resets_at": rate_limit.seven_day_resets_at,
                "updated_at": rate_limit.updated_at,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "collected_at": snapshot.collected_at.to_rfc3339(),
        "sessions": sessions,
        "orphan_ports": orphan_ports,
        "rate_limits": rate_limits,
    })))
}

async fn admin_shutdown(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_strict_token(&headers, &state.api_token)?;
    request_runtime_shutdown(&state.shutdown);
    Ok(Json(serde_json::json!({
        "accepted": true,
        "message": "shutdown requested"
    })))
}

async fn web_unavailable() -> impl IntoResponse {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Html(
            r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <title>Memory Layer Web UI unavailable</title>
    <style>
      body { font-family: ui-sans-serif, system-ui, sans-serif; background: #0f1722; color: #e6edf5; margin: 0; }
      main { max-width: 760px; margin: 8rem auto; padding: 2rem; background: #182233; border: 1px solid #42506a; border-radius: 18px; }
      code { color: #ffd17d; }
      h1 { margin-top: 0; }
      p { line-height: 1.6; }
    </style>
  </head>
  <body>
    <main>
      <h1>Memory Layer Web UI is not installed</h1>
      <p><code>mem-service</code> is running, but it could not find built web assets.</p>
      <p>Build the frontend under <code>web/</code> or install a package that ships <code>share/memory-layer/web</code>.</p>
    </main>
  </body>
</html>"#,
        ),
    )
}

async fn query(
    State(state): State<AppState>,
    Json(request): Json<QueryRequest>,
) -> Result<Json<mem_api::QueryResponse>, ApiError> {
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/query", &request, false).await?,
        ));
    }
    let pool = state.pool()?;
    let embedders = state.embedders.read().await;
    match query_memory(pool, &request, embedders.active()).await {
        Ok(mut response) => {
            if should_enrich_query_answer_with_llm(&request) {
                enrich_query_answer_with_llm(&state, &request, &mut response).await;
            }
            notify_project_changed_with_metadata(
                &state,
                request.project.clone(),
                None,
                ActivityKind::Query,
                format!("Query: {}", summarize_query(&request.query)),
                Some(query_activity_details(&request, &response)),
                None,
                None,
                Some("query".to_string()),
                None,
                Some(response.answer_generation.duration_ms),
                Some(state.config.llm.provider.clone()),
                Some(state.config.llm.model.clone()),
                response.answer_generation.token_usage.clone(),
            );
            Ok(Json(response))
        }
        Err(error) => {
            notify_project_changed(
                &state,
                request.project.clone(),
                None,
                ActivityKind::QueryError,
                format!("Query error: {}", summarize_query(&request.query)),
                Some(ActivityDetails::Query {
                    query: request.query.clone(),
                    top_k: request.top_k,
                    result_count: 0,
                    confidence: 0.0,
                    insufficient_evidence: true,
                    total_duration_ms: 0,
                    graph_status: None,
                    graph_candidates: 0,
                    graph_augmented_candidates: 0,
                    graph_duration_ms: 0,
                    graph_result_count: 0,
                    graph_connection_count: 0,
                    graph_connections: Vec::new(),
                    answer: None,
                    error: Some(error.to_string()),
                }),
            );
            Err(ApiError::io(error))
        }
    }
}

fn should_enrich_query_answer_with_llm(request: &QueryRequest) -> bool {
    matches!(
        request.answer_mode.unwrap_or_default(),
        QueryAnswerMode::Auto | QueryAnswerMode::Llm
    )
}

fn query_activity_details(request: &QueryRequest, response: &QueryResponse) -> ActivityDetails {
    let graph_connections = query_activity_graph_connections(response);
    let graph_connection_count = response
        .results
        .iter()
        .map(|result| result.graph_connections.len())
        .sum();
    let graph_result_count = response
        .results
        .iter()
        .filter(|result| !result.graph_connections.is_empty() || result.debug.graph_boost > 0.0)
        .count();

    ActivityDetails::Query {
        query: request.query.clone(),
        top_k: request.top_k,
        result_count: response.results.len(),
        confidence: response.confidence,
        insufficient_evidence: response.insufficient_evidence,
        total_duration_ms: response.diagnostics.total_duration_ms,
        graph_status: if response.diagnostics.graph_status.is_empty() {
            None
        } else {
            Some(response.diagnostics.graph_status.clone())
        },
        graph_candidates: response.diagnostics.graph_candidates,
        graph_augmented_candidates: response.diagnostics.graph_augmented_candidates,
        graph_duration_ms: response.diagnostics.graph_duration_ms,
        graph_result_count,
        graph_connection_count,
        graph_connections,
        answer: Some(response.answer.clone()),
        error: None,
    }
}

fn query_activity_graph_connections(response: &QueryResponse) -> Vec<QueryGraphConnection> {
    response
        .results
        .iter()
        .flat_map(|result| result.graph_connections.iter().cloned())
        .take(QUERY_ACTIVITY_GRAPH_CONNECTION_LIMIT)
        .collect()
}

async fn capture_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CaptureTaskRequest>,
) -> Result<Json<mem_api::CaptureTaskResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/capture/task", &request, true).await?,
        ));
    }
    let task_title = request.task_title.clone();
    let project = request.project.clone();
    let response = if request.dry_run {
        preview_capture(state.pool()?, &request)
            .await
            .map_err(ApiError::sql)?
    } else {
        store_capture(state.pool()?, &request)
            .await
            .map_err(ApiError::sql)?
    };
    if request.dry_run {
        return Ok(Json(response));
    }
    notify_project_changed(
        &state,
        project,
        None,
        ActivityKind::CaptureTask,
        format!("Captured task: {task_title}"),
        Some(ActivityDetails::CaptureTask {
            session_id: response.session_id,
            task_id: response.task_id,
            raw_capture_id: response.raw_capture_id,
            idempotency_key: response.idempotency_key.clone(),
            task_title: Some(task_title.clone()),
            writer_id: request.writer_id.clone(),
        }),
    );
    Ok(Json(response))
}

async fn scan_activity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ScanActivityRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/scan/activity", &request, true).await?,
        ));
    }

    let summary = if request.dry_run {
        format!(
            "Scanned repository in dry-run mode and accepted {} candidate memory entry/entries.",
            request.candidate_count
        )
    } else {
        format!(
            "Scanned repository and accepted {} candidate memory entry/entries.",
            request.candidate_count
        )
    };
    notify_project_changed(
        &state,
        request.project.clone(),
        None,
        ActivityKind::Scan,
        summary,
        Some(ActivityDetails::Scan {
            dry_run: request.dry_run,
            candidate_count: request.candidate_count,
            files_considered: request.files_considered,
            commits_considered: request.commits_considered,
            index_reused: request.index_reused,
            report_path: request.report_path.clone(),
            capture_id: request.capture_id.clone(),
            curate_run_id: request.curate_run_id.clone(),
        }),
    );
    Ok(Json(serde_json::json!({ "logged": true })))
}

async fn graph_activity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<GraphActivityRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/graph/activity", &request, true).await?,
        ));
    }

    notify_project_changed(
        &state,
        request.project.clone(),
        None,
        ActivityKind::GraphExtract,
        graph_activity_summary(&request),
        Some(graph_activity_details(&request)),
    );
    Ok(Json(serde_json::json!({ "logged": true })))
}

fn graph_activity_summary(request: &GraphActivityRequest) -> String {
    let verb = if request.reused_existing_run {
        "Reused code graph extraction"
    } else if request.dry_run {
        "Previewed code graph extraction"
    } else {
        "Extracted code graph"
    };
    format!(
        "{verb}: {} symbols, {} references, {} graph edge(s).",
        request.symbol_count, request.reference_count, request.graph_edge_count
    )
}

fn graph_activity_details(request: &GraphActivityRequest) -> ActivityDetails {
    ActivityDetails::GraphExtract {
        repo_root: request.repo_root.clone(),
        git_head: request.git_head.clone(),
        since: request.since.clone(),
        extraction_run_id: request.extraction_run_id,
        dry_run: request.dry_run,
        reused_existing_run: request.reused_existing_run,
        index_reused: request.index_reused,
        analyzer_version: request.analyzer_version.clone(),
        strategy_version: request.strategy_version.clone(),
        symbol_count: request.symbol_count,
        reference_count: request.reference_count,
        resolved_reference_count: request.resolved_reference_count,
        unresolved_reference_count: request.unresolved_reference_count,
        ambiguous_reference_count: request.ambiguous_reference_count,
        graph_node_count: request.graph_node_count,
        graph_edge_count: request.graph_edge_count,
        evidence_count: request.evidence_count,
    }
}

async fn checkpoint_activity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CheckpointActivityRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/checkpoint/activity", &request, true).await?,
        ));
    }

    let summary = if let Some(note) = request.checkpoint.note.as_deref() {
        format!("Saved checkpoint for project {} ({note})", request.project)
    } else {
        format!("Saved checkpoint for project {}", request.project)
    };
    notify_project_changed(
        &state,
        request.project.clone(),
        None,
        ActivityKind::Checkpoint,
        summary,
        Some(ActivityDetails::Checkpoint {
            repo_root: request.checkpoint.repo_root.clone(),
            marked_at: request.checkpoint.marked_at,
            note: request.checkpoint.note.clone(),
            git_branch: request.checkpoint.git_branch.clone(),
            git_head: request.checkpoint.git_head.clone(),
        }),
    );
    Ok(Json(serde_json::json!({ "logged": true })))
}

async fn plan_activity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<PlanActivityRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/plan/activity", &request, true).await?,
        ));
    }

    let remaining_count = request.remaining_items.len();
    let verified_complete = matches!(request.action, PlanActivityAction::FinishVerified);
    let summary = match &request.action {
        PlanActivityAction::Started => {
            format!("Recorded approved plan for execution: {}", request.title)
        }
        PlanActivityAction::Synced => {
            format!("Synced approved plan state: {}", request.title)
        }
        PlanActivityAction::FinishBlocked => format!(
            "Plan completion blocked: {} ({} remaining item(s))",
            request.title, remaining_count
        ),
        PlanActivityAction::FinishVerified => {
            format!("Verified approved plan complete: {}", request.title)
        }
    };
    notify_project_changed(
        &state,
        request.project.clone(),
        None,
        ActivityKind::Plan,
        summary,
        Some(ActivityDetails::Plan {
            action: request.action.clone(),
            title: request.title.clone(),
            thread_key: request.thread_key.clone(),
            total_items: request.total_items,
            completed_items: request.completed_items,
            remaining_items: request.remaining_items.clone(),
            source_path: request.source_path.clone(),
            verified_complete,
        }),
    );
    Ok(Json(serde_json::json!({ "logged": true })))
}

async fn curate_memory(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CurateRequest>,
) -> Result<Json<mem_api::CurateResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/curate", &request, true).await?,
        ));
    }
    let project = request.project.clone();
    let response = if request.dry_run {
        preview_curate(state.pool()?, &request)
            .await
            .map_err(ApiError::sql)?
    } else {
        curate(state.pool()?, &request)
            .await
            .map_err(ApiError::sql)?
    };
    if request.dry_run {
        return Ok(Json(response));
    }
    let embedders = state.embedders.read().await;
    if !embedders.is_empty() {
        rebuild_chunks_for_automatic_creation(
            state.pool()?,
            &request.project,
            &embedders,
            state
                .automated_embedding_creation_enabled
                .load(Ordering::Relaxed),
        )
        .await
        .map_err(ApiError::io)?;
    }
    notify_project_changed(
        &state,
        project,
        None,
        ActivityKind::Curate,
        format!(
            "Curated {} capture(s) into {} memory entry/entries with {} replacement(s) and {} queued update proposal(s).",
            response.input_count,
            response.output_count,
            response.replaced_count,
            response.proposal_count
        ),
        Some(ActivityDetails::Curate {
            run_id: response.run_id,
            input_count: response.input_count,
            output_count: response.output_count,
            replaced_count: response.replaced_count,
            proposal_count: response.proposal_count,
        }),
    );
    for replacement in &response.replacements {
        notify_project_changed(
            &state,
            request.project.clone(),
            Some(replacement.new_memory_id),
            ActivityKind::MemoryReplacement,
            format!(
                "Replaced memory \"{}\" with \"{}\".",
                replacement.old_summary, replacement.new_summary
            ),
            Some(ActivityDetails::MemoryReplacement {
                old_memory_id: replacement.old_memory_id,
                old_summary: replacement.old_summary.clone(),
                new_memory_id: replacement.new_memory_id,
                new_summary: replacement.new_summary.clone(),
                automatic: replacement.automatic,
                policy: replacement.policy,
            }),
        );
    }
    Ok(Json(response))
}

async fn reindex(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ReindexRequest>,
) -> Result<Json<ReindexResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/reindex", &request, true).await?,
        ));
    }
    let project = request.project.clone();
    let count = if request.dry_run {
        sqlx::query(
            r#"
            SELECT COUNT(*) AS count
            FROM memory_entries m
            JOIN projects p ON p.id = m.project_id
            WHERE p.slug = $1
            "#,
        )
        .bind(&request.project)
        .fetch_one(state.pool()?)
        .await
        .map_err(ApiError::sql)?
        .try_get::<i64, _>("count")
        .map_err(ApiError::sql)? as u64
    } else {
        let embedders = state.embedders.read().await;
        rebuild_chunks(
            state.pool()?,
            &request.project,
            &embedders,
            request.backend.as_deref(),
        )
        .await
        .map_err(ApiError::io)?
    };
    if request.dry_run {
        return Ok(Json(ReindexResponse {
            reindexed_entries: count,
            dry_run: true,
        }));
    }
    notify_project_changed(
        &state,
        project,
        None,
        ActivityKind::Reindex,
        format!("Reindexed {count} memory entry/entries."),
        Some(ActivityDetails::Reindex {
            reindexed_entries: count,
        }),
    );
    Ok(Json(ReindexResponse {
        reindexed_entries: count,
        dry_run: false,
    }))
}

async fn reembed(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ReembedRequest>,
) -> Result<Json<ReembedResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/reembed", &request, true).await?,
        ));
    }
    let embedders = state.embedders.read().await;
    if embedders.is_empty() {
        return Err(ApiError::validation(ValidationError::new(
            "embeddings are not configured; cannot re-embed",
        )));
    }
    let selected_keys: Vec<(String, String)> = match request.backend.as_deref() {
        Some(name) => {
            let service = embedders.get(name).ok_or_else(|| {
                ApiError::validation(ValidationError::new(format!(
                    "unknown embedding backend: {name}"
                )))
            })?;
            vec![(name.to_string(), service.embedding_space_key())]
        }
        None => embedders
            .iter()
            .map(|(name, service)| (name.to_string(), service.embedding_space_key()))
            .collect(),
    };
    let project = request.project.clone();
    let count = if request.dry_run {
        let mut total: i64 = 0;
        for (_, space_key) in &selected_keys {
            total += sqlx::query(
                r#"
                SELECT COUNT(*) AS count
                FROM memory_chunks mc
                JOIN memory_entries m ON m.id = mc.memory_entry_id
                JOIN projects p ON p.id = m.project_id
                LEFT JOIN memory_chunk_embeddings mce
                  ON mce.chunk_id = mc.id
                 AND mce.embedding_space = $2
                WHERE p.slug = $1
                  AND m.status = 'active'
                  AND (
                        mce.chunk_id IS NULL
                        OR mce.embedding_dimension IS NULL
                      )
                "#,
            )
            .bind(&request.project)
            .bind(space_key)
            .fetch_one(state.pool()?)
            .await
            .map_err(ApiError::sql)?
            .try_get::<i64, _>("count")
            .map_err(ApiError::sql)?;
        }
        total as u64
    } else {
        reembed_project_chunks(
            state.pool()?,
            &request.project,
            &embedders,
            request.backend.as_deref(),
        )
        .await
        .map_err(ApiError::io)?
    };
    if request.dry_run {
        return Ok(Json(ReembedResponse {
            reembedded_chunks: count,
            dry_run: true,
        }));
    }
    notify_project_changed(
        &state,
        project,
        None,
        ActivityKind::Reembed,
        format!("Re-embedded {count} chunk(s)."),
        Some(ActivityDetails::Reembed {
            reembedded_chunks: count,
        }),
    );
    Ok(Json(ReembedResponse {
        reembedded_chunks: count,
        dry_run: false,
    }))
}

async fn prune_embeddings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<PruneEmbeddingsRequest>,
) -> Result<Json<PruneEmbeddingsResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/prune-embeddings", &request, true).await?,
        ));
    }
    let embedders = state.embedders.read().await;
    if embedders.is_empty() {
        return Err(ApiError::validation(ValidationError::new(
            "embeddings are not configured; cannot prune inactive spaces",
        )));
    }
    let keep: Vec<String> = embedders
        .iter()
        .map(|(_, service)| service.embedding_space_key())
        .collect();
    let project = request.project.clone();
    let count = if request.dry_run {
        sqlx::query(
            r#"
            SELECT COUNT(*) AS count
            FROM memory_chunk_embeddings mce
            JOIN memory_chunks mc ON mc.id = mce.chunk_id
            JOIN memory_entries m ON m.id = mc.memory_entry_id
            JOIN projects p ON p.id = m.project_id
            WHERE p.slug = $1
              AND m.status = 'active'
              AND mce.embedding_space <> ALL($2)
            "#,
        )
        .bind(&request.project)
        .bind(&keep)
        .fetch_one(state.pool()?)
        .await
        .map_err(ApiError::sql)?
        .try_get::<i64, _>("count")
        .map_err(ApiError::sql)? as u64
    } else {
        prune_project_embeddings(state.pool()?, &request.project, &embedders)
            .await
            .map_err(ApiError::io)?
    };
    if request.dry_run {
        return Ok(Json(PruneEmbeddingsResponse {
            pruned_embeddings: count,
            dry_run: true,
        }));
    }
    notify_project_changed(
        &state,
        project,
        None,
        ActivityKind::Reembed,
        format!("Pruned {count} inactive embedding row(s)."),
        Some(ActivityDetails::Reembed {
            reembedded_chunks: count,
        }),
    );
    Ok(Json(PruneEmbeddingsResponse {
        pruned_embeddings: count,
        dry_run: false,
    }))
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
struct EmbeddingBackendsQuery {
    project: Option<String>,
}

async fn list_embedding_backends(
    State(state): State<AppState>,
    Query(params): Query<EmbeddingBackendsQuery>,
) -> Result<Json<EmbeddingBackendsResponse>, ApiError> {
    build_embedding_backends_response(&state, params.project.as_deref()).await
}

async fn build_embedding_backends_response(
    state: &AppState,
    project: Option<&str>,
) -> Result<Json<EmbeddingBackendsResponse>, ApiError> {
    let embedders = state.embedders.read().await;
    let active_name = embedders.active_name().map(|s| s.to_string());
    // Map name -> space_key for ready backends so we can merge coverage
    // counts (which are grouped by embedding_space) back by name.
    let space_by_name: std::collections::HashMap<String, String> = embedders
        .iter()
        .map(|(name, service)| (name.to_string(), service.embedding_space_key()))
        .collect();
    let ready: std::collections::HashSet<String> = space_by_name.keys().cloned().collect();

    let coverage_by_space: std::collections::HashMap<String, (i64, i64)> = match project {
        Some(slug) => fetch_project_embedding_coverage(state, slug).await?,
        None => std::collections::HashMap::new(),
    };

    let backends = state
        .config
        .embeddings
        .backends
        .iter()
        .map(|backend| {
            let base_url = effective_embedding_base_url(&backend.provider, &backend.base_url)
                .unwrap_or_else(|| backend.base_url.trim_end_matches('/').to_string());
            let (project_chunk_count, project_memory_count) = if project.is_some() {
                match space_by_name
                    .get(&backend.name)
                    .and_then(|key| coverage_by_space.get(key))
                {
                    Some((chunks, memories)) => (Some(*chunks), Some(*memories)),
                    None => (Some(0), Some(0)),
                }
            } else {
                (None, None)
            };
            EmbeddingBackendInfo {
                name: backend.name.clone(),
                provider: backend.provider.clone(),
                base_url,
                model: backend.model.clone(),
                active: active_name.as_deref() == Some(backend.name.as_str()),
                ready: ready.contains(&backend.name),
                create_enabled: if ready.contains(&backend.name) {
                    embedders.create_enabled(&backend.name)
                } else {
                    backend.create_enabled
                },
                project_chunk_count,
                project_memory_count,
            }
        })
        .collect();
    Ok(Json(EmbeddingBackendsResponse {
        backends,
        active: active_name,
        create_enabled: state
            .automated_embedding_creation_enabled
            .load(Ordering::Relaxed),
    }))
}

async fn fetch_project_embedding_coverage(
    state: &AppState,
    slug: &str,
) -> Result<std::collections::HashMap<String, (i64, i64)>, ApiError> {
    let Some(pool) = state.pool.as_ref() else {
        return Ok(std::collections::HashMap::new());
    };
    let rows = sqlx::query(
        r#"
        SELECT mce.embedding_space,
               COUNT(*)::bigint                       AS chunk_count,
               COUNT(DISTINCT mc.memory_entry_id)::bigint AS memory_count
        FROM memory_chunk_embeddings mce
        JOIN memory_chunks mc ON mc.id = mce.chunk_id
        JOIN memory_entries m ON m.id = mc.memory_entry_id
        JOIN projects p ON p.id = m.project_id
        WHERE p.slug = $1
          AND m.status = 'active'
          AND m.is_tombstone = FALSE
        GROUP BY mce.embedding_space
        "#,
    )
    .bind(slug)
    .fetch_all(pool)
    .await
    .map_err(ApiError::sql)?;

    let mut map = std::collections::HashMap::with_capacity(rows.len());
    for row in rows {
        let space: String = row.try_get("embedding_space").map_err(ApiError::sql)?;
        let chunk_count: i64 = row.try_get("chunk_count").map_err(ApiError::sql)?;
        let memory_count: i64 = row.try_get("memory_count").map_err(ApiError::sql)?;
        insert_embedding_coverage_count(&mut map, space.clone(), chunk_count, memory_count);
        if let Some(alias) = equivalent_openai_embedding_space_key(&space) {
            insert_embedding_coverage_count(&mut map, alias, chunk_count, memory_count);
        }
    }
    Ok(map)
}

fn insert_embedding_coverage_count(
    map: &mut std::collections::HashMap<String, (i64, i64)>,
    space: String,
    chunk_count: i64,
    memory_count: i64,
) {
    map.entry(space)
        .and_modify(|(chunks, memories)| {
            *chunks = (*chunks).max(chunk_count);
            *memories = (*memories).max(memory_count);
        })
        .or_insert((chunk_count, memory_count));
}

fn equivalent_openai_embedding_space_key(space: &str) -> Option<String> {
    space
        .strip_prefix("openai|")
        .map(|suffix| format!("openai_compatible|{suffix}"))
        .or_else(|| {
            space
                .strip_prefix("openai_compatible|")
                .map(|suffix| format!("openai|{suffix}"))
        })
}

async fn activate_embedding_backend(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ActivateEmbeddingBackendRequest>,
) -> Result<Json<EmbeddingBackendsResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/embeddings/activate", &request, true).await?,
        ));
    }

    let previous_active = {
        let mut embedders = state.embedders.write().await;
        let previous = embedders.active_name().map(|s| s.to_string());
        embedders
            .set_active(&request.name)
            .map_err(|err| ApiError::validation(ValidationError::new(err.to_string())))?;
        previous
    };

    if let Err(err) = persist_active_embedding_backend(&state, Some(&request.name)).await {
        // Revert in-memory state so config and registry stay in sync.
        let mut embedders = state.embedders.write().await;
        if let Some(name) = previous_active {
            let _ = embedders.set_active(&name);
        }
        return Err(err);
    }

    build_embedding_backends_response(&state, None).await
}

async fn deactivate_embedding_backend(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(_request): Json<mem_api::DeactivateEmbeddingBackendRequest>,
) -> Result<Json<EmbeddingBackendsResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(
                &state,
                "/v1/embeddings/deactivate",
                &mem_api::DeactivateEmbeddingBackendRequest::default(),
                true,
            )
            .await?,
        ));
    }

    let previous_active = {
        let mut embedders = state.embedders.write().await;
        let previous = embedders.active_name().map(|s| s.to_string());
        embedders.clear_active();
        previous
    };

    if let Err(err) = persist_active_embedding_backend(&state, None).await {
        if let Some(name) = previous_active {
            let mut embedders = state.embedders.write().await;
            let _ = embedders.set_active(&name);
        }
        return Err(err);
    }

    build_embedding_backends_response(&state, None).await
}

async fn set_embedding_creation_enabled(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<SetEmbeddingCreationRequest>,
) -> Result<Json<EmbeddingBackendsResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/embeddings/create-enabled", &request, true).await?,
        ));
    }

    let name = request.name.trim();
    if name.is_empty() {
        return Err(ApiError::validation(ValidationError::new(
            "name must be non-empty",
        )));
    }
    if !state
        .config
        .embeddings
        .backends
        .iter()
        .any(|backend| backend.name == name)
    {
        return Err(ApiError::validation(ValidationError::new(format!(
            "unknown embedding backend: {name}"
        ))));
    }

    let previous = {
        let mut embedders = state.embedders.write().await;
        let previous = embedders.create_enabled(name);
        if embedders.get(name).is_some() {
            embedders
                .set_create_enabled(name, request.enabled)
                .map_err(|err| ApiError::validation(ValidationError::new(err.to_string())))?;
        }
        previous
    };
    let previous_global = state
        .automated_embedding_creation_enabled
        .swap(true, Ordering::Relaxed);
    if let Err(err) = persist_embedding_creation_enabled(&state, name, request.enabled).await {
        let mut embedders = state.embedders.write().await;
        if embedders.get(name).is_some() {
            let _ = embedders.set_create_enabled(name, previous);
        }
        state
            .automated_embedding_creation_enabled
            .store(previous_global, Ordering::Relaxed);
        return Err(err);
    }

    build_embedding_backends_response(&state, None).await
}

async fn persist_active_embedding_backend(
    state: &AppState,
    active_name: Option<&str>,
) -> Result<(), ApiError> {
    let Some(config_path) = state.config.resolved_config_path.clone() else {
        // Ephemeral (env-var only) config — no file to rewrite. The
        // in-memory activation is still applied, but it will not survive
        // a restart. Surface this to the caller as a soft warning via
        // tracing rather than an error.
        tracing::warn!(
            "changed active embedding backend without persistence: no TOML config file is resolved"
        );
        return Ok(());
    };
    let existing = tokio::fs::read_to_string(&config_path)
        .await
        .map_err(|err| ApiError::io(anyhow::anyhow!("read {}: {err}", config_path.display())))?;
    let rendered = set_active_embedding_backend_in_toml(&existing, active_name)
        .map_err(|err| ApiError::io(anyhow::anyhow!("update {}: {err}", config_path.display())))?;
    let tmp_path = config_path.with_extension("toml.tmp");
    tokio::fs::write(&tmp_path, rendered.as_bytes())
        .await
        .map_err(|err| ApiError::io(anyhow::anyhow!("write {}: {err}", tmp_path.display())))?;
    tokio::fs::rename(&tmp_path, &config_path)
        .await
        .map_err(|err| {
            ApiError::io(anyhow::anyhow!(
                "rename {} -> {}: {err}",
                tmp_path.display(),
                config_path.display()
            ))
        })?;
    Ok(())
}

async fn persist_embedding_creation_enabled(
    state: &AppState,
    name: &str,
    enabled: bool,
) -> Result<(), ApiError> {
    let Some(config_path) = state.config.resolved_config_path.clone() else {
        tracing::warn!(
            "changed automatic embedding creation without persistence: no TOML config file is resolved"
        );
        return Ok(());
    };
    let existing = tokio::fs::read_to_string(&config_path)
        .await
        .map_err(|err| ApiError::io(anyhow::anyhow!("read {}: {err}", config_path.display())))?;
    let rendered = set_embedding_creation_enabled_in_toml(&existing, name, enabled)
        .map_err(|err| ApiError::io(anyhow::anyhow!("update {}: {err}", config_path.display())))?;
    let tmp_path = config_path.with_extension("toml.tmp");
    tokio::fs::write(&tmp_path, rendered.as_bytes())
        .await
        .map_err(|err| ApiError::io(anyhow::anyhow!("write {}: {err}", tmp_path.display())))?;
    tokio::fs::rename(&tmp_path, &config_path)
        .await
        .map_err(|err| {
            ApiError::io(anyhow::anyhow!(
                "rename {} -> {}: {err}",
                tmp_path.display(),
                config_path.display()
            ))
        })?;
    Ok(())
}

fn set_active_embedding_backend_in_toml(
    existing: &str,
    active_name: Option<&str>,
) -> anyhow::Result<String> {
    let mut doc = existing.parse::<toml_edit::DocumentMut>()?;
    // Ensure [embeddings] table exists.
    if !doc.contains_key("embeddings") {
        doc["embeddings"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let embeddings = doc["embeddings"]
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[embeddings] is not a table in config"))?;
    match active_name {
        Some(name) => {
            embeddings["enabled"] = toml_edit::value(true);
            embeddings["active"] = toml_edit::value(name);
        }
        None => {
            embeddings["enabled"] = toml_edit::value(false);
        }
    }
    Ok(doc.to_string())
}

fn set_embedding_creation_enabled_in_toml(
    existing: &str,
    name: &str,
    enabled: bool,
) -> anyhow::Result<String> {
    let mut doc = existing.parse::<toml_edit::DocumentMut>()?;
    if !doc.contains_key("embeddings") {
        doc["embeddings"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let embeddings = doc["embeddings"]
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[embeddings] is not a table in config"))?;
    embeddings["create_enabled"] = toml_edit::value(true);
    if let Some(backends) = embeddings
        .get_mut("backends")
        .and_then(|item| item.as_array_of_tables_mut())
    {
        let mut updated = false;
        for backend in backends.iter_mut() {
            if backend
                .get("name")
                .and_then(|value| value.as_str())
                .is_some_and(|value| value == name)
            {
                backend["create_enabled"] = toml_edit::value(enabled);
                updated = true;
                break;
            }
        }
        if updated {
            return Ok(doc.to_string());
        }
    }
    embeddings["create_enabled"] = toml_edit::value(enabled);
    Ok(doc.to_string())
}

async fn get_memory(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<MemoryEntryResponse>, ApiError> {
    if !state.is_primary() {
        return Ok(Json(
            proxy_get_json(&state, &format!("/v1/memory/{id}")).await?,
        ));
    }
    let detail = fetch_memory_entry(state.pool()?, id)
        .await
        .map_err(ApiError::sql)?
        .ok_or_else(|| ApiError::not_found("memory entry not found"))?;
    Ok(Json(detail))
}

async fn get_memory_history(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<MemoryHistoryResponse>, ApiError> {
    if !state.is_primary() {
        return Ok(Json(
            proxy_get_json(&state, &format!("/v1/memory/{id}/history")).await?,
        ));
    }
    let pool = state.pool()?;
    // Walk back to the canonical_id of the provided version, then pull every
    // sibling version in chronological order. The caller can pass any
    // version's id (including a tombstone) and get the same chain.
    let anchor = sqlx::query(
        r#"
        SELECT m.canonical_id, p.slug
        FROM memory_entries m
        JOIN projects p ON p.id = m.project_id
        WHERE m.id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::sql)?
    .ok_or_else(|| ApiError::not_found("memory entry not found"))?;

    let canonical_id: Uuid = anchor.try_get("canonical_id").map_err(ApiError::sql)?;
    let project: String = anchor.try_get("slug").map_err(ApiError::sql)?;

    let version_ids: Vec<Uuid> = sqlx::query(
        r#"
        SELECT id
        FROM memory_entries
        WHERE canonical_id = $1
        ORDER BY version_no ASC
        "#,
    )
    .bind(canonical_id)
    .fetch_all(pool)
    .await
    .map_err(ApiError::sql)?
    .into_iter()
    .map(|row| row.try_get::<Uuid, _>("id"))
    .collect::<Result<Vec<_>, _>>()
    .map_err(ApiError::sql)?;

    let mut versions = Vec::with_capacity(version_ids.len());
    for version_id in version_ids {
        if let Some(entry) = fetch_memory_entry(pool, version_id)
            .await
            .map_err(ApiError::sql)?
        {
            versions.push(entry);
        }
    }

    Ok(Json(MemoryHistoryResponse {
        canonical_id,
        project,
        versions,
    }))
}

async fn stats(State(state): State<AppState>) -> Result<Json<StatsResponse>, ApiError> {
    if !state.is_primary() {
        return Ok(Json(proxy_get_json(&state, "/v1/stats").await?));
    }
    let pool = state.pool()?;
    let counts = [
        ("projects", "SELECT COUNT(*) AS count FROM projects"),
        ("sessions", "SELECT COUNT(*) AS count FROM sessions"),
        ("tasks", "SELECT COUNT(*) AS count FROM tasks"),
        ("raw_captures", "SELECT COUNT(*) AS count FROM raw_captures"),
        (
            "memory_entries",
            "SELECT COUNT(*) AS count FROM memory_entries",
        ),
        (
            "curation_runs",
            "SELECT COUNT(*) AS count FROM curation_runs",
        ),
    ];

    let mut values = Vec::with_capacity(counts.len());
    for (_, sql) in counts {
        let row = sqlx::query(sql)
            .fetch_one(pool)
            .await
            .map_err(ApiError::sql)?;
        values.push(row.try_get::<i64, _>("count").map_err(ApiError::sql)?);
    }

    Ok(Json(StatsResponse {
        projects: values[0],
        sessions: values[1],
        tasks: values[2],
        raw_captures: values[3],
        memory_entries: values[4],
        curation_runs: values[5],
    }))
}

async fn sync_commits(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CommitSyncRequest>,
) -> Result<Json<CommitSyncResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/commits/sync", &request, true).await?,
        ));
    }
    let project = request.project.clone();
    let response = if request.dry_run {
        preview_project_commit_sync(state.pool()?, &request)
            .await
            .map_err(ApiError::sql)?
    } else {
        sync_project_commits(state.pool()?, &request)
            .await
            .map_err(ApiError::sql)?
    };
    if request.dry_run {
        return Ok(Json(response));
    }
    notify_project_changed(
        &state,
        project,
        None,
        ActivityKind::CommitSync,
        format!(
            "Synced {} commit(s): {} imported, {} updated.",
            response.total_received, response.imported_count, response.updated_count
        ),
        Some(ActivityDetails::CommitSync {
            imported_count: response.imported_count,
            updated_count: response.updated_count,
            total_received: response.total_received,
            newest_commit: response.newest_commit.clone(),
            oldest_commit: response.oldest_commit.clone(),
        }),
    );
    Ok(Json(response))
}

#[derive(Debug, Default, Deserialize)]
struct ProjectMemoriesParams {
    status: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
struct ProjectCommitsParams {
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn project_memories(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(params): Query<ProjectMemoriesParams>,
) -> Result<Json<ProjectMemoriesResponse>, ApiError> {
    if !state.is_primary() {
        let suffix = format!(
            "?limit={}&offset={}",
            params.limit.unwrap_or(200).clamp(1, 500),
            params.offset.unwrap_or(0).max(0)
        );
        let mut path = format!("/v1/projects/{slug}/memories{suffix}");
        if let Some(status) = &params.status {
            path.push_str("&status=");
            path.push_str(status);
        }
        return Ok(Json(proxy_get_json(&state, &path).await?));
    }
    let limit = params.limit.unwrap_or(200).clamp(1, 500);
    let offset = params.offset.unwrap_or(0).max(0);
    let status_filter = params
        .status
        .as_deref()
        .map(parse_status_filter)
        .transpose()
        .map_err(ApiError::validation)?;

    Ok(Json(
        fetch_project_memories(
            state.pool()?,
            &slug,
            status_filter.as_deref(),
            limit,
            offset,
        )
        .await
        .map_err(ApiError::sql)?,
    ))
}

async fn project_overview(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<ProjectOverviewResponse>, ApiError> {
    if !state.is_primary() {
        return Ok(Json(
            proxy_get_json(&state, &format!("/v1/projects/{slug}/overview")).await?,
        ));
    }
    Ok(Json(
        fetch_project_overview_with_watchers(&state, &slug)
            .await
            .map_err(ApiError::sql)?,
    ))
}

async fn project_bundle_export_preview(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(options): Json<ProjectMemoryExportOptions>,
) -> Result<Json<ProjectMemoryBundlePreview>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    let pool = state.pool()?;
    let memories = load_project_bundle_entries(pool, &slug, &options).await?;
    let (manifest, warnings) = build_bundle_manifest(&slug, &options, &memories)?;
    Ok(Json(build_export_preview(&manifest, warnings)))
}

async fn project_bundle_export(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(options): Json<ProjectMemoryExportOptions>,
) -> Result<Response, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    let pool = state.pool()?;
    let memories = load_project_bundle_entries(pool, &slug, &options).await?;
    let (manifest, _) = build_bundle_manifest(&slug, &options, &memories)?;
    let bytes = serialize_bundle_archive(&manifest)?;
    let filename = bundle_filename(&slug, &manifest.bundle_id);
    notify_project_changed(
        &state,
        slug.clone(),
        None,
        ActivityKind::BundleExport,
        format!("Exported memory bundle {}", manifest.bundle_id),
        Some(ActivityDetails::BundleTransfer {
            bundle_id: manifest.bundle_id.clone(),
            item_count: manifest.entries.len(),
            source_project: Some(slug.clone()),
        }),
    );
    let mut response = Response::new(bytes.into());
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/zip"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        header::HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .map_err(|error| ApiError::io(error.into()))?,
    );
    Ok(response)
}

async fn project_bundle_import_preview(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    body: Bytes,
) -> Result<Json<ProjectMemoryImportPreview>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    let loaded = load_bundle_archive(&body)?;
    let preview =
        preview_bundle_import(state.pool()?, &slug, &loaded.manifest, loaded.warnings).await?;
    Ok(Json(preview))
}

async fn project_bundle_import(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    body: Bytes,
) -> Result<Json<ProjectMemoryImportResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    let loaded = load_bundle_archive(&body)?;
    let pool = state.pool()?;
    let target_project_id = upsert_project_slug(pool, &slug).await?;
    let import_id = Uuid::new_v4();

    sqlx::query(
        r#"
        INSERT INTO memory_bundle_imports (id, target_project_id, bundle_id, bundle_hash, source_project_slug, summary, options_json, imported_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, now())
        "#,
    )
    .bind(import_id)
    .bind(target_project_id)
    .bind(&loaded.manifest.bundle_id)
    .bind(&loaded.manifest.bundle_hash)
    .bind(&loaded.manifest.source_project)
    .bind(&loaded.manifest.summary_markdown)
    .bind(sqlx::types::Json(&loaded.manifest.options))
    .execute(pool)
    .await
    .map_err(ApiError::sql)?;

    let mut imported_ids = Vec::new();
    let mut current_ids = HashMap::new();
    let mut skipped_count = 0usize;
    let mut replaced_count = 0usize;
    let mut imported_count = 0usize;

    for entry in &loaded.manifest.entries {
        let hash = entry_hash(entry)?;
        let existing = sqlx::query(
            r#"
            SELECT memory_entry_id, entry_hash
            FROM imported_memory_entries
            WHERE target_project_id = $1
              AND bundle_id = $2
              AND exported_entry_key = $3
            "#,
        )
        .bind(target_project_id)
        .bind(&loaded.manifest.bundle_id)
        .bind(&entry.entry_key)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::sql)?;

        let mut superseded_memory_id = None;
        if let Some(row) = existing {
            let existing_memory_id: Uuid = row.try_get("memory_entry_id").map_err(ApiError::sql)?;
            let existing_hash: String = row.try_get("entry_hash").map_err(ApiError::sql)?;
            if existing_hash == hash {
                current_ids.insert(entry.entry_key.clone(), existing_memory_id);
                skipped_count += 1;
                continue;
            }
            superseded_memory_id = Some(existing_memory_id);
            replaced_count += 1;
        }

        let memory_id = Uuid::new_v4();
        let (canonical_id, version_no) = if let Some(existing_memory_id) = superseded_memory_id {
            let row = sqlx::query(
                r#"
                SELECT canonical_id, MAX(version_no) OVER (PARTITION BY canonical_id) AS latest
                FROM memory_entries
                WHERE id = $1
                "#,
            )
            .bind(existing_memory_id)
            .fetch_one(pool)
            .await
            .map_err(ApiError::sql)?;
            (
                row.try_get::<Uuid, _>("canonical_id")
                    .map_err(ApiError::sql)?,
                row.try_get::<i32, _>("latest").map_err(ApiError::sql)? + 1,
            )
        } else {
            (memory_id, 1)
        };
        sqlx::query(
            r#"
            INSERT INTO memory_entries
                (id, project_id, canonical_id, version_no, is_tombstone, canonical_text, summary, memory_type, scope, importance, confidence, status, created_at, updated_at, archived_at, search_document)
            VALUES
                ($1, $2, $3, $4, FALSE, $5, $6, $7, 'project', $8, $9, 'active', $10, $11, NULL, to_tsvector('english', $5 || ' ' || $6))
            "#,
        )
        .bind(memory_id)
        .bind(target_project_id)
        .bind(canonical_id)
        .bind(version_no)
        .bind(&entry.canonical_text)
        .bind(&entry.summary)
        .bind(entry.memory_type.to_string())
        .bind(entry.importance)
        .bind(entry.confidence)
        .bind(entry.created_at)
        .bind(entry.updated_at)
        .execute(pool)
        .await
        .map_err(ApiError::sql)?;

        for tag in &entry.tags {
            sqlx::query(
                "INSERT INTO memory_tags (memory_entry_id, tag) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            )
            .bind(memory_id)
            .bind(tag)
            .execute(pool)
            .await
            .map_err(ApiError::sql)?;
        }

        for source in &entry.sources {
            sqlx::query(
                r#"
                INSERT INTO memory_sources (id, memory_entry_id, task_id, file_path, git_commit, source_kind, excerpt, created_at)
                VALUES ($1, $2, NULL, $3, $4, $5, $6, now())
                "#,
            )
            .bind(Uuid::new_v4())
            .bind(memory_id)
            .bind(&source.file_path)
            .bind(&source.git_commit)
            .bind(match source.source_kind {
                SourceKind::TaskPrompt => "task_prompt",
                SourceKind::File => "file",
                SourceKind::GitCommit => "git_commit",
                SourceKind::CommandOutput => "command_output",
                SourceKind::Test => "test",
                SourceKind::Note => "note",
            })
            .bind(&source.excerpt)
            .execute(pool)
            .await
            .map_err(ApiError::sql)?;
        }

        sqlx::query(
            r#"
            INSERT INTO imported_memory_entries (target_project_id, bundle_id, exported_entry_key, entry_hash, memory_entry_id, latest_import_id, imported_at)
            VALUES ($1, $2, $3, $4, $5, $6, now())
            ON CONFLICT (target_project_id, bundle_id, exported_entry_key) DO UPDATE
            SET entry_hash = EXCLUDED.entry_hash,
                memory_entry_id = EXCLUDED.memory_entry_id,
                latest_import_id = EXCLUDED.latest_import_id,
                imported_at = now()
            "#,
        )
        .bind(target_project_id)
        .bind(&loaded.manifest.bundle_id)
        .bind(&entry.entry_key)
        .bind(&hash)
        .bind(memory_id)
        .bind(import_id)
        .execute(pool)
        .await
        .map_err(ApiError::sql)?;

        current_ids.insert(entry.entry_key.clone(), memory_id);
        imported_ids.push(memory_id);
        imported_count += 1;
    }

    for memory_id in &imported_ids {
        refresh_memory_relations(pool, &slug, *memory_id)
            .await
            .map_err(ApiError::sql)?;
    }

    for entry in &loaded.manifest.entries {
        let Some(src_memory_id) = current_ids.get(&entry.entry_key).copied() else {
            continue;
        };
        sqlx::query("DELETE FROM memory_relations WHERE src_memory_id = $1")
            .bind(src_memory_id)
            .execute(pool)
            .await
            .map_err(ApiError::sql)?;
        for relation in &entry.relations {
            if let Some(dst_memory_id) = current_ids.get(&relation.target_entry_key).copied() {
                sqlx::query(
                    r#"
                    INSERT INTO memory_relations (id, src_memory_id, relation_type, dst_memory_id)
                    VALUES ($1, $2, $3, $4)
                    ON CONFLICT DO NOTHING
                    "#,
                )
                .bind(Uuid::new_v4())
                .bind(src_memory_id)
                .bind(relation.relation_type.to_string())
                .bind(dst_memory_id)
                .execute(pool)
                .await
                .map_err(ApiError::sql)?;
            }
        }
    }

    let embedders = state.embedders.read().await;
    rebuild_chunks_for_automatic_creation(
        pool,
        &slug,
        &embedders,
        state
            .automated_embedding_creation_enabled
            .load(Ordering::Relaxed),
    )
    .await
    .map_err(ApiError::io)?;

    notify_project_changed(
        &state,
        slug.clone(),
        None,
        ActivityKind::BundleImport,
        format!(
            "Imported memory bundle {} into {} memory entry/entries.",
            loaded.manifest.bundle_id, imported_count
        ),
        Some(ActivityDetails::BundleTransfer {
            bundle_id: loaded.manifest.bundle_id.clone(),
            item_count: imported_count,
            source_project: Some(loaded.manifest.source_project.clone()),
        }),
    );
    notify_project_refreshed(&state, slug.clone());

    Ok(Json(ProjectMemoryImportResponse {
        target_project: slug,
        bundle_id: loaded.manifest.bundle_id,
        bundle_hash: loaded.manifest.bundle_hash,
        imported_count,
        replaced_count,
        skipped_count,
        relation_count: loaded
            .manifest
            .entries
            .iter()
            .map(|entry| entry.relations.len())
            .sum(),
    }))
}

async fn project_replacement_proposals(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<ReplacementProposalListResponse>, ApiError> {
    if !state.is_primary() {
        return Ok(Json(
            proxy_get_json(
                &state,
                &format!("/v1/projects/{slug}/replacement-proposals"),
            )
            .await?,
        ));
    }
    Ok(Json(
        list_replacement_proposals(state.pool()?, &slug)
            .await
            .map_err(ApiError::sql)?,
    ))
}

async fn project_replacement_proposal_approve(
    State(state): State<AppState>,
    Path((slug, proposal_id)): Path<(String, Uuid)>,
    headers: HeaderMap,
) -> Result<Json<ReplacementProposalResolutionResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(
                &state,
                &format!("/v1/projects/{slug}/replacement-proposals/{proposal_id}/approve"),
                &serde_json::json!({}),
                true,
            )
            .await?,
        ));
    }
    let response = approve_replacement_proposal(state.pool()?, &slug, proposal_id)
        .await
        .map_err(ApiError::sql)?;
    if let Some(new_memory_id) = response.new_memory_id {
        notify_project_changed(
            &state,
            slug.clone(),
            Some(new_memory_id),
            ActivityKind::MemoryReplacement,
            format!(
                "Replaced memory \"{}\" with \"{}\" after review.",
                response.target_summary, response.candidate_summary
            ),
            Some(ActivityDetails::MemoryReplacement {
                old_memory_id: response.target_memory_id,
                old_summary: response.target_summary.clone(),
                new_memory_id,
                new_summary: response.candidate_summary.clone(),
                automatic: false,
                policy: response.policy,
            }),
        );
    }
    notify_project_refreshed(&state, slug.clone());
    Ok(Json(response))
}

async fn project_replacement_proposal_reject(
    State(state): State<AppState>,
    Path((slug, proposal_id)): Path<(String, Uuid)>,
    headers: HeaderMap,
) -> Result<Json<ReplacementProposalResolutionResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(
                &state,
                &format!("/v1/projects/{slug}/replacement-proposals/{proposal_id}/reject"),
                &serde_json::json!({}),
                true,
            )
            .await?,
        ));
    }
    let response = reject_replacement_proposal(state.pool()?, &slug, proposal_id)
        .await
        .map_err(ApiError::sql)?;
    notify_project_refreshed(&state, slug.clone());
    Ok(Json(response))
}

#[derive(Debug, Deserialize)]
struct ReplacementPolicyQuery {
    repo_root: Option<String>,
}

async fn project_replacement_policy(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(params): Query<ReplacementPolicyQuery>,
) -> Result<Json<ReplacementPolicyResponse>, ApiError> {
    if !state.is_primary() {
        return Ok(Json(
            proxy_get_json(&state, &format!("/v1/projects/{slug}/replacement-policy")).await?,
        ));
    }
    let repo_root = resolve_project_repo_root(&state, &slug, params.repo_root.as_deref());
    let replacement_policy = repo_root
        .as_deref()
        .and_then(|root| load_repo_replacement_policy(FsPath::new(root)).ok())
        .unwrap_or_default();
    Ok(Json(ReplacementPolicyResponse {
        project: slug,
        writable: repo_root.is_some(),
        repo_root,
        replacement_policy,
    }))
}

async fn project_replacement_policy_update(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
    Json(request): Json<ReplacementPolicyRequest>,
) -> Result<Json<ReplacementPolicyResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(
                &state,
                &format!("/v1/projects/{slug}/replacement-policy"),
                &request,
                true,
            )
            .await?,
        ));
    }
    let repo_root = request
        .repo_root
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::validation(ValidationError::new("repo_root must be non-empty")))?;
    write_replacement_policy(FsPath::new(repo_root), request.replacement_policy)
        .map_err(ApiError::io)?;
    notify_project_refreshed(&state, slug.clone());
    Ok(Json(ReplacementPolicyResponse {
        project: slug,
        repo_root: Some(repo_root.to_string()),
        replacement_policy: request.replacement_policy,
        writable: true,
    }))
}

fn resolve_project_repo_root(
    state: &AppState,
    project: &str,
    requested: Option<&str>,
) -> Option<String> {
    if let Some(repo_root) = requested.map(str::trim).filter(|value| !value.is_empty()) {
        return Some(repo_root.to_string());
    }
    if let Some(repo_root) = state
        .config
        .automation
        .repo_root
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(repo_root.to_string());
    }

    let mut repo_roots = state
        .watchers
        .lock()
        .expect("watcher registry mutex poisoned")
        .values()
        .filter(|watcher| watcher.project == project)
        .map(|watcher| watcher.repo_root.clone())
        .collect::<Vec<_>>();
    repo_roots.sort();
    repo_roots.dedup();
    if repo_roots.len() == 1 {
        repo_roots.pop()
    } else {
        None
    }
}

fn write_replacement_policy(repo_root: &FsPath, policy: ReplacementPolicy) -> Result<()> {
    let path = repo_agent_settings_path(repo_root);
    let mut document = if path.exists() {
        std::fs::read_to_string(&path)
            .with_context(|| format!("read {}", path.display()))?
            .parse::<toml_edit::DocumentMut>()
            .with_context(|| format!("parse {}", path.display()))?
    } else {
        toml_edit::DocumentMut::new()
    };
    document["curation"]["replacement_policy"] = toml_edit::value(policy.to_string());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    std::fs::write(&path, document.to_string())
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

async fn project_commits(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(params): Query<ProjectCommitsParams>,
) -> Result<Json<ProjectCommitsResponse>, ApiError> {
    if !state.is_primary() {
        let path = format!(
            "/v1/projects/{slug}/commits?limit={}&offset={}",
            params.limit.unwrap_or(50).clamp(1, 500),
            params.offset.unwrap_or(0).max(0)
        );
        return Ok(Json(proxy_get_json(&state, &path).await?));
    }
    let limit = params.limit.unwrap_or(50).clamp(1, 500);
    let offset = params.offset.unwrap_or(0).max(0);
    Ok(Json(
        fetch_project_commits(state.pool()?, &slug, limit, offset)
            .await
            .map_err(ApiError::sql)?,
    ))
}

async fn project_commit_detail(
    State(state): State<AppState>,
    Path((slug, hash)): Path<(String, String)>,
) -> Result<Json<CommitDetailResponse>, ApiError> {
    if !state.is_primary() {
        return Ok(Json(
            proxy_get_json(&state, &format!("/v1/projects/{slug}/commits/{hash}")).await?,
        ));
    }
    let commit = fetch_project_commit(state.pool()?, &slug, &hash)
        .await
        .map_err(ApiError::sql)?
        .ok_or_else(|| ApiError::not_found("project commit not found"))?;
    Ok(Json(CommitDetailResponse {
        project: slug,
        commit,
    }))
}

async fn project_resume(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Json(mut request): Json<ResumeRequest>,
) -> Result<Json<ResumeResponse>, ApiError> {
    request.validate().map_err(ApiError::validation)?;
    if request.project != slug {
        return Err(ApiError::validation(ValidationError::new(
            "request project must match path slug",
        )));
    }
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(
                &state,
                &format!("/v1/projects/{slug}/resume"),
                &request,
                false,
            )
            .await?,
        ));
    }

    if request.checkpoint.is_none() {
        request.checkpoint = request.repo_root.as_deref().and_then(|root| {
            load_resume_checkpoint(&slug, FsPath::new(root))
                .ok()
                .flatten()
        });
    }

    let pool = state.pool()?;
    let since = request
        .checkpoint
        .as_ref()
        .map(|checkpoint| checkpoint.marked_at)
        .or(request.since);
    let overview_fut = fetch_project_overview_with_watchers(&state, &slug);
    let timeline_fut = fetch_project_timeline(pool, &slug, since, request.limit);
    let commits_fut = fetch_project_commits_since(pool, &slug, since, request.limit);
    let changed_memories_fut = fetch_recent_project_memories(pool, &slug, since, request.limit);
    let durable_context_fut = fetch_durable_resume_context(pool, &slug, request.limit.min(8));
    let active_plan_fut = fetch_latest_active_plan_memory(pool, &slug);
    let (overview, timeline, commits, changed_memories, durable_context, active_plan) =
        tokio::try_join!(
            overview_fut,
            timeline_fut,
            commits_fut,
            changed_memories_fut,
            durable_context_fut,
            active_plan_fut,
        )
        .map_err(ApiError::sql)?;
    let warnings = resume_warnings(&overview);
    let actions = resume_actions(
        &slug,
        request.checkpoint.as_ref(),
        &overview,
        &timeline,
        &changed_memories,
    );
    let current_thread = infer_current_thread(
        request.checkpoint.as_ref(),
        &overview,
        &timeline,
        &commits,
        &changed_memories,
        active_plan.as_ref(),
    );
    let change_summary = build_change_summary(&timeline, &commits, &changed_memories);
    let attention_items = build_attention_items(&overview, &timeline);
    let context_items =
        select_resume_context(&changed_memories, &durable_context, active_plan.as_ref());
    let primary_next_step = actions.first().cloned();
    let secondary_next_steps = actions.iter().skip(1).take(2).cloned().collect::<Vec<_>>();
    let deterministic = build_resume_briefing(
        &slug,
        request.checkpoint.as_ref(),
        current_thread.as_deref(),
        &change_summary,
        &attention_items,
        primary_next_step.as_ref(),
        &secondary_next_steps,
        &context_items,
    );
    let briefing = if request.include_llm_summary {
        summarize_resume_with_llm(&state, &slug, &deterministic)
            .await
            .unwrap_or(deterministic)
    } else {
        deterministic
    };

    Ok(Json(ResumeResponse {
        project: slug,
        generated_at: chrono::Utc::now(),
        checkpoint: request.checkpoint,
        briefing,
        current_thread,
        change_summary,
        attention_items,
        primary_next_step,
        secondary_next_steps,
        context_items,
        timeline,
        commits,
        changed_memories,
        durable_context,
        warnings,
        actions,
        overview,
    }))
}

#[derive(Debug, Deserialize)]
struct ActivityListQuery {
    limit: Option<usize>,
    kind: Option<String>,
    since: Option<chrono::DateTime<chrono::Utc>>,
    before: Option<chrono::DateTime<chrono::Utc>>,
    include_details: Option<bool>,
}

async fn project_activities(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<ActivityListQuery>,
) -> Result<Json<ActivityListResponse>, ApiError> {
    if !state.is_primary() {
        let mut path = format!("/v1/projects/{slug}/activities");
        let mut params = Vec::new();
        if let Some(limit) = query.limit {
            params.push(format!("limit={limit}"));
        }
        if let Some(kind) = &query.kind {
            params.push(format!("kind={kind}"));
        }
        if let Some(since) = query.since {
            params.push(format!("since={}", since.to_rfc3339()));
        }
        if let Some(before) = query.before {
            params.push(format!("before={}", before.to_rfc3339()));
        }
        if let Some(include_details) = query.include_details {
            params.push(format!("include_details={include_details}"));
        }
        if !params.is_empty() {
            path.push('?');
            path.push_str(&params.join("&"));
        }
        return Ok(Json(proxy_get_json(&state, &path).await?));
    }
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let mut items = fetch_project_activities(
        state.pool()?,
        &slug,
        query.since,
        query.before,
        query.kind.as_deref(),
        limit,
        query.include_details.unwrap_or(true),
    )
    .await
    .map_err(ApiError::sql)?;
    if !query.include_details.unwrap_or(true) {
        for item in &mut items {
            item.details = None;
        }
    }
    Ok(Json(ActivityListResponse {
        project: slug,
        total_returned: items.len(),
        items,
    }))
}

async fn project_up_to_speed(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Json(request): Json<UpToSpeedRequest>,
) -> Result<Json<UpToSpeedResponse>, ApiError> {
    request.validate().map_err(ApiError::validation)?;
    if request.project != slug {
        return Err(ApiError::validation(ValidationError::new(
            "request project must match path slug",
        )));
    }
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(
                &state,
                &format!("/v1/projects/{slug}/up-to-speed"),
                &request,
                false,
            )
            .await?,
        ));
    }
    let response = build_up_to_speed_response(&state, &slug, &request).await?;
    notify_project_changed(
        &state,
        slug,
        None,
        ActivityKind::Briefing,
        "Generated get-up-to-speed briefing.".to_string(),
        None,
    );
    Ok(Json(response))
}

async fn build_up_to_speed_response(
    state: &AppState,
    slug: &str,
    request: &UpToSpeedRequest,
) -> Result<UpToSpeedResponse, ApiError> {
    let pool = state.pool()?;
    let limit = request.limit.clamp(1, 50);
    let overview_fut = fetch_project_overview_with_watchers(state, slug);
    let activities_fut = fetch_project_activities(pool, slug, None, None, None, limit, true);
    let commits_fut = fetch_project_commits_since(pool, slug, None, 8);
    let durable_context_fut = fetch_durable_resume_context(pool, slug, 8);
    let active_plan_fut = fetch_latest_active_plan_memory(pool, slug);
    let (overview, all_activities, commits, durable_context, active_plan) = tokio::try_join!(
        overview_fut,
        activities_fut,
        commits_fut,
        durable_context_fut,
        active_plan_fut,
    )
    .map_err(ApiError::sql)?;
    let recent_activities = all_activities
        .into_iter()
        .filter(|event| !matches!(event.kind, ActivityKind::Briefing))
        .collect::<Vec<_>>();
    let changed_memories = fetch_recent_project_memories(pool, slug, None, 8)
        .await
        .map_err(ApiError::sql)?;
    let warnings = resume_warnings(&overview);
    let next_actions = resume_actions(slug, None, &overview, &recent_activities, &changed_memories);
    let recent_work = build_change_summary(&recent_activities, &commits, &changed_memories);
    let blockers = build_attention_items(&overview, &recent_activities);
    let useful_memories =
        select_resume_context(&changed_memories, &durable_context, active_plan.as_ref());
    let current_focus = infer_current_thread(
        None,
        &overview,
        &recent_activities,
        &commits,
        &changed_memories,
        active_plan.as_ref(),
    )
    .into_iter()
    .collect::<Vec<_>>();
    let token_usage = summarize_activity_tokens(&recent_activities);
    let deterministic = build_up_to_speed_briefing(
        slug,
        &current_focus,
        &recent_work,
        &blockers,
        &next_actions,
        &useful_memories,
        &token_usage,
    );
    let briefing = if request.include_llm_summary {
        summarize_resume_with_llm(state, slug, &deterministic)
            .await
            .unwrap_or(deterministic)
    } else {
        deterministic
    };
    Ok(UpToSpeedResponse {
        project: slug.to_string(),
        generated_at: chrono::Utc::now(),
        briefing,
        current_focus,
        recent_work,
        blockers,
        next_actions,
        useful_memories,
        recent_activities,
        token_usage,
        warnings,
    })
}

#[derive(Debug, Deserialize, Default)]
struct StoredResumeCheckpoints {
    #[serde(default)]
    checkpoints: BTreeMap<String, ResumeCheckpoint>,
}

fn load_resume_checkpoint(project: &str, repo_root: &FsPath) -> Result<Option<ResumeCheckpoint>> {
    let state_dir = mem_platform::preferred_user_state_dir()
        .ok_or_else(|| anyhow::anyhow!("could not determine user state directory"))?;
    let path = state_dir.join("resume-checkpoints.json");
    if !path.exists() {
        return Ok(None);
    }
    let contents =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let store: StoredResumeCheckpoints =
        serde_json::from_str(&contents).context("parse checkpoint store")?;
    Ok(store
        .checkpoints
        .get(&format!("{}::{}", project, repo_root.display()))
        .cloned())
}

async fn fetch_project_timeline(
    pool: &PgPool,
    slug: &str,
    since: Option<chrono::DateTime<chrono::Utc>>,
    limit: usize,
) -> Result<Vec<ActivityEvent>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT te.id, te.recorded_at, p.slug AS project, te.kind, te.memory_id, te.summary, te.details_json,
               te.actor_id, te.actor_name, te.source, te.operation_id, te.duration_ms, te.provider, te.model,
               te.input_tokens, te.output_tokens, te.cache_read_tokens, te.cache_write_tokens, te.total_tokens
        FROM project_timeline_events te
        JOIN projects p ON p.id = te.project_id
        WHERE p.slug = $1
          AND ($2::timestamptz IS NULL OR te.recorded_at >= $2)
        ORDER BY te.recorded_at DESC
        LIMIT $3
        "#,
    )
    .bind(slug)
    .bind(since)
    .bind(limit as i64)
    .fetch_all(pool)
    .await?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let kind: String = row.try_get("kind")?;
        let details = row
            .try_get::<Option<sqlx::types::Json<ActivityDetails>>, _>("details_json")?
            .map(|payload| payload.0);
        items.push(ActivityEvent {
            id: row.try_get("id")?,
            recorded_at: row.try_get("recorded_at")?,
            project: row.try_get("project")?,
            kind: parse_activity_kind(&kind),
            memory_id: row.try_get("memory_id")?,
            summary: row.try_get("summary")?,
            details,
            actor_id: row.try_get("actor_id")?,
            actor_name: row.try_get("actor_name")?,
            source: row.try_get("source")?,
            operation_id: row.try_get("operation_id")?,
            duration_ms: row
                .try_get::<Option<i64>, _>("duration_ms")?
                .map(|value| value as u64),
            provider: row.try_get("provider")?,
            model: row.try_get("model")?,
            token_usage: token_usage_from_row(&row)?,
        });
    }
    Ok(items)
}

async fn fetch_project_activities(
    pool: &PgPool,
    slug: &str,
    since: Option<chrono::DateTime<chrono::Utc>>,
    before: Option<chrono::DateTime<chrono::Utc>>,
    kind: Option<&str>,
    limit: usize,
    include_details: bool,
) -> Result<Vec<ActivityEvent>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT te.id, te.recorded_at, p.slug AS project, te.kind, te.memory_id, te.summary,
               CASE WHEN $6 THEN te.details_json ELSE NULL END AS details_json,
               te.actor_id, te.actor_name, te.source, te.operation_id, te.duration_ms, te.provider, te.model,
               te.input_tokens, te.output_tokens, te.cache_read_tokens, te.cache_write_tokens, te.total_tokens
        FROM project_timeline_events te
        JOIN projects p ON p.id = te.project_id
        WHERE p.slug = $1
          AND ($2::timestamptz IS NULL OR te.recorded_at >= $2)
          AND ($3::timestamptz IS NULL OR te.recorded_at < $3)
          AND ($4::text IS NULL OR te.kind = $4)
        ORDER BY te.recorded_at DESC
        LIMIT $5
        "#,
    )
    .bind(slug)
    .bind(since)
    .bind(before)
    .bind(kind)
    .bind(limit as i64)
    .bind(include_details)
    .fetch_all(pool)
    .await?;
    activity_events_from_rows(rows)
}

fn activity_events_from_rows(
    rows: Vec<sqlx::postgres::PgRow>,
) -> Result<Vec<ActivityEvent>, sqlx::Error> {
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let kind: String = row.try_get("kind")?;
        let details = row
            .try_get::<Option<sqlx::types::Json<ActivityDetails>>, _>("details_json")?
            .map(|payload| payload.0);
        items.push(ActivityEvent {
            id: row.try_get("id")?,
            recorded_at: row.try_get("recorded_at")?,
            project: row.try_get("project")?,
            kind: parse_activity_kind(&kind),
            memory_id: row.try_get("memory_id")?,
            summary: row.try_get("summary")?,
            details,
            actor_id: row.try_get("actor_id")?,
            actor_name: row.try_get("actor_name")?,
            source: row.try_get("source")?,
            operation_id: row.try_get("operation_id")?,
            duration_ms: row
                .try_get::<Option<i64>, _>("duration_ms")?
                .map(|value| value as u64),
            provider: row.try_get("provider")?,
            model: row.try_get("model")?,
            token_usage: token_usage_from_row(&row)?,
        });
    }
    Ok(items)
}

fn token_usage_from_row(row: &sqlx::postgres::PgRow) -> Result<Option<TokenUsage>, sqlx::Error> {
    let input_tokens = row
        .try_get::<Option<i64>, _>("input_tokens")?
        .unwrap_or_default() as u64;
    let output_tokens = row
        .try_get::<Option<i64>, _>("output_tokens")?
        .unwrap_or_default() as u64;
    let cache_read_tokens = row
        .try_get::<Option<i64>, _>("cache_read_tokens")?
        .unwrap_or_default() as u64;
    let cache_write_tokens = row
        .try_get::<Option<i64>, _>("cache_write_tokens")?
        .unwrap_or_default() as u64;
    let total_tokens = row
        .try_get::<Option<i64>, _>("total_tokens")?
        .unwrap_or_default() as u64;
    if input_tokens == 0
        && output_tokens == 0
        && cache_read_tokens == 0
        && cache_write_tokens == 0
        && total_tokens == 0
    {
        return Ok(None);
    }
    Ok(Some(TokenUsage {
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        total_tokens,
    }))
}

async fn fetch_project_commits_since(
    pool: &PgPool,
    slug: &str,
    since: Option<chrono::DateTime<chrono::Utc>>,
    limit: usize,
) -> Result<Vec<mem_api::CommitRecord>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT pc.commit_hash, pc.short_hash, pc.subject, pc.body, pc.author_name, pc.author_email,
               pc.committed_at, pc.parent_hashes, pc.changed_paths, pc.imported_at
        FROM project_commits pc
        JOIN projects p ON p.id = pc.project_id
        WHERE p.slug = $1
          AND ($2::timestamptz IS NULL OR pc.imported_at >= $2 OR pc.committed_at >= $2)
        ORDER BY pc.committed_at DESC
        LIMIT $3
        "#,
    )
    .bind(slug)
    .bind(since)
    .bind(limit as i64)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(mem_service::row_to_commit_record)
        .collect()
}

async fn fetch_recent_project_memories(
    pool: &PgPool,
    slug: &str,
    since: Option<chrono::DateTime<chrono::Utc>>,
    limit: usize,
) -> Result<Vec<mem_api::ProjectMemoryListItem>, sqlx::Error> {
    let response = fetch_project_memories(pool, slug, None, limit as i64, 0).await?;
    Ok(response
        .items
        .into_iter()
        .filter(|item| since.is_none_or(|cutoff| item.updated_at >= cutoff))
        .collect())
}

async fn fetch_durable_resume_context(
    pool: &PgPool,
    slug: &str,
    limit: usize,
) -> Result<Vec<mem_api::ProjectMemoryListItem>, sqlx::Error> {
    let response = fetch_project_memories(pool, slug, Some("active"), 200, 0).await?;
    let mut items = response.items;
    items.sort_by(|left, right| {
        right
            .importance
            .cmp(&left.importance)
            .then_with(|| {
                right
                    .confidence
                    .partial_cmp(&left.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| right.updated_at.cmp(&left.updated_at))
    });
    items.retain(|item| {
        matches!(
            item.memory_type,
            mem_api::MemoryType::Architecture
                | mem_api::MemoryType::Convention
                | mem_api::MemoryType::Environment
        )
    });
    items.truncate(limit);
    Ok(items)
}

async fn fetch_latest_active_plan_memory(
    pool: &PgPool,
    slug: &str,
) -> Result<Option<mem_api::ProjectMemoryListItem>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT
            m.id,
            m.summary,
            left(m.canonical_text, 240) AS preview,
            m.memory_type,
            m.status,
            m.confidence,
            m.importance,
            m.updated_at,
            m.canonical_id,
            m.version_no,
            m.is_tombstone,
            COALESCE((
                SELECT ARRAY_AGG(mt.tag ORDER BY mt.tag)
                FROM memory_tags mt
                WHERE mt.memory_entry_id = m.id
            ), ARRAY[]::text[]) AS tags,
            COALESCE((
                SELECT COUNT(*)
                FROM memory_tags mt
                WHERE mt.memory_entry_id = m.id
            ), 0) AS tag_count,
            COALESCE((
                SELECT COUNT(*)
                FROM memory_sources ms
                WHERE ms.memory_entry_id = m.id
            ), 0) AS source_count
        FROM memory_entries m
        JOIN projects p ON p.id = m.project_id
        WHERE p.slug = $1
          AND m.status = 'active'
          AND m.memory_type = 'plan'
          AND m.is_tombstone = FALSE
          AND m.version_no = (
              SELECT MAX(m2.version_no)
              FROM memory_entries m2
              WHERE m2.canonical_id = m.canonical_id
          )
        ORDER BY m.updated_at DESC, m.id DESC
        LIMIT 1
        "#,
    )
    .bind(slug)
    .fetch_optional(pool)
    .await?;

    row.map(|row| {
        Ok(mem_api::ProjectMemoryListItem {
            id: row.try_get("id")?,
            summary: row.try_get("summary")?,
            preview: row.try_get("preview")?,
            memory_type: mem_search::parse_memory_type(&row.try_get::<String, _>("memory_type")?),
            status: match row.try_get::<String, _>("status")?.as_str() {
                "archived" => mem_api::MemoryStatus::Archived,
                _ => mem_api::MemoryStatus::Active,
            },
            confidence: row.try_get("confidence")?,
            importance: row.try_get("importance")?,
            updated_at: row.try_get("updated_at")?,
            tags: row.try_get("tags")?,
            tag_count: row.try_get("tag_count")?,
            source_count: row.try_get("source_count")?,
            canonical_id: row.try_get("canonical_id")?,
            version_no: row.try_get("version_no")?,
            is_tombstone: row.try_get("is_tombstone")?,
        })
    })
    .transpose()
}

fn resume_warnings(overview: &ProjectOverviewResponse) -> Vec<String> {
    let mut warnings = Vec::new();
    if overview.uncurated_raw_captures > 0 {
        warnings.push(format!(
            "{} raw capture(s) still need curation.",
            overview.uncurated_raw_captures
        ));
    }
    if overview
        .watchers
        .as_ref()
        .is_some_and(|watchers| watchers.unhealthy_count > 0)
    {
        let unhealthy = overview
            .watchers
            .as_ref()
            .map(|w| w.unhealthy_count)
            .unwrap_or(0);
        warnings.push(format!("{unhealthy} watcher(s) are unhealthy."));
    }
    if overview.missing_embedding_chunks > 0 {
        warnings.push(format!(
            "{} chunk(s) are missing active-space embeddings.",
            overview.missing_embedding_chunks
        ));
    }
    if overview.pending_replacement_proposals > 0 {
        warnings.push(format!(
            "{} memory update proposal(s) are waiting for review.",
            overview.pending_replacement_proposals
        ));
    }
    warnings
}

fn resume_actions(
    project: &str,
    checkpoint: Option<&mem_api::ResumeCheckpoint>,
    overview: &ProjectOverviewResponse,
    timeline: &[ActivityEvent],
    changed_memories: &[mem_api::ProjectMemoryListItem],
) -> Vec<ResumeAction> {
    let mut actions = Vec::new();
    let active_task_title = latest_capture_task_title(timeline);
    if overview.pending_replacement_proposals > 0 {
        actions.push(ResumeAction {
            title: "Review queued memory updates".to_string(),
            rationale: active_task_title
                .as_ref()
                .map(|task_title| {
                    format!(
                        "{} memory update proposal(s) from \"{}\" are waiting for review before outdated memories can be replaced.",
                        overview.pending_replacement_proposals, task_title
                    )
                })
                .unwrap_or_else(|| {
                    format!(
                        "{} memory update proposal(s) are waiting for review before outdated memories can be replaced.",
                        overview.pending_replacement_proposals
                    )
                }),
            command_hint: Some(format!("memory tui --project {project}")),
        });
    }
    if overview.uncurated_raw_captures > 0 {
        actions.push(ResumeAction {
            title: "Curate pending captures".to_string(),
            rationale: format!(
                "{} raw capture(s) are waiting to be curated into canonical memory.",
                overview.uncurated_raw_captures
            ),
            command_hint: Some(format!("memory curate --project {project}")),
        });
    }
    if overview
        .watchers
        .as_ref()
        .is_some_and(|watchers| watchers.unhealthy_count > 0)
    {
        actions.push(ResumeAction {
            title: "Inspect watcher health".to_string(),
            rationale: "At least one watcher is unhealthy or restarting.".to_string(),
            command_hint: Some(format!("memory watcher status --project {project}")),
        });
    }
    if timeline
        .iter()
        .any(|event| matches!(event.kind, ActivityKind::QueryError))
    {
        actions.push(ResumeAction {
            title: "Review recent failed queries".to_string(),
            rationale: "Recent agent or user queries failed and may indicate blockers.".to_string(),
            command_hint: Some(format!("memory tui --project {project}")),
        });
    }
    if !changed_memories.is_empty() {
        actions.push(ResumeAction {
            title: "Review changed memories".to_string(),
            rationale: format!(
                "{} memory entry/entries changed since the last checkpoint.",
                changed_memories.len()
            ),
            command_hint: Some(format!("memory resume --project {project}")),
        });
    }
    if let Some(note) = checkpoint.and_then(|checkpoint| checkpoint.note.as_deref()) {
        actions.push(ResumeAction {
            title: "Resume the last approved thread".to_string(),
            rationale: format!("Your last checkpoint note was: {note}"),
            command_hint: Some(format!("memory resume --project {project}")),
        });
    }
    if actions.is_empty() {
        actions.push(ResumeAction {
            title: "Ask the next scoped question".to_string(),
            rationale: "The project looks stable; use the resume pack as the launch point for your next task.".to_string(),
            command_hint: Some(format!("memory query --project {project} --question \"What should I work on next?\"")),
        });
    }
    actions
}

fn infer_current_thread(
    checkpoint: Option<&mem_api::ResumeCheckpoint>,
    overview: &ProjectOverviewResponse,
    timeline: &[ActivityEvent],
    commits: &[mem_api::CommitRecord],
    changed_memories: &[mem_api::ProjectMemoryListItem],
    active_plan: Option<&mem_api::ProjectMemoryListItem>,
) -> Option<String> {
    if let Some(plan) = active_plan {
        if overview.pending_replacement_proposals > 0 {
            return Some(format!(
                "Approved plan in execution: {}. Curation left {} queued memory update proposal(s) to review.",
                plan.summary, overview.pending_replacement_proposals
            ));
        }
        if overview.uncurated_raw_captures > 0 {
            return Some(format!(
                "Approved plan in execution: {}. {} raw capture(s) are still waiting to be curated.",
                plan.summary, overview.uncurated_raw_captures
            ));
        }
        return Some(format!("Approved plan in execution: {}.", plan.summary));
    }

    let active_task_title = latest_capture_task_title(timeline);
    if overview.pending_replacement_proposals > 0 {
        return Some(
            active_task_title
                .as_ref()
                .map(|task_title| {
                    format!(
                        "Recent work focused on {}. Curation left {} queued memory update proposal(s) to review.",
                        task_title, overview.pending_replacement_proposals
                    )
                })
                .unwrap_or_else(|| {
                    format!(
                        "Recent curation surfaced {} queued memory update proposal(s) that still need review.",
                        overview.pending_replacement_proposals
                    )
                }),
        );
    }
    if overview.uncurated_raw_captures > 0 {
        return Some(
            active_task_title
                .as_ref()
                .map(|task_title| {
                    format!(
                        "Recent work focused on {}. {} raw capture(s) are still waiting to be curated.",
                        task_title, overview.uncurated_raw_captures
                    )
                })
                .unwrap_or_else(|| {
                    format!(
                        "{} raw capture(s) are waiting to be curated into canonical memory.",
                        overview.uncurated_raw_captures
                    )
                }),
        );
    }
    if let Some(task_title) = active_task_title {
        return Some(format!("Recent work focused on {}.", task_title));
    }
    if let Some(event) = timeline
        .iter()
        .find(|event| !matches!(event.kind, ActivityKind::Checkpoint))
    {
        let thread = match event.kind {
            ActivityKind::Scan => {
                "Recent work focused on refreshing project memory from a repo scan."
            }
            ActivityKind::Plan => {
                "Recent work focused on an approved execution plan for the current task."
            }
            ActivityKind::Curate => {
                "Recent work focused on curating new captures into canonical memory."
            }
            ActivityKind::CaptureTask => {
                "Recent work captured fresh project evidence that may need follow-up."
            }
            ActivityKind::MemoryReplacement => {
                "Recent work replaced outdated memory with a newer canonical version."
            }
            ActivityKind::Reindex => "Recent work rebuilt the project's searchable chunk index.",
            ActivityKind::Reembed => {
                "Recent work refreshed the active embedding space for semantic retrieval."
            }
            ActivityKind::GraphExtract => {
                "Recent work refreshed the parser-backed code graph for graph-aware retrieval."
            }
            ActivityKind::CommitSync => "Recent work synced stored commit history for the project.",
            ActivityKind::Query | ActivityKind::QueryError => {
                "Recent work centered on answering or debugging project questions."
            }
            ActivityKind::WatcherHealth => {
                "Recent work involved watcher health and background automation recovery."
            }
            ActivityKind::BundleImport | ActivityKind::BundleExport => {
                "Recent work focused on importing or exporting shareable memory bundles."
            }
            ActivityKind::Archive | ActivityKind::DeleteMemory => {
                "Recent work changed the active memory set for the project."
            }
            ActivityKind::Briefing => "Recent work generated a get-up-to-speed briefing.",
            ActivityKind::Checkpoint => "",
        };
        if !thread.is_empty() {
            return Some(format!(
                "{thread} Latest event: {}",
                event.summary.trim_end_matches('.')
            ));
        }
    }
    if let Some(commit) = commits.first() {
        return Some(format!(
            "Recent work landed in git, most recently `{}` ({})",
            commit.subject, commit.short_hash
        ));
    }
    if let Some(memory) = changed_memories.first() {
        return Some(format!(
            "Recent work changed project memory, including: {}",
            memory.summary
        ));
    }
    checkpoint
        .and_then(|checkpoint| checkpoint.note.as_ref())
        .map(|note| format!("The last explicit work checkpoint was: {note}"))
}

fn build_change_summary(
    timeline: &[ActivityEvent],
    commits: &[mem_api::CommitRecord],
    changed_memories: &[mem_api::ProjectMemoryListItem],
) -> Vec<String> {
    let mut items = Vec::new();
    let mut seen_titles = Vec::new();
    for event in timeline.iter().take(6) {
        if let Some(task_title) = extract_capture_task_title(event)
            && !seen_titles.contains(&task_title)
        {
            items.push(format!(
                "{} Worked on: {}",
                event.recorded_at.format("%m-%d %H:%M"),
                task_title
            ));
            seen_titles.push(task_title);
        }
    }
    if let Some(commit) = commits.first() {
        let changed_paths = if commit.changed_paths.is_empty() {
            "no path summary".to_string()
        } else {
            commit
                .changed_paths
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        };
        items.push(format!(
            "Latest stored commit: {} ({}) touching {}",
            commit.subject, commit.short_hash, changed_paths
        ));
    }
    if items.is_empty() {
        for event in timeline
            .iter()
            .filter(|event| !matches!(event.kind, ActivityKind::Checkpoint | ActivityKind::Curate))
            .take(3)
        {
            let entry = format!(
                "{} {}",
                event.recorded_at.format("%m-%d %H:%M"),
                format_resume_event_summary(event)
            );
            if !items.contains(&entry) {
                items.push(entry);
            }
        }
    }
    if !changed_memories.is_empty() && items.is_empty() {
        let examples = changed_memories
            .iter()
            .take(2)
            .map(|item| item.summary.clone())
            .collect::<Vec<_>>()
            .join(" | ");
        items.push(format!(
            "{} memory update(s) landed, including: {}",
            changed_memories.len(),
            examples
        ));
    }
    items.truncate(5);
    items
}

fn latest_capture_task_title(timeline: &[ActivityEvent]) -> Option<String> {
    timeline.iter().find_map(extract_capture_task_title)
}

fn extract_capture_task_title(event: &ActivityEvent) -> Option<String> {
    match &event.details {
        Some(ActivityDetails::CaptureTask { task_title, .. }) => task_title
            .as_ref()
            .map(|title| title.trim().trim_end_matches('.').to_string())
            .filter(|title| !title.is_empty())
            .or_else(|| {
                event
                    .summary
                    .strip_prefix("Captured task: ")
                    .map(|title| title.trim().trim_end_matches('.').to_string())
                    .filter(|title| !title.is_empty())
            }),
        _ => event
            .summary
            .strip_prefix("Captured task: ")
            .map(|title| title.trim().trim_end_matches('.').to_string())
            .filter(|title| !title.is_empty()),
    }
}

fn format_resume_event_summary(event: &ActivityEvent) -> String {
    let base = match &event.details {
        Some(ActivityDetails::Plan { action, title, .. }) => {
            let prefix = match action {
                PlanActivityAction::Started => "Approved plan recorded",
                PlanActivityAction::Synced => "Approved plan synced",
                PlanActivityAction::FinishBlocked => "Plan completion blocked",
                PlanActivityAction::FinishVerified => "Plan completion verified",
            };
            format!("{prefix}: {}", title.trim())
        }
        Some(ActivityDetails::Query { query, .. }) => {
            format!("Query explored: {}", query.trim())
        }
        Some(ActivityDetails::Checkpoint { note, .. }) => note
            .as_ref()
            .map(|note| format!("Saved checkpoint: {note}"))
            .unwrap_or_else(|| event.summary.trim().to_string()),
        _ => match event.kind {
            ActivityKind::Query | ActivityKind::QueryError => {
                let query = event
                    .summary
                    .strip_prefix("Query: ")
                    .or_else(|| event.summary.strip_prefix("Query failed: "))
                    .unwrap_or(event.summary.as_str())
                    .trim();
                format!("Query explored: {query}")
            }
            ActivityKind::Briefing => "Generated a get-up-to-speed briefing".to_string(),
            _ => event.summary.trim().to_string(),
        },
    };
    clamp_resume_line(base.trim_end_matches('.'), 110)
}

fn clamp_resume_line(value: &str, limit: usize) -> String {
    let value = value.trim();
    if value.chars().count() <= limit {
        return value.to_string();
    }
    let mut truncated = String::new();
    for ch in value.chars().take(limit.saturating_sub(1)) {
        truncated.push(ch);
    }
    truncated.push('…');
    truncated
}

fn build_attention_items(
    overview: &ProjectOverviewResponse,
    timeline: &[ActivityEvent],
) -> Vec<String> {
    let mut items = Vec::new();
    if overview.pending_replacement_proposals > 0 {
        items.push(format!(
            "{} memory update proposal(s) are waiting for review.",
            overview.pending_replacement_proposals
        ));
    }
    if overview.uncurated_raw_captures > 0 {
        items.push(format!(
            "{} raw capture(s) still need curation.",
            overview.uncurated_raw_captures
        ));
    }
    if overview
        .watchers
        .as_ref()
        .is_some_and(|watchers| watchers.unhealthy_count > 0)
    {
        let unhealthy = overview
            .watchers
            .as_ref()
            .map(|watchers| watchers.unhealthy_count)
            .unwrap_or(0);
        items.push(format!(
            "{unhealthy} watcher(s) are unhealthy or restarting."
        ));
    }
    if timeline
        .iter()
        .any(|event| matches!(event.kind, ActivityKind::QueryError))
    {
        items.push("Recent query errors may indicate an unresolved blocker.".to_string());
    }
    let embedding_work_active = timeline.iter().any(|event| {
        matches!(
            event.kind,
            ActivityKind::Scan
                | ActivityKind::GraphExtract
                | ActivityKind::Reembed
                | ActivityKind::Reindex
        )
    });
    if overview.missing_embedding_chunks > 0 && (embedding_work_active || items.is_empty()) {
        items.push(format!(
            "{} chunk(s) are missing active-space embeddings.",
            overview.missing_embedding_chunks
        ));
    }
    items
}

fn select_resume_context(
    changed_memories: &[mem_api::ProjectMemoryListItem],
    durable_context: &[mem_api::ProjectMemoryListItem],
    active_plan: Option<&mem_api::ProjectMemoryListItem>,
) -> Vec<mem_api::ProjectMemoryListItem> {
    let mut selected = Vec::new();

    if let Some(plan) = active_plan {
        selected.push(plan.clone());
    }

    if let Some(item) = changed_memories.iter().find(|item| {
        matches!(
            item.memory_type,
            mem_api::MemoryType::Plan
                | mem_api::MemoryType::Decision
                | mem_api::MemoryType::Architecture
                | mem_api::MemoryType::Convention
                | mem_api::MemoryType::Debugging
        ) && !selected.iter().any(|existing| existing.id == item.id)
    }) {
        selected.push(item.clone());
    } else if let Some(item) = changed_memories
        .iter()
        .find(|item| !selected.iter().any(|existing| existing.id == item.id))
    {
        selected.push(item.clone());
    }

    if let Some(item) = durable_context.iter().find(|item| {
        matches!(
            item.memory_type,
            mem_api::MemoryType::Decision
                | mem_api::MemoryType::Architecture
                | mem_api::MemoryType::Convention
                | mem_api::MemoryType::Environment
        ) && !selected.iter().any(|existing| existing.id == item.id)
    }) {
        selected.push(item.clone());
    }

    if let Some(item) = durable_context
        .iter()
        .find(|item| !selected.iter().any(|existing| existing.id == item.id))
    {
        selected.push(item.clone());
    }

    selected.truncate(3);
    selected
}

#[allow(clippy::too_many_arguments)]
fn build_resume_briefing(
    project: &str,
    checkpoint: Option<&mem_api::ResumeCheckpoint>,
    current_thread: Option<&str>,
    change_summary: &[String],
    attention_items: &[String],
    primary_next_step: Option<&ResumeAction>,
    secondary_next_steps: &[ResumeAction],
    context_items: &[mem_api::ProjectMemoryListItem],
) -> String {
    let mut lines = Vec::new();
    lines.push(format!("Resume briefing for project `{project}`."));
    if let Some(checkpoint) = checkpoint {
        lines.push(format!(
            "Last checkpoint: {}.",
            checkpoint.marked_at.format("%Y-%m-%d %H:%M UTC")
        ));
        if let Some(note) = &checkpoint.note {
            lines.push(format!("Checkpoint note: {note}"));
        }
    } else {
        lines.push("No checkpoint is stored yet. This is a current-state briefing.".to_string());
    }
    if let Some(current_thread) = current_thread {
        lines.push(String::new());
        lines.push("Current thread:".to_string());
        lines.push(format!("- {current_thread}"));
    }
    if let Some(action) = primary_next_step {
        lines.push(String::new());
        lines.push("Next step:".to_string());
        lines.push(format!("- {}: {}", action.title, action.rationale));
        if let Some(command_hint) = &action.command_hint {
            lines.push(format!("  {command_hint}"));
        }
    }
    if !change_summary.is_empty() {
        lines.push(String::new());
        lines.push("What changed:".to_string());
        for item in change_summary.iter().take(5) {
            lines.push(format!("- {item}"));
        }
    }
    if !attention_items.is_empty() {
        lines.push(String::new());
        lines.push("Needs attention:".to_string());
        for item in attention_items.iter().take(4) {
            lines.push(format!("- {item}"));
        }
    }
    if !context_items.is_empty() {
        lines.push(String::new());
        lines.push("Keep in mind:".to_string());
        for item in context_items.iter().take(3) {
            lines.push(format!("- [{}] {}", item.memory_type, item.summary));
        }
    }
    if !secondary_next_steps.is_empty() {
        lines.push(String::new());
        lines.push("Other useful follow-ups:".to_string());
        for action in secondary_next_steps.iter().take(2) {
            lines.push(format!("- {}: {}", action.title, action.rationale));
        }
    }
    lines.join("\n")
}

fn summarize_activity_tokens(events: &[ActivityEvent]) -> TokenUsageSummary {
    let mut summary = TokenUsageSummary::default();
    for usage in events.iter().filter_map(|event| event.token_usage.as_ref()) {
        summary.action_count += 1;
        summary.total_input_tokens += usage.input_tokens;
        summary.total_output_tokens += usage.output_tokens;
        summary.total_cache_read_tokens += usage.cache_read_tokens;
        summary.total_cache_write_tokens += usage.cache_write_tokens;
        summary.total_tokens += usage.total_tokens;
    }
    summary
}

fn build_up_to_speed_briefing(
    project: &str,
    current_focus: &[String],
    recent_work: &[String],
    blockers: &[String],
    next_actions: &[ResumeAction],
    useful_memories: &[ProjectMemoryListItem],
    token_usage: &TokenUsageSummary,
) -> String {
    let mut lines = vec![format!("Get up to speed for `{project}`.")];
    if !current_focus.is_empty() {
        lines.push(String::new());
        lines.push("Current focus:".to_string());
        for item in current_focus {
            lines.push(format!("- {item}"));
        }
    }
    if !recent_work.is_empty() {
        lines.push(String::new());
        lines.push("Recent work:".to_string());
        for item in recent_work.iter().take(6) {
            lines.push(format!("- {item}"));
        }
    }
    if !blockers.is_empty() {
        lines.push(String::new());
        lines.push("Needs attention:".to_string());
        for item in blockers.iter().take(6) {
            lines.push(format!("- {item}"));
        }
    }
    if !useful_memories.is_empty() {
        lines.push(String::new());
        lines.push("Useful memories:".to_string());
        for item in useful_memories.iter().take(6) {
            lines.push(format!("- [{}] {}", item.memory_type, item.summary));
        }
    }
    if token_usage.action_count > 0 {
        lines.push(String::new());
        lines.push(format!(
            "Token usage across {} recent action(s): {} total ({} input, {} output, {} cache read, {} cache write).",
            token_usage.action_count,
            token_usage.total_tokens,
            token_usage.total_input_tokens,
            token_usage.total_output_tokens,
            token_usage.total_cache_read_tokens,
            token_usage.total_cache_write_tokens,
        ));
    }
    if !next_actions.is_empty() {
        lines.push(String::new());
        lines.push("Recommended next actions:".to_string());
        for action in next_actions.iter().take(3) {
            lines.push(format!("- {}: {}", action.title, action.rationale));
            if let Some(command_hint) = &action.command_hint {
                lines.push(format!("  {command_hint}"));
            }
        }
    }
    if lines.len() == 1 {
        lines.push(
            "No recent activity was found. Start with `memory query` or inspect the TUI."
                .to_string(),
        );
    }
    lines.join("\n")
}

async fn summarize_resume_with_llm(
    state: &AppState,
    project: &str,
    deterministic: &str,
) -> Result<String> {
    if state.config.llm.provider.trim() != "openai_compatible"
        || state.config.llm.model.trim().is_empty()
    {
        anyhow::bail!("llm summary is not configured");
    }
    let api_key = std::env::var(&state.config.llm.api_key_env)
        .context("read llm api key for resume summary")?;
    let url = format!(
        "{}/chat/completions",
        state.config.llm.base_url.trim_end_matches('/')
    );
    let request = serde_json::json!({
        "model": state.config.llm.model,
        "temperature": 0.0,
        "max_completion_tokens": 600,
        "messages": [
            {
                "role": "system",
                "content": "You write concise project resume briefings for returning developers. Summarize what changed, what still matters, and what to do next. Keep it factual and grounded in the provided project resume pack."
            },
            {
                "role": "user",
                "content": format!("Project: {project}\n\nResume pack:\n{deterministic}")
            }
        ]
    });
    let response = state
        .http_client
        .post(url)
        .bearer_auth(api_key)
        .json(&request)
        .send()
        .await
        .context("send llm resume summary request")?;
    let status = response.status();
    let body = response
        .text()
        .await
        .context("read llm resume summary body")?;
    if !status.is_success() {
        anyhow::bail!("llm resume summary failed: {status} {body}");
    }
    let payload: serde_json::Value =
        serde_json::from_str(&body).context("parse llm resume summary response")?;
    let content = payload
        .get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_str())
        .map(str::trim)
        .filter(|content| !content.is_empty())
        .ok_or_else(|| anyhow::anyhow!("llm resume summary missing content"))?;
    Ok(content.to_string())
}

async fn enrich_query_answer_with_llm(
    state: &AppState,
    request: &QueryRequest,
    response: &mut QueryResponse,
) {
    let started = std::time::Instant::now();
    let result = synthesize_query_answer_with_llm(state, request, response).await;
    match result {
        Ok(answer) => {
            response.answer = answer.answer;
            response.confidence = answer.confidence;
            response.insufficient_evidence = answer.insufficient_evidence;
            response.answer_citations = answer.citations;
            response.answer_generation = QueryAnswerGeneration {
                method: QueryAnswerMethod::Llm,
                cited_result_numbers: answer.cited_result_numbers,
                evidence_count: response.answer_citations.len(),
                duration_ms: started.elapsed().as_millis() as u64,
                fallback_reason: None,
                token_usage: answer.token_usage,
            };
        }
        Err(error) => {
            let cited_result_numbers = response
                .answer_citations
                .iter()
                .map(|citation| citation.result_number)
                .collect::<Vec<_>>();
            response.answer_generation = QueryAnswerGeneration {
                method: QueryAnswerMethod::Fallback,
                cited_result_numbers,
                evidence_count: response.answer_citations.len(),
                duration_ms: started.elapsed().as_millis() as u64,
                fallback_reason: Some(error.to_string()),
                token_usage: None,
            };
        }
    }
}

#[derive(Debug)]
struct LlmQueryAnswer {
    answer: String,
    confidence: f32,
    insufficient_evidence: bool,
    cited_result_numbers: Vec<usize>,
    citations: Vec<QueryAnswerCitation>,
    token_usage: Option<TokenUsage>,
}

#[derive(Debug, SerdeDeserialize)]
struct LlmQueryAnswerPayload {
    answer: String,
    #[serde(default)]
    confidence: f32,
    #[serde(default)]
    insufficient_evidence: bool,
    #[serde(default)]
    citations: Vec<usize>,
}

async fn synthesize_query_answer_with_llm(
    state: &AppState,
    request: &QueryRequest,
    response: &QueryResponse,
) -> Result<LlmQueryAnswer> {
    if response.results.is_empty() {
        anyhow::bail!("no query memories available for llm answer synthesis");
    }
    if state.config.llm.provider.trim() != "openai_compatible"
        || state.config.llm.model.trim().is_empty()
    {
        anyhow::bail!("llm query answer is not configured");
    }
    let api_key = std::env::var(&state.config.llm.api_key_env)
        .context("read llm api key for query answer")?;
    let url = format!(
        "{}/chat/completions",
        state.config.llm.base_url.trim_end_matches('/')
    );
    let request_body = serde_json::json!({
        "model": state.config.llm.model,
        "temperature": 0.0,
        "max_completion_tokens": state.config.llm.max_output_tokens.min(800),
        "messages": [
            {
                "role": "system",
                "content": "Answer project-memory questions using only the numbered memories supplied by the user. Return strict JSON with keys: answer (string), citations (array of result numbers), confidence (0..1), insufficient_evidence (boolean). Cite only memories that directly support the answer. If evidence is weak, say so and set insufficient_evidence true."
            },
            {
                "role": "user",
                "content": build_query_answer_prompt(request, response)
            }
        ]
    });
    let http_response = state
        .http_client
        .post(url)
        .bearer_auth(api_key)
        .json(&request_body)
        .send()
        .await
        .context("send llm query answer request")?;
    let status = http_response.status();
    let body = http_response
        .text()
        .await
        .context("read llm query answer body")?;
    if !status.is_success() {
        anyhow::bail!("llm query answer failed: {status} {body}");
    }
    let mut answer = parse_llm_query_answer_body(&body, response)?;
    answer.token_usage = token_usage_from_chat_body(&body);
    Ok(answer)
}

fn build_query_answer_prompt(request: &QueryRequest, response: &QueryResponse) -> String {
    let mut lines = vec![
        format!("Project: {}", request.project),
        format!("Question: {}", request.query),
        String::new(),
        "Returned memories:".to_string(),
    ];
    for (index, result) in response.results.iter().enumerate() {
        lines.push(format!(
            "[{}] type={} score={:.2} summary={}",
            index + 1,
            result.memory_type,
            result.score,
            result.summary
        ));
        lines.push(format!("snippet: {}", result.snippet));
        if !result.sources.is_empty() {
            let sources = result
                .sources
                .iter()
                .take(3)
                .map(|source| {
                    let mut parts = vec![source_kind_name(&source.source_kind).to_string()];
                    if let Some(path) = &source.file_path {
                        parts.push(path.clone());
                    }
                    if let Some(excerpt) = &source.excerpt {
                        parts.push(excerpt.clone());
                    }
                    parts.join(" | ")
                })
                .collect::<Vec<_>>()
                .join("; ");
            lines.push(format!("sources: {sources}"));
        }
        if !result.graph_connections.is_empty() {
            let graph_connections = result
                .graph_connections
                .iter()
                .take(3)
                .map(|connection| {
                    let mut parts = vec![connection.reason.clone(), connection.file_path.clone()];
                    if let Some(symbol) = &connection.symbol {
                        parts.push(format!("symbol={symbol}"));
                    }
                    if let Some(edge_kind) = &connection.edge_kind {
                        parts.push(format!("edge={edge_kind}"));
                    }
                    if let Some(neighbor) = &connection.neighbor_symbol {
                        parts.push(format!("neighbor={neighbor}"));
                    }
                    parts.push(format!("boost={:.2}", connection.score_boost));
                    parts.join(" | ")
                })
                .collect::<Vec<_>>()
                .join("; ");
            lines.push(format!("graph: {graph_connections}"));
        }
        lines.push(String::new());
    }
    lines.push(
        "Return JSON only, for example: {\"answer\":\"... [1]\",\"citations\":[1],\"confidence\":0.82,\"insufficient_evidence\":false}"
            .to_string(),
    );
    lines.join("\n")
}

fn parse_llm_query_answer_body(body: &str, response: &QueryResponse) -> Result<LlmQueryAnswer> {
    let payload: serde_json::Value =
        serde_json::from_str(body).context("parse llm query answer response")?;
    let content = payload
        .get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_str())
        .map(str::trim)
        .filter(|content| !content.is_empty())
        .ok_or_else(|| anyhow::anyhow!("llm query answer missing content"))?;
    parse_llm_query_answer_content(content, response)
}

fn parse_llm_query_answer_content(
    content: &str,
    response: &QueryResponse,
) -> Result<LlmQueryAnswer> {
    let json = content
        .strip_prefix("```json")
        .and_then(|value| value.strip_suffix("```"))
        .or_else(|| {
            content
                .strip_prefix("```")
                .and_then(|value| value.strip_suffix("```"))
        })
        .map(str::trim)
        .unwrap_or(content);
    let payload: LlmQueryAnswerPayload =
        serde_json::from_str(json).context("parse llm query answer content")?;
    let answer = payload.answer.trim();
    if answer.is_empty() {
        anyhow::bail!("llm query answer was empty");
    }
    let cited_result_numbers = validate_query_answer_citations(&payload.citations, response)?;
    let citations = citations_from_result_numbers(&cited_result_numbers, response);
    Ok(LlmQueryAnswer {
        answer: answer.to_string(),
        confidence: payload.confidence.clamp(0.0, 1.0),
        insufficient_evidence: payload.insufficient_evidence || citations.is_empty(),
        cited_result_numbers,
        citations,
        token_usage: None,
    })
}

fn token_usage_from_chat_body(body: &str) -> Option<TokenUsage> {
    let payload: serde_json::Value = serde_json::from_str(body).ok()?;
    let usage = payload.get("usage")?;
    let input_tokens = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(|value| value.as_u64())
        .unwrap_or_default();
    let output_tokens = usage
        .get("completion_tokens")
        .or_else(|| usage.get("output_tokens"))
        .and_then(|value| value.as_u64())
        .unwrap_or_default();
    let cache_read_tokens = usage
        .get("cache_read_input_tokens")
        .or_else(|| usage.get("cached_input_tokens"))
        .and_then(|value| value.as_u64())
        .unwrap_or_default();
    let cache_write_tokens = usage
        .get("cache_creation_input_tokens")
        .and_then(|value| value.as_u64())
        .unwrap_or_default();
    let total_tokens = usage
        .get("total_tokens")
        .and_then(|value| value.as_u64())
        .unwrap_or(input_tokens + output_tokens + cache_read_tokens + cache_write_tokens);
    if input_tokens == 0
        && output_tokens == 0
        && cache_read_tokens == 0
        && cache_write_tokens == 0
        && total_tokens == 0
    {
        return None;
    }
    Some(TokenUsage {
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        total_tokens,
    })
}

fn validate_query_answer_citations(
    citations: &[usize],
    response: &QueryResponse,
) -> Result<Vec<usize>> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for citation in citations {
        if *citation == 0 || *citation > response.results.len() {
            anyhow::bail!("llm query answer cited unavailable result {citation}");
        }
        if seen.insert(*citation) {
            result.push(*citation);
        }
    }
    Ok(result)
}

fn citations_from_result_numbers(
    cited_result_numbers: &[usize],
    response: &QueryResponse,
) -> Vec<QueryAnswerCitation> {
    cited_result_numbers
        .iter()
        .filter_map(|number| {
            let result = response.results.get(number.saturating_sub(1))?;
            Some(QueryAnswerCitation {
                result_number: *number,
                memory_id: result.memory_id,
                memory_type: result.memory_type.clone(),
                summary: result.summary.clone(),
                snippet: result.snippet.clone(),
            })
        })
        .collect()
}

fn source_kind_name(source_kind: &SourceKind) -> &'static str {
    match source_kind {
        SourceKind::TaskPrompt => "task_prompt",
        SourceKind::File => "file",
        SourceKind::GitCommit => "git_commit",
        SourceKind::CommandOutput => "command_output",
        SourceKind::Test => "test",
        SourceKind::Note => "note",
    }
}

fn parse_activity_kind(value: &str) -> ActivityKind {
    match value {
        "checkpoint" => ActivityKind::Checkpoint,
        "scan" => ActivityKind::Scan,
        "plan" => ActivityKind::Plan,
        "commit_sync" => ActivityKind::CommitSync,
        "bundle_export" => ActivityKind::BundleExport,
        "bundle_import" => ActivityKind::BundleImport,
        "graph_extract" => ActivityKind::GraphExtract,
        "query" => ActivityKind::Query,
        "query_error" => ActivityKind::QueryError,
        "watcher_health" => ActivityKind::WatcherHealth,
        "memory_replacement" => ActivityKind::MemoryReplacement,
        "capture_task" => ActivityKind::CaptureTask,
        "curate" => ActivityKind::Curate,
        "reindex" => ActivityKind::Reindex,
        "reembed" => ActivityKind::Reembed,
        "archive" => ActivityKind::Archive,
        "delete_memory" => ActivityKind::DeleteMemory,
        "briefing" => ActivityKind::Briefing,
        _ => ActivityKind::Query,
    }
}

fn watcher_health_label(health: &WatcherHealth) -> &'static str {
    match health {
        WatcherHealth::Healthy => "healthy",
        WatcherHealth::Stale => "stale",
        WatcherHealth::Restarting => "restarting",
        WatcherHealth::Failed => "failed",
    }
}

async fn watcher_heartbeat(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<WatcherHeartbeatRequest>,
) -> Result<Json<WatcherPresenceSummary>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        let project = request.project.clone();
        let (_, changed, transition) = register_watcher_heartbeat(&state.watchers, request.clone());
        if changed {
            notify_project_refreshed(&state, project);
        }
        if let Some((summary, details)) = transition {
            notify_project_changed(
                &state,
                request.project.clone(),
                None,
                ActivityKind::WatcherHealth,
                summary,
                Some(details),
            );
        }
        return Ok(Json(
            proxy_post_json(&state, "/v1/watchers/heartbeat", &request, true).await?,
        ));
    }
    let project = request.project.clone();
    let (summary, changed, transition) = register_watcher_heartbeat(&state.watchers, request);
    if changed {
        notify_project_refreshed(&state, project.clone());
    }
    if let Some((summary, details)) = transition {
        notify_project_changed(
            &state,
            project,
            None,
            ActivityKind::WatcherHealth,
            summary,
            Some(details),
        );
    }
    Ok(Json(summary))
}

async fn watcher_unregister(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<WatcherUnregisterRequest>,
) -> Result<Json<WatcherPresenceSummary>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        let project = request.project.clone();
        let (_, changed) = unregister_watcher(&state.watchers, &request);
        if changed {
            notify_project_refreshed(&state, project);
        }
        return Ok(Json(
            proxy_post_json(&state, "/v1/watchers/unregister", &request, true).await?,
        ));
    }
    let project = request.project.clone();
    let (summary, changed) = unregister_watcher(&state.watchers, &request);
    if changed {
        notify_project_refreshed(&state, project);
    }
    Ok(Json(summary))
}

async fn watcher_restart_local(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<WatcherRestartRequest>,
) -> Result<Json<WatcherRestartResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if request.host_service_id != state.config.cluster.service_id {
        return Err(ApiError::status_message(
            StatusCode::BAD_REQUEST,
            "restart request was sent to the wrong host service",
        ));
    }

    restart_local_watcher_service_name(&local_watcher_restart_service_name(&request))
        .map_err(ApiError::io)?;
    update_local_watcher_restart_state(&state.watchers, &request.watcher_id);
    notify_project_refreshed(&state, request.project.clone());

    Ok(Json(WatcherRestartResponse {
        accepted: true,
        message: format!("requested restart for watcher {}", request.watcher_id),
    }))
}

async fn archive(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ArchiveRequest>,
) -> Result<Json<ArchiveResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/archive", &request, true).await?,
        ));
    }
    let project = request.project.clone();
    let archived_count = if request.dry_run {
        sqlx::query(
            r#"
            SELECT COUNT(*) AS count
            FROM memory_entries m
            JOIN projects p ON p.id = m.project_id
            WHERE p.slug = $1
              AND m.status = 'active'
              AND m.confidence <= $2
              AND m.importance <= $3
            "#,
        )
        .bind(&request.project)
        .bind(request.max_confidence)
        .bind(request.max_importance)
        .fetch_one(state.pool()?)
        .await
        .map_err(ApiError::sql)?
        .try_get::<i64, _>("count")
        .map_err(ApiError::sql)? as u64
    } else {
        sqlx::query(
            r#"
            UPDATE memory_entries m
            SET status = 'archived', archived_at = now(), updated_at = now()
            FROM projects p
            WHERE p.id = m.project_id
              AND p.slug = $1
              AND m.status = 'active'
              AND m.confidence <= $2
              AND m.importance <= $3
            "#,
        )
        .bind(&request.project)
        .bind(request.max_confidence)
        .bind(request.max_importance)
        .execute(state.pool()?)
        .await
        .map_err(ApiError::sql)?
        .rows_affected()
    };
    if request.dry_run {
        return Ok(Json(ArchiveResponse {
            archived_count,
            dry_run: true,
        }));
    }
    notify_project_changed(
        &state,
        project,
        None,
        ActivityKind::Archive,
        format!(
            "Archived {} low-value memory entry/entries.",
            archived_count
        ),
        Some(ActivityDetails::Archive {
            archived_count,
            max_confidence: request.max_confidence,
            max_importance: request.max_importance,
        }),
    );

    Ok(Json(ArchiveResponse {
        archived_count,
        dry_run: false,
    }))
}

async fn delete_memory(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<DeleteMemoryRequest>,
) -> Result<Json<DeleteMemoryResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_delete_json(&state, "/v1/memory", &request).await?,
        ));
    }

    // Memories are immutable. Delete writes a tombstone version — a row with
    // the same canonical_id but empty content and is_tombstone=TRUE. Default
    // searches skip it; history-aware queries can still surface the prior
    // versions so nothing is truly lost.
    let pool = state.pool()?;
    let mut tx = pool.begin().await.map_err(ApiError::sql)?;
    let target = sqlx::query(
        r#"
        SELECT m.id, m.project_id, p.slug, m.canonical_id, m.summary,
               (
                   SELECT MAX(m2.version_no)
                   FROM memory_entries m2
                   WHERE m2.canonical_id = m.canonical_id
               ) AS latest_version
        FROM memory_entries m
        JOIN projects p ON p.id = m.project_id
        WHERE m.id = $1
        "#,
    )
    .bind(request.memory_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(ApiError::sql)?
    .ok_or_else(|| ApiError::not_found("memory entry not found"))?;

    let project_id: Uuid = target.try_get("project_id").map_err(ApiError::sql)?;
    let project: String = target.try_get("slug").map_err(ApiError::sql)?;
    let canonical_id: Uuid = target.try_get("canonical_id").map_err(ApiError::sql)?;
    let latest_version: i32 = target.try_get("latest_version").map_err(ApiError::sql)?;
    let summary: String = target.try_get("summary").map_err(ApiError::sql)?;

    let tombstone_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO memory_entries
            (id, project_id, canonical_id, version_no, is_tombstone,
             canonical_text, summary, memory_type, scope, importance,
             confidence, status, created_at, updated_at, archived_at,
             search_document)
        VALUES
            ($1, $2, $3, $4, TRUE, '', '', 'implementation', 'project', 0, 0.0,
             'active', now(), now(), NULL, to_tsvector('english', ''))
        "#,
    )
    .bind(tombstone_id)
    .bind(project_id)
    .bind(canonical_id)
    .bind(latest_version + 1)
    .execute(&mut *tx)
    .await
    .map_err(ApiError::sql)?;
    tx.commit().await.map_err(ApiError::sql)?;

    let memory_id = tombstone_id;
    notify_project_changed(
        &state,
        project.clone(),
        Some(memory_id),
        ActivityKind::DeleteMemory,
        format!("Deleted memory: {summary}"),
        Some(ActivityDetails::DeleteMemory {
            deleted: true,
            summary: summary.clone(),
        }),
    );

    Ok(Json(DeleteMemoryResponse {
        memory_id,
        project,
        summary,
        deleted: true,
    }))
}

async fn prune_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<PruneHistoryRequest>,
) -> Result<Json<PruneHistoryResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    // Fill missing thresholds from server config so the caller can rely on
    // either source without duplicating the logic in every client.
    let tombstone_after = request
        .tombstone_after
        .or(state.config.retention.tombstone_after);
    let superseded_after = request
        .superseded_after
        .or(state.config.retention.superseded_after);
    let effective = PruneHistoryRequest {
        project: request.project.clone(),
        tombstone_after,
        superseded_after,
        dry_run: request.dry_run,
    };
    effective.validate().map_err(ApiError::validation)?;

    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/prune-history", &effective, true).await?,
        ));
    }

    let pool = state.pool()?;
    let mut tx = pool.begin().await.map_err(ApiError::sql)?;

    let project_filter: Option<String> = effective.project.clone();
    let dry_run = effective.dry_run;

    let mut canonicals_tombstoned_deleted: u64 = 0;
    if let Some(threshold) = effective.tombstone_after {
        let seconds = threshold.as_secs_f64();
        let count_sql = r#"
            WITH latest AS (
                SELECT DISTINCT ON (m.canonical_id)
                       m.canonical_id, m.updated_at, m.is_tombstone
                FROM memory_entries m
                JOIN projects p ON p.id = m.project_id
                WHERE ($1::text IS NULL OR p.slug = $1)
                ORDER BY m.canonical_id, m.version_no DESC
            )
            SELECT COUNT(*) AS count
            FROM latest
            WHERE is_tombstone = TRUE
              AND updated_at < now() - make_interval(secs => $2)
        "#;
        let count: i64 = sqlx::query(count_sql)
            .bind(project_filter.as_deref())
            .bind(seconds)
            .fetch_one(&mut *tx)
            .await
            .map_err(ApiError::sql)?
            .try_get("count")
            .map_err(ApiError::sql)?;
        canonicals_tombstoned_deleted = count.max(0) as u64;

        if !dry_run && canonicals_tombstoned_deleted > 0 {
            let delete_sql = r#"
                WITH latest AS (
                    SELECT DISTINCT ON (m.canonical_id)
                           m.canonical_id, m.updated_at, m.is_tombstone
                    FROM memory_entries m
                    JOIN projects p ON p.id = m.project_id
                    WHERE ($1::text IS NULL OR p.slug = $1)
                    ORDER BY m.canonical_id, m.version_no DESC
                ),
                dead AS (
                    SELECT canonical_id FROM latest
                    WHERE is_tombstone = TRUE
                      AND updated_at < now() - make_interval(secs => $2)
                )
                DELETE FROM memory_entries
                WHERE canonical_id IN (SELECT canonical_id FROM dead)
            "#;
            sqlx::query(delete_sql)
                .bind(project_filter.as_deref())
                .bind(seconds)
                .execute(&mut *tx)
                .await
                .map_err(ApiError::sql)?;
        }
    }

    let mut superseded_versions_pruned: u64 = 0;
    if let Some(threshold) = effective.superseded_after {
        let seconds = threshold.as_secs_f64();
        let count_sql = r#"
            SELECT COUNT(*) AS count
            FROM memory_entries m
            JOIN projects p ON p.id = m.project_id
            WHERE ($1::text IS NULL OR p.slug = $1)
              AND m.is_tombstone = FALSE
              AND m.updated_at < now() - make_interval(secs => $2)
              AND m.version_no < (
                  SELECT MAX(m2.version_no)
                  FROM memory_entries m2
                  WHERE m2.canonical_id = m.canonical_id
              )
        "#;
        let count: i64 = sqlx::query(count_sql)
            .bind(project_filter.as_deref())
            .bind(seconds)
            .fetch_one(&mut *tx)
            .await
            .map_err(ApiError::sql)?
            .try_get("count")
            .map_err(ApiError::sql)?;
        superseded_versions_pruned = count.max(0) as u64;

        if !dry_run && superseded_versions_pruned > 0 {
            let delete_sql = r#"
                DELETE FROM memory_entries m
                USING projects p
                WHERE m.project_id = p.id
                  AND ($1::text IS NULL OR p.slug = $1)
                  AND m.is_tombstone = FALSE
                  AND m.updated_at < now() - make_interval(secs => $2)
                  AND m.version_no < (
                      SELECT MAX(m2.version_no)
                      FROM memory_entries m2
                      WHERE m2.canonical_id = m.canonical_id
                  )
            "#;
            sqlx::query(delete_sql)
                .bind(project_filter.as_deref())
                .bind(seconds)
                .execute(&mut *tx)
                .await
                .map_err(ApiError::sql)?;
        }
    }

    tx.commit().await.map_err(ApiError::sql)?;

    Ok(Json(PruneHistoryResponse {
        project: project_filter,
        canonicals_tombstoned_deleted,
        superseded_versions_pruned,
        dry_run,
    }))
}

fn persist_timeline_event(state: &AppState, event: &ServiceEvent) {
    let Some(pool) = state.pool.clone() else {
        return;
    };
    let project = event.project.clone();
    let kind = activity_kind_label(&event.kind).to_string();
    let id = event.id;
    let summary = event.summary.clone();
    let memory_id = event.memory_id;
    let recorded_at = event.recorded_at;
    let details = event.details.clone().map(sqlx::types::Json);
    let actor_id = event.actor_id.clone();
    let actor_name = event.actor_name.clone();
    let source = event.source.clone();
    let operation_id = event.operation_id.clone();
    let duration_ms = event.duration_ms.map(|value| value as i64);
    let provider = event.provider.clone();
    let model = event.model.clone();
    let input_tokens = event
        .token_usage
        .as_ref()
        .map(|usage| usage.input_tokens as i64);
    let output_tokens = event
        .token_usage
        .as_ref()
        .map(|usage| usage.output_tokens as i64);
    let cache_read_tokens = event
        .token_usage
        .as_ref()
        .map(|usage| usage.cache_read_tokens as i64);
    let cache_write_tokens = event
        .token_usage
        .as_ref()
        .map(|usage| usage.cache_write_tokens as i64);
    let total_tokens = event
        .token_usage
        .as_ref()
        .map(|usage| usage.total_tokens as i64);
    tokio::spawn(async move {
        let project_id = match sqlx::query("SELECT id FROM projects WHERE slug = $1")
            .bind(&project)
            .fetch_optional(&pool)
            .await
        {
            Ok(Some(row)) => match row.try_get::<Uuid, _>("id") {
                Ok(value) => value,
                Err(_) => return,
            },
            _ => return,
        };
        let _ = sqlx::query(
            r#"
            INSERT INTO project_timeline_events (
                id, project_id, recorded_at, kind, memory_id, summary, details_json,
                actor_id, actor_name, source, operation_id, duration_ms, provider, model,
                input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, total_tokens
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19)
            "#,
        )
        .bind(id)
        .bind(project_id)
        .bind(recorded_at)
        .bind(kind)
        .bind(memory_id)
        .bind(summary)
        .bind(details)
        .bind(actor_id)
        .bind(actor_name)
        .bind(source)
        .bind(operation_id)
        .bind(duration_ms)
        .bind(provider)
        .bind(model)
        .bind(input_tokens)
        .bind(output_tokens)
        .bind(cache_read_tokens)
        .bind(cache_write_tokens)
        .bind(total_tokens)
        .execute(&pool)
        .await;
    });
}

fn notify_project_changed(
    state: &AppState,
    project: String,
    memory_id: Option<Uuid>,
    kind: ActivityKind,
    summary: String,
    details: Option<ActivityDetails>,
) {
    notify_project_changed_with_metadata(
        state, project, memory_id, kind, summary, details, None, None, None, None, None, None,
        None, None,
    );
}

#[allow(clippy::too_many_arguments)]
fn notify_project_changed_with_metadata(
    state: &AppState,
    project: String,
    memory_id: Option<Uuid>,
    kind: ActivityKind,
    summary: String,
    details: Option<ActivityDetails>,
    actor_id: Option<String>,
    actor_name: Option<String>,
    source: Option<String>,
    operation_id: Option<String>,
    duration_ms: Option<u64>,
    provider: Option<String>,
    model: Option<String>,
    token_usage: Option<TokenUsage>,
) {
    let event = ServiceEvent {
        id: Uuid::new_v4(),
        project,
        memory_id,
        kind,
        summary,
        details,
        recorded_at: chrono::Utc::now(),
        actor_id,
        actor_name,
        source: source.or_else(|| Some("service".to_string())),
        operation_id,
        duration_ms,
        provider,
        model,
        token_usage,
        include_activity: true,
    };
    let _ = state.events.send(event.clone());
    if event.include_activity {
        persist_timeline_event(state, &event);
    }
    let mut history = state
        .recent_activity
        .lock()
        .expect("activity history mutex poisoned");
    history.push_front(event);
    while history.len() > 20 {
        history.pop_back();
    }
}

fn notify_project_refreshed(state: &AppState, project: String) {
    let event = ServiceEvent {
        id: Uuid::new_v4(),
        project,
        memory_id: None,
        kind: ActivityKind::Query,
        summary: String::new(),
        details: None,
        recorded_at: chrono::Utc::now(),
        actor_id: None,
        actor_name: None,
        source: Some("service".to_string()),
        operation_id: None,
        duration_ms: None,
        provider: None,
        model: None,
        token_usage: None,
        include_activity: false,
    };
    let _ = state.events.send(event);
}

fn summarize_query(query: &str) -> String {
    let compact = query.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = compact.chars();
    let summary = chars.by_ref().take(80).collect::<String>();
    if chars.next().is_some() {
        format!("{summary}...")
    } else {
        summary
    }
}

fn activity_kind_label(kind: &ActivityKind) -> &'static str {
    match kind {
        ActivityKind::Checkpoint => "checkpoint",
        ActivityKind::Scan => "scan",
        ActivityKind::Plan => "plan",
        ActivityKind::CommitSync => "commit_sync",
        ActivityKind::BundleExport => "bundle_export",
        ActivityKind::BundleImport => "bundle_import",
        ActivityKind::GraphExtract => "graph_extract",
        ActivityKind::Query => "query",
        ActivityKind::QueryError => "query_error",
        ActivityKind::WatcherHealth => "watcher_health",
        ActivityKind::MemoryReplacement => "memory_replacement",
        ActivityKind::CaptureTask => "capture_task",
        ActivityKind::Curate => "curate",
        ActivityKind::Reindex => "reindex",
        ActivityKind::Reembed => "reembed",
        ActivityKind::Archive => "archive",
        ActivityKind::DeleteMemory => "delete_memory",
        ActivityKind::Briefing => "briefing",
    }
}

async fn fetch_project_overview_with_watchers(
    state: &AppState,
    slug: &str,
) -> Result<ProjectOverviewResponse, sqlx::Error> {
    let pool = state
        .pool
        .as_ref()
        .expect("project overview requires a primary database pool");
    let mut overview = fetch_project_overview(
        pool,
        slug,
        &state.config.automation,
        state.config.embeddings.active_backend(),
    )
    .await?;
    overview.watchers = Some(watcher_summary_for_project(&state.watchers, slug));
    Ok(overview)
}

fn register_watcher_heartbeat(
    watchers: &Mutex<HashMap<String, WatcherPresence>>,
    request: WatcherHeartbeatRequest,
) -> (
    WatcherPresenceSummary,
    bool,
    Option<(String, ActivityDetails)>,
) {
    let mut registry = watchers.lock().expect("watcher registry mutex poisoned");
    let before = watcher_summary_from_registry(&registry, &request.project);
    expire_dead_watchers(&mut registry);
    let now = chrono::Utc::now();
    let mut transition = None;
    registry
        .entry(request.watcher_id.clone())
        .and_modify(|watcher| {
            let previous_health = watcher.health.clone();
            let previous_restart_attempt_count = watcher.restart_attempt_count;
            let recovered = previous_health != WatcherHealth::Healthy;
            watcher.project = request.project.clone();
            watcher.repo_root = request.repo_root.clone();
            watcher.hostname = request.hostname.clone();
            watcher.pid = request.pid;
            watcher.mode = request.mode.clone();
            watcher.started_at = request.started_at;
            watcher.last_heartbeat_at = now;
            watcher.host_service_id = request.host_service_id.clone();
            watcher.managed_by_service = request.managed_by_service;
            watcher.agent_cli = request.agent_cli.clone();
            watcher.agent_session_id = request.agent_session_id.clone();
            watcher.agent_pid = request.agent_pid;
            watcher.agent_started_at = request.agent_started_at;
            watcher.health = WatcherHealth::Healthy;
            watcher.last_restart_attempt_at = None;
            watcher.restart_attempt_count = 0;
            if recovered {
                transition = Some((
                    format!(
                        "Watcher {} recovered from {} after {} restart attempt(s)",
                        request.watcher_id,
                        watcher_health_label(&previous_health),
                        previous_restart_attempt_count
                    ),
                    ActivityDetails::WatcherHealth {
                        watcher_id: request.watcher_id.clone(),
                        hostname: request.hostname.clone(),
                        health: WatcherHealth::Healthy,
                        managed_by_service: request.managed_by_service,
                        restart_attempt_count: 0,
                        agent_cli: request.agent_cli.clone(),
                        agent_session_id: request.agent_session_id.clone(),
                        agent_pid: request.agent_pid,
                        previous_health: Some(previous_health),
                        recovered_after_restart_attempts: Some(previous_restart_attempt_count),
                        message: Some("watcher heartbeat recovered".to_string()),
                    },
                ));
            }
        })
        .or_insert_with(|| WatcherPresence {
            watcher_id: request.watcher_id.clone(),
            project: request.project.clone(),
            repo_root: request.repo_root.clone(),
            hostname: request.hostname.clone(),
            pid: request.pid,
            mode: request.mode.clone(),
            started_at: request.started_at,
            last_heartbeat_at: now,
            host_service_id: request.host_service_id.clone(),
            managed_by_service: request.managed_by_service,
            health: WatcherHealth::Healthy,
            agent_cli: request.agent_cli.clone(),
            agent_session_id: request.agent_session_id.clone(),
            agent_pid: request.agent_pid,
            agent_started_at: request.agent_started_at,
            last_restart_attempt_at: None,
            restart_attempt_count: 0,
        });
    let after = watcher_summary_from_registry(&registry, &request.project);
    let changed = before.active_count != after.active_count
        || before.unhealthy_count != after.unhealthy_count
        || before
            .watchers
            .iter()
            .map(|watcher| watcher.watcher_id.as_str())
            .collect::<Vec<_>>()
            != after
                .watchers
                .iter()
                .map(|watcher| watcher.watcher_id.as_str())
                .collect::<Vec<_>>();
    (after, changed, transition)
}

fn unregister_watcher(
    watchers: &Mutex<HashMap<String, WatcherPresence>>,
    request: &WatcherUnregisterRequest,
) -> (WatcherPresenceSummary, bool) {
    let mut registry = watchers.lock().expect("watcher registry mutex poisoned");
    let before = watcher_summary_from_registry(&registry, &request.project);
    expire_dead_watchers(&mut registry);
    let removed = registry.remove(&request.watcher_id).is_some();
    let after = watcher_summary_from_registry(&registry, &request.project);
    let changed = removed
        || before.active_count != after.active_count
        || before.unhealthy_count != after.unhealthy_count
        || before
            .watchers
            .iter()
            .map(|watcher| watcher.watcher_id.as_str())
            .collect::<Vec<_>>()
            != after
                .watchers
                .iter()
                .map(|watcher| watcher.watcher_id.as_str())
                .collect::<Vec<_>>();
    (after, changed)
}

fn watcher_summary_for_project(
    watchers: &Mutex<HashMap<String, WatcherPresence>>,
    project: &str,
) -> WatcherPresenceSummary {
    let mut registry = watchers.lock().expect("watcher registry mutex poisoned");
    expire_dead_watchers(&mut registry);
    refresh_watcher_health_from_heartbeats(&mut registry);
    watcher_summary_from_registry(&registry, project)
}

fn expire_dead_watchers(registry: &mut HashMap<String, WatcherPresence>) {
    let expiry_after =
        chrono::Duration::from_std(StdDuration::from_secs(WATCHER_EXPIRY_AFTER_SECONDS))
            .expect("valid watcher expiry duration");
    let now = chrono::Utc::now();
    registry.retain(|_, watcher| now - watcher.last_heartbeat_at <= expiry_after);
}

fn refresh_watcher_health_from_heartbeats(registry: &mut HashMap<String, WatcherPresence>) {
    let stale_after =
        chrono::Duration::from_std(StdDuration::from_secs(WATCHER_STALE_AFTER_SECONDS))
            .expect("valid watcher stale duration");
    let now = chrono::Utc::now();
    for watcher in registry.values_mut() {
        if now - watcher.last_heartbeat_at > stale_after && watcher.health == WatcherHealth::Healthy
        {
            watcher.health = WatcherHealth::Stale;
        }
    }
}

fn watcher_summary_from_registry(
    registry: &HashMap<String, WatcherPresence>,
    project: &str,
) -> WatcherPresenceSummary {
    let mut watchers = registry
        .values()
        .filter(|watcher| watcher.project == project)
        .cloned()
        .collect::<Vec<_>>();
    watchers.sort_by(|left, right| {
        right
            .last_heartbeat_at
            .cmp(&left.last_heartbeat_at)
            .then_with(|| left.watcher_id.cmp(&right.watcher_id))
    });
    let last_heartbeat_at = watchers.first().map(|watcher| watcher.last_heartbeat_at);
    let active_count = watchers
        .iter()
        .filter(|watcher| watcher.health == WatcherHealth::Healthy)
        .count();
    let unhealthy_count = watchers.len().saturating_sub(active_count);
    WatcherPresenceSummary {
        active_count,
        unhealthy_count,
        stale_after_seconds: WATCHER_STALE_AFTER_SECONDS,
        last_heartbeat_at,
        watchers,
    }
}

fn update_local_watcher_restart_state(
    watchers: &Mutex<HashMap<String, WatcherPresence>>,
    watcher_id: &str,
) {
    let mut registry = watchers.lock().expect("watcher registry mutex poisoned");
    if let Some(watcher) = registry.get_mut(watcher_id) {
        watcher.health = WatcherHealth::Restarting;
        watcher.last_restart_attempt_at = Some(chrono::Utc::now());
        watcher.restart_attempt_count = watcher.restart_attempt_count.saturating_add(1);
    }
}

async fn run_watcher_watchdog(state: AppState) -> Result<()> {
    let tick = Duration::from_secs(15);
    let stale_after =
        chrono::Duration::from_std(StdDuration::from_secs(WATCHER_STALE_AFTER_SECONDS))
            .expect("valid watcher stale duration");
    let restart_backoff =
        chrono::Duration::from_std(StdDuration::from_secs(WATCHER_RESTART_BACKOFF_SECONDS))
            .expect("valid watcher restart backoff");
    loop {
        tokio::time::sleep(tick).await;
        if !state.is_primary() {
            continue;
        }

        let mut activity_events = Vec::new();
        let mut restart_requests = Vec::new();
        {
            let mut registry = state
                .watchers
                .lock()
                .expect("watcher registry mutex poisoned");
            expire_dead_watchers(&mut registry);
            let now = chrono::Utc::now();
            for watcher in registry.values_mut() {
                if now - watcher.last_heartbeat_at <= stale_after {
                    continue;
                }

                if !watcher.managed_by_service {
                    if watcher.health != WatcherHealth::Stale {
                        watcher.health = WatcherHealth::Stale;
                        activity_events.push((
                            watcher.project.clone(),
                            format!("Watcher {} went stale", watcher.watcher_id),
                            ActivityDetails::WatcherHealth {
                                watcher_id: watcher.watcher_id.clone(),
                                hostname: watcher.hostname.clone(),
                                health: WatcherHealth::Stale,
                                managed_by_service: false,
                                restart_attempt_count: watcher.restart_attempt_count,
                                agent_cli: watcher.agent_cli.clone(),
                                agent_session_id: watcher.agent_session_id.clone(),
                                agent_pid: watcher.agent_pid,
                                previous_health: Some(WatcherHealth::Healthy),
                                recovered_after_restart_attempts: None,
                                message: Some(
                                    "heartbeat missed; manual watcher will not be restarted"
                                        .to_string(),
                                ),
                            },
                        ));
                    }
                    continue;
                }

                let retry_allowed = watcher
                    .last_restart_attempt_at
                    .map(|last| now - last >= restart_backoff)
                    .unwrap_or(true);
                if watcher.restart_attempt_count >= WATCHER_MAX_RESTART_ATTEMPTS {
                    if watcher.health != WatcherHealth::Failed {
                        watcher.health = WatcherHealth::Failed;
                        activity_events.push((
                            watcher.project.clone(),
                            format!("Watcher {} failed to recover", watcher.watcher_id),
                            ActivityDetails::WatcherHealth {
                                watcher_id: watcher.watcher_id.clone(),
                                hostname: watcher.hostname.clone(),
                                health: WatcherHealth::Failed,
                                managed_by_service: true,
                                restart_attempt_count: watcher.restart_attempt_count,
                                agent_cli: watcher.agent_cli.clone(),
                                agent_session_id: watcher.agent_session_id.clone(),
                                agent_pid: watcher.agent_pid,
                                previous_health: Some(WatcherHealth::Restarting),
                                recovered_after_restart_attempts: None,
                                message: Some("watcher exceeded restart attempt limit".to_string()),
                            },
                        ));
                    }
                    continue;
                }
                if !retry_allowed || watcher.health == WatcherHealth::Restarting {
                    continue;
                }

                watcher.health = WatcherHealth::Restarting;
                watcher.last_restart_attempt_at = Some(now);
                watcher.restart_attempt_count = watcher.restart_attempt_count.saturating_add(1);
                restart_requests.push(WatcherRestartRequest {
                    project: watcher.project.clone(),
                    watcher_id: watcher.watcher_id.clone(),
                    host_service_id: watcher.host_service_id.clone(),
                    agent_session_id: watcher.agent_session_id.clone(),
                });
                activity_events.push((
                    watcher.project.clone(),
                    format!("Restarting watcher {}", watcher.watcher_id),
                    ActivityDetails::WatcherHealth {
                        watcher_id: watcher.watcher_id.clone(),
                        hostname: watcher.hostname.clone(),
                        health: WatcherHealth::Restarting,
                        managed_by_service: true,
                        restart_attempt_count: watcher.restart_attempt_count,
                        agent_cli: watcher.agent_cli.clone(),
                        agent_session_id: watcher.agent_session_id.clone(),
                        agent_pid: watcher.agent_pid,
                        previous_health: Some(WatcherHealth::Stale),
                        recovered_after_restart_attempts: None,
                        message: Some("watcher heartbeat missed; requesting restart".to_string()),
                    },
                ));
            }
        }

        for (project, summary, details) in activity_events {
            notify_project_refreshed(&state, project.clone());
            notify_project_changed(
                &state,
                project,
                None,
                ActivityKind::WatcherHealth,
                summary,
                Some(details),
            );
        }

        for request in restart_requests {
            let dispatch = dispatch_watcher_restart(&state, &request).await;
            if let Err(error) = dispatch {
                let mut registry = state
                    .watchers
                    .lock()
                    .expect("watcher registry mutex poisoned");
                if let Some(watcher) = registry.get_mut(&request.watcher_id) {
                    watcher.health = WatcherHealth::Failed;
                    let details = ActivityDetails::WatcherHealth {
                        watcher_id: watcher.watcher_id.clone(),
                        hostname: watcher.hostname.clone(),
                        health: WatcherHealth::Failed,
                        managed_by_service: watcher.managed_by_service,
                        restart_attempt_count: watcher.restart_attempt_count,
                        agent_cli: watcher.agent_cli.clone(),
                        agent_session_id: watcher.agent_session_id.clone(),
                        agent_pid: watcher.agent_pid,
                        previous_health: Some(WatcherHealth::Restarting),
                        recovered_after_restart_attempts: None,
                        message: Some(format!("restart request failed: {error}")),
                    };
                    let project = watcher.project.clone();
                    drop(registry);
                    notify_project_refreshed(&state, project.clone());
                    notify_project_changed(
                        &state,
                        project,
                        None,
                        ActivityKind::WatcherHealth,
                        format!("Watcher {} restart failed", request.watcher_id),
                        Some(details),
                    );
                }
            }
        }
    }
}

async fn dispatch_watcher_restart(state: &AppState, request: &WatcherRestartRequest) -> Result<()> {
    if request.host_service_id == state.config.cluster.service_id {
        restart_local_watcher_service_name(&local_watcher_restart_service_name(request))?;
        return Ok(());
    }

    let peer = cluster_peer_by_service_id(state, &request.host_service_id)
        .ok_or_else(|| anyhow::anyhow!("host-local memory service is unavailable"))?;
    let response = state
        .http_client
        .post(format!(
            "http://{}/v1/watchers/restart-local",
            peer.advertise_addr
        ))
        .header("x-api-token", &state.api_token)
        .json(request)
        .send()
        .await
        .context("send remote watcher restart request")?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("remote restart failed with {status}: {body}");
    }
    Ok(())
}

fn local_watcher_restart_service_name(request: &WatcherRestartRequest) -> String {
    request
        .agent_session_id
        .as_deref()
        .filter(|session_id| !session_id.trim().is_empty())
        .map(managed_watch_service_name)
        .unwrap_or_else(|| watch_service_unit_name(&request.project))
}

fn stream_activity_response(event: ServiceEvent) -> StreamResponse {
    StreamResponse::Activity {
        event: ActivityEvent {
            id: event.id,
            recorded_at: event.recorded_at,
            project: event.project,
            kind: event.kind,
            memory_id: event.memory_id,
            summary: event.summary,
            details: event.details,
            actor_id: event.actor_id,
            actor_name: event.actor_name,
            source: event.source,
            operation_id: event.operation_id,
            duration_ms: event.duration_ms,
            provider: event.provider,
            model: event.model,
            token_usage: event.token_usage,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mem_api::AutomationMode;

    fn test_presence(
        watcher_id: &str,
        project: &str,
        hostname: &str,
        mode: AutomationMode,
        last_heartbeat_at: chrono::DateTime<chrono::Utc>,
    ) -> WatcherPresence {
        WatcherPresence {
            watcher_id: watcher_id.to_string(),
            project: project.to_string(),
            repo_root: "/repo".to_string(),
            hostname: hostname.to_string(),
            pid: 111,
            mode,
            started_at: last_heartbeat_at,
            last_heartbeat_at,
            host_service_id: "service-a".to_string(),
            managed_by_service: true,
            health: WatcherHealth::Healthy,
            agent_cli: None,
            agent_session_id: None,
            agent_pid: None,
            agent_started_at: None,
            last_restart_attempt_at: None,
            restart_attempt_count: 0,
        }
    }

    fn test_query_response() -> QueryResponse {
        QueryResponse {
            answer: "fallback answer".to_string(),
            confidence: 0.5,
            results: vec![mem_api::QueryResult {
                memory_id: uuid::Uuid::new_v4(),
                summary: "Primary memory".to_string(),
                memory_type: mem_api::MemoryType::Architecture,
                score: 12.0,
                snippet: "Primary evidence snippet".to_string(),
                match_kind: mem_api::QueryMatchKind::Hybrid,
                score_explanation: Vec::new(),
                debug: mem_api::QueryResultDebug::default(),
                tags: Vec::new(),
                sources: Vec::new(),
                graph_connections: Vec::new(),
            }],
            insufficient_evidence: false,
            answer_generation: QueryAnswerGeneration::default(),
            answer_citations: Vec::new(),
            diagnostics: mem_api::QueryDiagnostics::default(),
        }
    }

    #[test]
    fn embedding_backend_toml_update_can_activate_and_deactivate() {
        let activated = set_active_embedding_backend_in_toml(
            r#"
            [embeddings]
            enabled = false
            active = "voyage"
            "#,
            Some("openai"),
        )
        .expect("activate toml");

        assert!(activated.contains("enabled = true"));
        assert!(activated.contains("active = \"openai\""));

        let deactivated =
            set_active_embedding_backend_in_toml(&activated, None).expect("deactivate toml");

        assert!(deactivated.contains("enabled = false"));
        assert!(deactivated.contains("active = \"openai\""));
    }

    #[test]
    fn embedding_creation_toml_update_sets_create_enabled() {
        let disabled = set_embedding_creation_enabled_in_toml(
            r#"
            [embeddings]
            enabled = true
            active = "voyage"

            [[embeddings.backends]]
            name = "voyage"
            provider = "voyage"
            model = "voyage-code-3"
            "#,
            "voyage",
            false,
        )
        .expect("disable creation toml");

        assert!(disabled.contains("enabled = true"));
        assert!(disabled.contains("active = \"voyage\""));
        assert!(disabled.contains("create_enabled = true"));
        assert!(disabled.contains("create_enabled = false"));

        let enabled = set_embedding_creation_enabled_in_toml(&disabled, "voyage", true)
            .expect("enable creation toml");

        assert!(enabled.contains("create_enabled = true"));
    }

    #[test]
    fn openai_embedding_space_aliases_legacy_and_compatible_keys() {
        assert_eq!(
            equivalent_openai_embedding_space_key(
                "openai|https://api.openai.com/v1|text-embedding-3-small"
            )
            .as_deref(),
            Some("openai_compatible|https://api.openai.com/v1|text-embedding-3-small")
        );
        assert_eq!(
            equivalent_openai_embedding_space_key(
                "openai_compatible|https://api.openai.com/v1|text-embedding-3-small"
            )
            .as_deref(),
            Some("openai|https://api.openai.com/v1|text-embedding-3-small")
        );
        assert!(
            equivalent_openai_embedding_space_key("voyage|https://api.voyageai.com|voyage-code-3")
                .is_none()
        );
    }

    #[test]
    fn llm_query_answer_content_accepts_valid_citations() {
        let response = test_query_response();
        let parsed = parse_llm_query_answer_content(
            r#"{"answer":"Use the primary memory. [1]","citations":[1],"confidence":0.88,"insufficient_evidence":false}"#,
            &response,
        )
        .expect("valid llm answer");

        assert_eq!(parsed.answer, "Use the primary memory. [1]");
        assert_eq!(parsed.cited_result_numbers, vec![1]);
        assert_eq!(parsed.citations.len(), 1);
        assert_eq!(parsed.confidence, 0.88);
        assert!(!parsed.insufficient_evidence);
    }

    #[test]
    fn llm_query_answer_content_rejects_unavailable_citation() {
        let response = test_query_response();
        let err = parse_llm_query_answer_content(
            r#"{"answer":"Unsupported","citations":[2],"confidence":0.8,"insufficient_evidence":false}"#,
            &response,
        )
        .expect_err("invalid citation should fail");

        assert!(err.to_string().contains("cited unavailable result 2"));
    }

    #[test]
    fn query_answer_prompt_includes_graph_connections() {
        let mut response = test_query_response();
        response.results[0].graph_connections = vec![mem_api::QueryGraphConnection {
            file_path: "src/lib.rs".to_string(),
            symbol: Some("GraphTarget".to_string()),
            symbol_kind: Some("function".to_string()),
            edge_kind: Some("calls".to_string()),
            neighbor_symbol: Some("caller".to_string()),
            direction: Some("incoming".to_string()),
            score_boost: 1.25,
            reason: "code symbol match".to_string(),
        }];

        let prompt = build_query_answer_prompt(
            &QueryRequest {
                project: "memory".to_string(),
                query: "GraphTarget".to_string(),
                filters: Default::default(),
                top_k: 8,
                min_confidence: None,
                history: false,
                retrieval_mode: None,
                answer_mode: None,
            },
            &response,
        );

        assert!(prompt.contains("graph: code symbol match | src/lib.rs"));
        assert!(prompt.contains("symbol=GraphTarget"));
        assert!(prompt.contains("edge=calls"));
    }

    #[test]
    fn query_activity_details_include_graph_diagnostics() {
        let mut response = test_query_response();
        response.diagnostics.graph_status = "active".to_string();
        response.diagnostics.graph_candidates = 4;
        response.diagnostics.graph_augmented_candidates = 2;
        response.diagnostics.graph_duration_ms = 17;
        response.diagnostics.total_duration_ms = 91;
        response.results[0].debug.graph_boost = 1.25;
        response.results[0].graph_connections = vec![
            mem_api::QueryGraphConnection {
                file_path: "src/lib.rs".to_string(),
                symbol: Some("GraphTarget".to_string()),
                symbol_kind: Some("function".to_string()),
                edge_kind: Some("calls".to_string()),
                neighbor_symbol: Some("caller".to_string()),
                direction: Some("incoming".to_string()),
                score_boost: 1.25,
                reason: "code symbol match".to_string(),
            },
            mem_api::QueryGraphConnection {
                file_path: "src/other.rs".to_string(),
                symbol: Some("OtherTarget".to_string()),
                symbol_kind: Some("struct".to_string()),
                edge_kind: None,
                neighbor_symbol: None,
                direction: None,
                score_boost: 1.0,
                reason: "code reference match".to_string(),
            },
        ];

        let details = query_activity_details(
            &QueryRequest {
                project: "memory".to_string(),
                query: "GraphTarget".to_string(),
                filters: Default::default(),
                top_k: 8,
                min_confidence: None,
                history: false,
                retrieval_mode: None,
                answer_mode: None,
            },
            &response,
        );

        match details {
            ActivityDetails::Query {
                graph_status,
                graph_candidates,
                graph_augmented_candidates,
                graph_duration_ms,
                graph_result_count,
                graph_connection_count,
                graph_connections,
                ..
            } => {
                assert_eq!(graph_status.as_deref(), Some("active"));
                assert_eq!(graph_candidates, 4);
                assert_eq!(graph_augmented_candidates, 2);
                assert_eq!(graph_duration_ms, 17);
                assert_eq!(graph_result_count, 1);
                assert_eq!(graph_connection_count, 2);
                assert_eq!(graph_connections.len(), 2);
                assert_eq!(graph_connections[0].file_path, "src/lib.rs");
            }
            other => panic!("unexpected activity details: {other:?}"),
        }
    }

    #[test]
    fn graph_activity_summary_and_details_capture_extraction_counts() {
        let run_id = Uuid::new_v4();
        let request = GraphActivityRequest {
            project: "memory".to_string(),
            repo_root: "/repo".to_string(),
            git_head: Some("abc123".to_string()),
            since: None,
            extraction_run_id: Some(run_id),
            dry_run: false,
            reused_existing_run: true,
            index_reused: true,
            analyzer_version: "mem-analyze-v2".to_string(),
            strategy_version: "code-graph-resolution-v1".to_string(),
            symbol_count: 1919,
            reference_count: 80116,
            resolved_reference_count: 14621,
            unresolved_reference_count: 61249,
            ambiguous_reference_count: 4246,
            graph_node_count: 1919,
            graph_edge_count: 13812,
            evidence_count: 15731,
        };

        let summary = graph_activity_summary(&request);
        assert!(summary.contains("Reused code graph extraction"));
        assert!(summary.contains("1919 symbols"));
        assert!(summary.contains("13812 graph edge"));

        match graph_activity_details(&request) {
            ActivityDetails::GraphExtract {
                extraction_run_id,
                reference_count,
                graph_edge_count,
                reused_existing_run,
                ..
            } => {
                assert_eq!(extraction_run_id, Some(run_id));
                assert_eq!(reference_count, 80116);
                assert_eq!(graph_edge_count, 13812);
                assert!(reused_existing_run);
            }
            other => panic!("unexpected activity details: {other:?}"),
        }
    }

    #[test]
    fn token_usage_from_chat_body_reads_openai_compatible_usage() {
        let usage = token_usage_from_chat_body(
            r#"{"usage":{"prompt_tokens":1200,"completion_tokens":300,"cached_input_tokens":200,"cache_creation_input_tokens":50,"total_tokens":1750}}"#,
        )
        .expect("usage");

        assert_eq!(usage.input_tokens, 1200);
        assert_eq!(usage.output_tokens, 300);
        assert_eq!(usage.cache_read_tokens, 200);
        assert_eq!(usage.cache_write_tokens, 50);
        assert_eq!(usage.total_tokens, 1750);
    }

    #[test]
    fn up_to_speed_briefing_includes_token_summary() {
        let token_usage = TokenUsageSummary {
            action_count: 2,
            total_input_tokens: 100,
            total_output_tokens: 40,
            total_cache_read_tokens: 20,
            total_cache_write_tokens: 5,
            total_tokens: 165,
        };
        let briefing = build_up_to_speed_briefing(
            "memory",
            &["Recent work focused on activity history.".to_string()],
            &["Persisted activity events".to_string()],
            &[],
            &[],
            &[],
            &token_usage,
        );

        assert!(briefing.contains("Get up to speed"));
        assert!(briefing.contains("165 total"));
        assert!(briefing.contains("2 recent action"));
    }

    #[tokio::test]
    async fn recent_activity_responses_replays_latest_project_events() {
        let recent_activity = Mutex::new(VecDeque::from(vec![
            ServiceEvent {
                id: Uuid::new_v4(),
                project: "memory".to_string(),
                memory_id: None,
                kind: ActivityKind::Curate,
                summary: "Curated memory".to_string(),
                details: None,
                recorded_at: chrono::Utc::now(),
                actor_id: None,
                actor_name: None,
                source: Some("service".to_string()),
                operation_id: None,
                duration_ms: None,
                provider: None,
                model: None,
                token_usage: None,
                include_activity: true,
            },
            ServiceEvent {
                id: Uuid::new_v4(),
                project: "other".to_string(),
                memory_id: None,
                kind: ActivityKind::CaptureTask,
                summary: "Captured task".to_string(),
                details: None,
                recorded_at: chrono::Utc::now(),
                actor_id: None,
                actor_name: None,
                source: Some("service".to_string()),
                operation_id: None,
                duration_ms: None,
                provider: None,
                model: None,
                token_usage: None,
                include_activity: true,
            },
            ServiceEvent {
                id: Uuid::new_v4(),
                project: "memory".to_string(),
                memory_id: None,
                kind: ActivityKind::Reindex,
                summary: "Reindexed entries".to_string(),
                details: None,
                recorded_at: chrono::Utc::now(),
                actor_id: None,
                actor_name: None,
                source: Some("service".to_string()),
                operation_id: None,
                duration_ms: None,
                provider: None,
                model: None,
                token_usage: None,
                include_activity: true,
            },
        ]));

        let responses = recent_activity_responses(&recent_activity, "memory").await;
        assert_eq!(responses.len(), 2);

        let summaries = responses
            .into_iter()
            .map(|response| match response {
                StreamResponse::Activity { event } => event.summary,
                other => panic!("unexpected response: {other:?}"),
            })
            .collect::<Vec<_>>();
        assert_eq!(summaries, vec!["Curated memory", "Reindexed entries"]);
    }

    #[test]
    fn watcher_registry_refreshes_without_double_counting() {
        let watchers = Mutex::new(HashMap::new());
        let started_at = chrono::Utc::now();
        let request = WatcherHeartbeatRequest {
            watcher_id: "watcher-1".to_string(),
            project: "memory".to_string(),
            repo_root: "/repo".to_string(),
            hostname: "host-a".to_string(),
            pid: 111,
            mode: AutomationMode::Suggest,
            started_at,
            host_service_id: "service-a".to_string(),
            managed_by_service: true,
            agent_cli: None,
            agent_session_id: None,
            agent_pid: None,
            agent_started_at: None,
        };

        let (first, first_changed, _) = register_watcher_heartbeat(&watchers, request.clone());
        let (second, second_changed, transition) = register_watcher_heartbeat(&watchers, request);

        assert_eq!(first.active_count, 1);
        assert_eq!(second.active_count, 1);
        assert_eq!(second.unhealthy_count, 0);
        assert_eq!(second.watchers.len(), 1);
        assert_eq!(second.watchers[0].watcher_id, "watcher-1");
        assert!(first_changed);
        assert!(!second_changed);
        assert!(transition.is_none());
    }

    #[test]
    fn watcher_summary_filters_by_project_and_marks_stale_entries_unhealthy() {
        let now = chrono::Utc::now();
        let mut registry = HashMap::new();
        registry.insert(
            "watcher-live".to_string(),
            test_presence(
                "watcher-live",
                "memory",
                "host-a",
                AutomationMode::Suggest,
                now,
            ),
        );
        registry.insert(
            "watcher-other".to_string(),
            test_presence(
                "watcher-other",
                "other",
                "host-b",
                AutomationMode::Auto,
                now,
            ),
        );
        registry.insert(
            "watcher-stale".to_string(),
            test_presence(
                "watcher-stale",
                "memory",
                "host-c",
                AutomationMode::Suggest,
                now - chrono::Duration::seconds(WATCHER_STALE_AFTER_SECONDS as i64 + 1),
            ),
        );
        let watchers = Mutex::new(registry);

        let summary = watcher_summary_for_project(&watchers, "memory");

        assert_eq!(summary.active_count, 1);
        assert_eq!(summary.unhealthy_count, 1);
        assert_eq!(summary.watchers.len(), 2);
        assert_eq!(summary.watchers[0].watcher_id, "watcher-live");
        assert_eq!(summary.watchers[1].watcher_id, "watcher-stale");
    }

    #[test]
    fn stale_manual_watcher_is_counted_as_unhealthy() {
        let now = chrono::Utc::now();
        let watchers = Mutex::new(HashMap::from([(
            "watcher-manual".to_string(),
            WatcherPresence {
                managed_by_service: false,
                ..test_presence(
                    "watcher-manual",
                    "memory",
                    "host-a",
                    AutomationMode::Suggest,
                    now - chrono::Duration::seconds(WATCHER_STALE_AFTER_SECONDS as i64 + 1),
                )
            },
        )]));

        let summary = watcher_summary_for_project(&watchers, "memory");
        assert_eq!(summary.active_count, 0);
        assert_eq!(summary.unhealthy_count, 1);
    }

    #[test]
    fn watcher_restart_service_name_prefers_managed_session_identity() {
        let managed = WatcherRestartRequest {
            project: "memory".to_string(),
            watcher_id: "watcher-1".to_string(),
            host_service_id: "service-a".to_string(),
            agent_session_id: Some("session 123".to_string()),
        };
        assert_eq!(
            local_watcher_restart_service_name(&managed),
            mem_platform::managed_watch_service_name("session 123")
        );

        let legacy = WatcherRestartRequest {
            project: "customer portal".to_string(),
            watcher_id: "watcher-2".to_string(),
            host_service_id: "service-a".to_string(),
            agent_session_id: None,
        };
        assert_eq!(
            local_watcher_restart_service_name(&legacy),
            mem_platform::watch_service_unit_name("customer portal")
        );
    }
}

fn require_token(headers: &HeaderMap, expected: &str, bind_addr: &str) -> Result<(), ApiError> {
    if let Some(provided) = headers
        .get("x-api-token")
        .and_then(|value| value.to_str().ok())
    {
        if provided != expected {
            return Err(ApiError::unauthorized("invalid api token"));
        }
        return Ok(());
    }

    if is_local_browser_request(headers, bind_addr) {
        return Ok(());
    }

    Err(ApiError::unauthorized(
        "missing x-api-token header or trusted local browser origin",
    ))
}

fn require_strict_token(headers: &HeaderMap, expected: &str) -> Result<(), ApiError> {
    let Some(provided) = headers
        .get("x-api-token")
        .and_then(|value| value.to_str().ok())
    else {
        return Err(ApiError::unauthorized("missing x-api-token header"));
    };
    if provided != expected {
        return Err(ApiError::unauthorized("invalid api token"));
    }
    Ok(())
}

fn is_local_browser_request(headers: &HeaderMap, bind_addr: &str) -> bool {
    let configured_host = bind_addr
        .rsplit_once(':')
        .map(|(host, _)| host.trim_matches('[').trim_matches(']'))
        .unwrap_or(bind_addr);

    ["origin", "referer"].iter().any(|header| {
        headers
            .get(*header)
            .and_then(|value| value.to_str().ok())
            .map(|value| {
                value.starts_with("http://127.0.0.1")
                    || value.starts_with("http://localhost")
                    || value.starts_with("http://[::1]")
                    || value.starts_with("https://127.0.0.1")
                    || value.starts_with("https://localhost")
                    || value.starts_with("https://[::1]")
                    || value.starts_with(&format!("http://{configured_host}"))
                    || value.starts_with(&format!("https://{configured_host}"))
            })
            .unwrap_or(false)
    })
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn validation(error: ValidationError) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: error.to_string(),
        }
    }

    fn unauthorized(message: &str) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.to_string(),
        }
    }

    fn not_found(message: &str) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.to_string(),
        }
    }

    fn service_unavailable(message: &str) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: message.to_string(),
        }
    }

    fn status_message(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    fn sql(error: sqlx::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: error.to_string(),
        }
    }

    fn io(error: anyhow::Error) -> Self {
        let mut message = error.to_string();
        for cause in error.chain().skip(1) {
            message.push_str(": ");
            message.push_str(&cause.to_string());
        }
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({
                "error": self.message
            })),
        )
            .into_response()
    }
}
