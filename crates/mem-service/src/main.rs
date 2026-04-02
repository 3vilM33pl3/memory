use std::{
    collections::{HashMap, VecDeque},
    io::ErrorKind,
    io::Read,
    net::SocketAddr,
    path::{Path as FsPath, PathBuf},
    sync::{Arc, Mutex},
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
    ActivityDetails, ActivityEvent, ActivityKind, AppConfig, ArchiveRequest, ArchiveResponse,
    CaptureTaskRequest, CheckpointActivityRequest, CommitDetailResponse, CommitSyncRequest,
    CommitSyncResponse, CurateRequest, DeleteMemoryRequest, DeleteMemoryResponse,
    MemoryEntryResponse, MemorySourceRecord, ProjectCommitsResponse, ProjectMemoriesResponse,
    ProjectMemoryBundleEntry, ProjectMemoryBundleEntryRelation, ProjectMemoryBundleManifest,
    ProjectMemoryBundlePreview, ProjectMemoryBundleSource, ProjectMemoryExportOptions,
    ProjectMemoryImportPreview, ProjectMemoryImportResponse, ProjectOverviewResponse,
    PruneEmbeddingsRequest, PruneEmbeddingsResponse, QueryRequest, ReembedRequest, ReembedResponse,
    ReindexRequest, ReindexResponse, RelatedMemorySummary, ReplacementProposalListResponse,
    ReplacementProposalResolutionResponse, ResumeAction, ResumeRequest, ResumeResponse,
    ScanActivityRequest, SourceKind, StatsResponse, StreamRequest, StreamResponse, ValidationError,
    WatcherHealth, WatcherHeartbeatRequest, WatcherPresence, WatcherPresenceSummary,
    WatcherRestartRequest, WatcherRestartResponse, WatcherUnregisterRequest, read_capnp_text_frame,
    write_capnp_text_frame,
};
use mem_curate::{
    approve_replacement_proposal, curate, list_replacement_proposals, refresh_memory_relations,
    reject_replacement_proposal, store_capture,
};
use mem_platform::restart_local_watcher_service;
use mem_search::{
    EmbeddingService, parse_memory_type, parse_relation_type, parse_source_kind,
    prune_project_embeddings, query_memory, rebuild_chunks, reembed_project_chunks,
};
use mem_service::{
    fetch_project_commit, fetch_project_commits, fetch_project_memories, fetch_project_overview,
    parse_status_filter, sync_project_commits,
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

#[derive(Clone)]
struct AppState {
    role: ServiceRole,
    instance_id: String,
    pool: Option<PgPool>,
    api_token: String,
    config: AppConfig,
    web_root: Option<PathBuf>,
    http_client: reqwest::Client,
    embedder: Option<EmbeddingService>,
    events: broadcast::Sender<ServiceEvent>,
    recent_activity: Arc<Mutex<VecDeque<ServiceEvent>>>,
    watchers: Arc<Mutex<HashMap<String, WatcherPresence>>>,
    cluster: ClusterRuntime,
    shutdown: Arc<Mutex<Option<oneshot::Sender<()>>>>,
}

#[derive(Clone, Debug)]
struct ServiceEvent {
    project: String,
    memory_id: Option<Uuid>,
    kind: ActivityKind,
    summary: String,
    details: Option<ActivityDetails>,
    recorded_at: chrono::DateTime<chrono::Utc>,
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
        println!("memory service {}", env!("CARGO_PKG_VERSION"));
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
    let (role, pool, embedder) = match pool_attempt {
        Ok(pool) => {
            sqlx::migrate!("../../migrations")
                .run(&pool)
                .await
                .context(
                    "run migrations (pgvector extension 'vector' must be installed in PostgreSQL)",
                )?;
            (
                ServiceRole::Primary,
                Some(pool),
                EmbeddingService::from_config(&config),
            )
        }
        Err(error) if config.cluster.enabled => {
            tracing::warn!(
                error = %error,
                "postgres unavailable; starting in relay mode"
            );
            (ServiceRole::Relay, None, None)
        }
        Err(error) => return Err(error).context("connect postgres"),
    };

    Ok(AppState {
        role,
        instance_id: Uuid::new_v4().to_string(),
        pool,
        api_token: config.service.api_token.clone(),
        web_root: discover_web_root(&config),
        http_client,
        embedder,
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
        std::env::var("HOME")
            .ok()
            .map(|home| PathBuf::from(home).join(".local/share/memory-layer/web")),
        Some(PathBuf::from("/usr/share/memory-layer/web")),
    ];

    for candidate in candidates.into_iter().flatten() {
        if candidate.join("index.html").is_file() {
            return Some(candidate);
        }
    }

    None
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
        .route("/v1/scan/activity", post(scan_activity))
        .route("/v1/commits/sync", post(sync_commits))
        .route("/v1/capture/task", post(capture_task))
        .route("/v1/curate", post(curate_memory))
        .route("/v1/reindex", post(reindex))
        .route("/v1/reembed", post(reembed))
        .route("/v1/prune-embeddings", post(prune_embeddings))
        .route("/v1/memory/{id}", get(get_memory))
        .route("/v1/memory", delete(delete_memory))
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
        .route("/v1/projects/{slug}/memories", get(project_memories))
        .route("/v1/projects/{slug}/overview", get(project_overview))
        .route("/v1/projects/{slug}/resume", post(project_resume))
        .route("/v1/watchers/heartbeat", post(watcher_heartbeat))
        .route("/v1/watchers/unregister", post(watcher_unregister))
        .route("/v1/watchers/restart-local", post(watcher_restart_local))
        .route("/v1/archive", post(archive))
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
        version: env!("CARGO_PKG_VERSION").to_string(),
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
    if let Some(project) = &subscriptions.project {
        if project == &event.project {
            if event.include_activity {
                responses.push(stream_activity_response(event.clone()));
            }
            let overview = fetch_project_overview_with_watchers(state, project).await?;
            let memories = fetch_project_memories(pool, project, None, 500, 0).await?;
            responses.push(StreamResponse::ProjectChanged { overview, memories });
        }
    }

    if let Some(memory_id) = subscriptions.memory_id {
        if event.memory_id == Some(memory_id) {
            let detail = fetch_memory_entry(pool, memory_id).await?;
            responses.push(StreamResponse::MemoryChanged { detail });
        }
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
            "version": env!("CARGO_PKG_VERSION")
        }))
    } else {
        let upstream = relay_upstream_health(state).await?;
        Ok(serde_json::json!({
            "status": if upstream.is_some() { "ok" } else { "degraded" },
            "role": "relay",
            "database": "down",
            "instance_id": state.instance_id,
            "service_id": state.config.cluster.service_id,
            "version": env!("CARGO_PKG_VERSION"),
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
               m.status, m.created_at, m.updated_at
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
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    }))
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
                if let Some(excerpt) = &source.excerpt {
                    if email_re.is_match(excerpt)
                        || token_re.is_match(excerpt)
                        || path_re.is_match(excerpt)
                        || phone_re.is_match(excerpt)
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
      <p>Build the frontend under <code>web/</code> or install a package that ships <code>/usr/share/memory-layer/web</code>.</p>
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
    match query_memory(pool, &request, state.embedder.as_ref()).await {
        Ok(response) => {
            notify_project_changed(
                &state,
                request.project.clone(),
                None,
                ActivityKind::Query,
                format!("Query: {}", summarize_query(&request.query)),
                Some(ActivityDetails::Query {
                    query: request.query.clone(),
                    top_k: request.top_k,
                    result_count: response.results.len(),
                    confidence: response.confidence,
                    insufficient_evidence: response.insufficient_evidence,
                    total_duration_ms: response.diagnostics.total_duration_ms,
                    answer: Some(response.answer.clone()),
                    error: None,
                }),
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
                    answer: None,
                    error: Some(error.to_string()),
                }),
            );
            Err(ApiError::io(error))
        }
    }
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
    let response = store_capture(state.pool()?, &request)
        .await
        .map_err(ApiError::sql)?;
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
    let response = curate(state.pool()?, &request)
        .await
        .map_err(ApiError::sql)?;
    if state.embedder.is_some() {
        rebuild_chunks(state.pool()?, &request.project, state.embedder.as_ref())
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
    let count = rebuild_chunks(state.pool()?, &request.project, state.embedder.as_ref())
        .await
        .map_err(ApiError::io)?;
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
    let Some(embedder) = state.embedder.as_ref() else {
        return Err(ApiError::validation(ValidationError::new(
            "embeddings are not configured; cannot re-embed",
        )));
    };
    let project = request.project.clone();
    let count = reembed_project_chunks(state.pool()?, &request.project, embedder)
        .await
        .map_err(ApiError::io)?;
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
    let Some(embedder) = state.embedder.as_ref() else {
        return Err(ApiError::validation(ValidationError::new(
            "embeddings are not configured; cannot prune inactive spaces",
        )));
    };
    let project = request.project.clone();
    let count = prune_project_embeddings(state.pool()?, &request.project, embedder)
        .await
        .map_err(ApiError::io)?;
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
    }))
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
    let response = sync_project_commits(state.pool()?, &request)
        .await
        .map_err(ApiError::sql)?;
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

        if let Some(row) = existing {
            let existing_memory_id: Uuid = row.try_get("memory_entry_id").map_err(ApiError::sql)?;
            let existing_hash: String = row.try_get("entry_hash").map_err(ApiError::sql)?;
            if existing_hash == hash {
                current_ids.insert(entry.entry_key.clone(), existing_memory_id);
                skipped_count += 1;
                continue;
            }
            sqlx::query("DELETE FROM memory_entries WHERE id = $1")
                .bind(existing_memory_id)
                .execute(pool)
                .await
                .map_err(ApiError::sql)?;
            replaced_count += 1;
        }

        let memory_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO memory_entries
                (id, project_id, canonical_text, summary, memory_type, scope, importance, confidence, status, created_at, updated_at, archived_at, search_document)
            VALUES
                ($1, $2, $3, $4, $5, 'project', $6, $7, 'active', $8, $9, NULL, to_tsvector('english', $3 || ' ' || $4))
            "#,
        )
        .bind(memory_id)
        .bind(target_project_id)
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

    rebuild_chunks(pool, &slug, state.embedder.as_ref())
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
    Json(request): Json<ResumeRequest>,
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
    let (overview, timeline, commits, changed_memories, durable_context) = tokio::try_join!(
        overview_fut,
        timeline_fut,
        commits_fut,
        changed_memories_fut,
        durable_context_fut,
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
    );
    let change_summary = build_change_summary(&timeline, &commits, &changed_memories);
    let attention_items = build_attention_items(&overview, &timeline);
    let context_items = select_resume_context(&changed_memories, &durable_context);
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

async fn fetch_project_timeline(
    pool: &PgPool,
    slug: &str,
    since: Option<chrono::DateTime<chrono::Utc>>,
    limit: usize,
) -> Result<Vec<ActivityEvent>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT te.recorded_at, p.slug AS project, te.kind, te.memory_id, te.summary, te.details_json
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
            recorded_at: row.try_get("recorded_at")?,
            project: row.try_get("project")?,
            kind: parse_activity_kind(&kind),
            memory_id: row.try_get("memory_id")?,
            summary: row.try_get("summary")?,
            details,
        });
    }
    Ok(items)
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
) -> Option<String> {
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
            ActivityKind::Scan => "Recent work focused on refreshing project memory from a repo scan.",
            ActivityKind::Curate => "Recent work focused on curating new captures into canonical memory.",
            ActivityKind::CaptureTask => "Recent work captured fresh project evidence that may need follow-up.",
            ActivityKind::MemoryReplacement => {
                "Recent work replaced outdated memory with a newer canonical version."
            }
            ActivityKind::Reindex => "Recent work rebuilt the project's searchable chunk index.",
            ActivityKind::Reembed => {
                "Recent work refreshed the active embedding space for semantic retrieval."
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
            ActivityKind::Checkpoint => "",
        };
        if !thread.is_empty() {
            return Some(format!("{thread} Latest event: {}", event.summary.trim_end_matches('.')));
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
        if let Some(task_title) = extract_capture_task_title(event) {
            if !seen_titles.contains(&task_title) {
                items.push(format!(
                    "{} Worked on: {}",
                    event.recorded_at.format("%m-%d %H:%M"),
                    task_title
                ));
                seen_titles.push(task_title);
            }
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
        items.push(format!("{unhealthy} watcher(s) are unhealthy or restarting."));
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
            ActivityKind::Scan | ActivityKind::Reembed | ActivityKind::Reindex
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
) -> Vec<mem_api::ProjectMemoryListItem> {
    let mut selected = Vec::new();

    if let Some(item) = changed_memories.iter().find(|item| {
        matches!(
            item.memory_type,
            mem_api::MemoryType::Decision
                | mem_api::MemoryType::Architecture
                | mem_api::MemoryType::Convention
                | mem_api::MemoryType::Debugging
        )
    }) {
        selected.push(item.clone());
    } else if let Some(item) = changed_memories.first() {
        selected.push(item.clone());
    }

    if let Some(item) = durable_context.iter().find(|item| {
        matches!(
            item.memory_type,
            mem_api::MemoryType::Architecture | mem_api::MemoryType::Convention
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
            lines.push(format!(
                "- [{}] {}",
                item.memory_type,
                item.summary
            ));
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

fn parse_activity_kind(value: &str) -> ActivityKind {
    match value {
        "checkpoint" => ActivityKind::Checkpoint,
        "scan" => ActivityKind::Scan,
        "commit_sync" => ActivityKind::CommitSync,
        "bundle_export" => ActivityKind::BundleExport,
        "bundle_import" => ActivityKind::BundleImport,
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

    restart_local_watcher_service(&request.project).map_err(ApiError::io)?;
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
    let result = sqlx::query(
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
    .map_err(ApiError::sql)?;
    notify_project_changed(
        &state,
        project,
        None,
        ActivityKind::Archive,
        format!(
            "Archived {} low-value memory entry/entries.",
            result.rows_affected()
        ),
        Some(ActivityDetails::Archive {
            archived_count: result.rows_affected(),
            max_confidence: request.max_confidence,
            max_importance: request.max_importance,
        }),
    );

    Ok(Json(ArchiveResponse {
        archived_count: result.rows_affected(),
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

    let record = sqlx::query(
        r#"
        DELETE FROM memory_entries m
        USING projects p
        WHERE m.project_id = p.id
          AND m.id = $1
        RETURNING m.id, p.slug, m.summary
        "#,
    )
    .bind(request.memory_id)
    .fetch_optional(state.pool()?)
    .await
    .map_err(ApiError::sql)?
    .ok_or_else(|| ApiError::not_found("memory entry not found"))?;

    let memory_id = record.try_get("id").map_err(ApiError::sql)?;
    let project: String = record.try_get("slug").map_err(ApiError::sql)?;
    let summary: String = record.try_get("summary").map_err(ApiError::sql)?;
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

fn persist_timeline_event(state: &AppState, event: &ServiceEvent) {
    let Some(pool) = state.pool.clone() else {
        return;
    };
    let project = event.project.clone();
    let kind = activity_kind_label(&event.kind).to_string();
    let summary = event.summary.clone();
    let memory_id = event.memory_id;
    let recorded_at = event.recorded_at;
    let details = event.details.clone().map(sqlx::types::Json);
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
            INSERT INTO project_timeline_events (id, project_id, recorded_at, kind, memory_id, summary, details_json)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(project_id)
        .bind(recorded_at)
        .bind(kind)
        .bind(memory_id)
        .bind(summary)
        .bind(details)
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
    let event = ServiceEvent {
        project,
        memory_id,
        kind,
        summary,
        details,
        recorded_at: chrono::Utc::now(),
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
        project,
        memory_id: None,
        kind: ActivityKind::Query,
        summary: String::new(),
        details: None,
        recorded_at: chrono::Utc::now(),
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
        ActivityKind::CommitSync => "commit_sync",
        ActivityKind::BundleExport => "bundle_export",
        ActivityKind::BundleImport => "bundle_import",
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
        &state.config.embeddings,
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
        restart_local_watcher_service(&request.project)?;
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

fn stream_activity_response(event: ServiceEvent) -> StreamResponse {
    StreamResponse::Activity {
        event: ActivityEvent {
            recorded_at: event.recorded_at,
            project: event.project,
            kind: event.kind,
            memory_id: event.memory_id,
            summary: event.summary,
            details: event.details,
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
            last_restart_attempt_at: None,
            restart_attempt_count: 0,
        }
    }

    #[tokio::test]
    async fn recent_activity_responses_replays_latest_project_events() {
        let recent_activity = Mutex::new(VecDeque::from(vec![
            ServiceEvent {
                project: "memory".to_string(),
                memory_id: None,
                kind: ActivityKind::Curate,
                summary: "Curated memory".to_string(),
                details: None,
                recorded_at: chrono::Utc::now(),
                include_activity: true,
            },
            ServiceEvent {
                project: "other".to_string(),
                memory_id: None,
                kind: ActivityKind::CaptureTask,
                summary: "Captured task".to_string(),
                details: None,
                recorded_at: chrono::Utc::now(),
                include_activity: true,
            },
            ServiceEvent {
                project: "memory".to_string(),
                memory_id: None,
                kind: ActivityKind::Reindex,
                summary: "Reindexed entries".to_string(),
                details: None,
                recorded_at: chrono::Utc::now(),
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
