use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    path::{Path as FsPath, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration as StdDuration, SystemTime},
};

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{any, delete, get, post},
};
use futures_util::{SinkExt, StreamExt};
use mem_api::{
    ActivityDetails, ActivityEvent, ActivityKind, AppConfig, ArchiveRequest, ArchiveResponse,
    CaptureTaskRequest, CommitDetailResponse, CommitSyncRequest, CommitSyncResponse, CurateRequest,
    DeleteMemoryRequest, DeleteMemoryResponse, MemoryEntryResponse, MemorySourceRecord,
    ProjectCommitsResponse, ProjectMemoriesResponse, ProjectOverviewResponse, QueryRequest,
    ReindexRequest, ReindexResponse, RelatedMemorySummary, StatsResponse, StreamRequest,
    StreamResponse, ValidationError, WatcherHeartbeatRequest, WatcherPresence,
    WatcherPresenceSummary, WatcherUnregisterRequest, read_capnp_text_frame,
    write_capnp_text_frame,
};
use mem_curate::{curate, store_capture};
use mem_search::{
    EmbeddingService, parse_memory_type, parse_relation_type, parse_source_kind, query_memory,
    rebuild_chunks,
};
use mem_service::{
    fetch_project_commit, fetch_project_commits, fetch_project_memories, fetch_project_overview,
    parse_status_filter, sync_project_commits,
};
use serde::Deserialize;
use serde::{Deserialize as SerdeDeserialize, Serialize};
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

#[derive(Clone)]
struct AppState {
    role: ServiceRole,
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

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::args().any(|arg| arg == "--version" || arg == "-V") {
        println!("mem-service {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer().json())
        .init();

    let config_path = std::env::args().nth(1).map(PathBuf::from);
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
        let state = build_state(config.clone()).await?;
        let app = build_http_app(state.clone());
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
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
                    let _ = shutdown_tx.send(());
                    http_server.await.context("join mem-service task")??;
                    abort_tasks(&mut proto_tasks);
                    abort_tasks(&mut cluster_tasks);
                    break;
                }
                result = wait_for_config_change(path, config_fingerprint) => {
                    config_fingerprint = result.context("watch config file")?;
                    tracing::info!(path = %path.display(), "config changed; restarting backend");
                    let _ = shutdown_tx.send(());
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
                    let _ = shutdown_tx.send(());
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

async fn build_state(config: AppConfig) -> Result<AppState> {
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
        .route("/v1/query", post(query))
        .route("/v1/commits/sync", post(sync_commits))
        .route("/v1/capture/task", post(capture_task))
        .route("/v1/curate", post(curate_memory))
        .route("/v1/reindex", post(reindex))
        .route("/v1/memory/{id}", get(get_memory))
        .route("/v1/memory", delete(delete_memory))
        .route("/v1/stats", get(stats))
        .route("/v1/projects/{slug}/commits", get(project_commits))
        .route(
            "/v1/projects/{slug}/commits/{hash}",
            get(project_commit_detail),
        )
        .route("/v1/projects/{slug}/memories", get(project_memories))
        .route("/v1/projects/{slug}/overview", get(project_overview))
        .route("/v1/watchers/heartbeat", post(watcher_heartbeat))
        .route("/v1/watchers/unregister", post(watcher_unregister))
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

async fn start_cluster_tasks(state: AppState) -> Result<Vec<JoinHandle<Result<()>>>> {
    if !state.config.cluster.enabled {
        return Ok(Vec::new());
    }

    let socket = Arc::new(bind_cluster_socket(
        &state.config.cluster.discovery_multicast_addr,
    )?);
    let mut tasks = Vec::new();
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
    let tcp_listener = TcpListener::bind(&state.config.service.capnp_tcp_addr)
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
            "service_id": state.config.cluster.service_id,
            "version": env!("CARGO_PKG_VERSION")
        }))
    } else {
        let upstream = relay_upstream_health(state).await?;
        Ok(serde_json::json!({
            "status": if upstream.is_some() { "ok" } else { "degraded" },
            "role": "relay",
            "database": "down",
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
            writer_id: request.writer_id.clone(),
        }),
    );
    Ok(Json(response))
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
            "Curated {} capture(s) into {} memory entry/entries.",
            response.input_count, response.output_count
        ),
        Some(ActivityDetails::Curate {
            run_id: response.run_id,
            input_count: response.input_count,
            output_count: response.output_count,
        }),
    );
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

async fn watcher_heartbeat(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<WatcherHeartbeatRequest>,
) -> Result<Json<WatcherPresenceSummary>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/watchers/heartbeat", &request, true).await?,
        ));
    }
    let project = request.project.clone();
    let (summary, changed) = register_watcher_heartbeat(&state.watchers, request);
    if changed {
        notify_project_refreshed(&state, project);
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

async fn fetch_project_overview_with_watchers(
    state: &AppState,
    slug: &str,
) -> Result<ProjectOverviewResponse, sqlx::Error> {
    let pool = state
        .pool
        .as_ref()
        .expect("project overview requires a primary database pool");
    let mut overview = fetch_project_overview(pool, slug, &state.config.automation).await?;
    overview.watchers = Some(watcher_summary_for_project(&state.watchers, slug));
    Ok(overview)
}

fn register_watcher_heartbeat(
    watchers: &Mutex<HashMap<String, WatcherPresence>>,
    request: WatcherHeartbeatRequest,
) -> (WatcherPresenceSummary, bool) {
    let mut registry = watchers.lock().expect("watcher registry mutex poisoned");
    let before = watcher_summary_from_registry(&registry, &request.project);
    prune_stale_watchers(&mut registry);
    let now = chrono::Utc::now();
    registry
        .entry(request.watcher_id.clone())
        .and_modify(|watcher| {
            watcher.project = request.project.clone();
            watcher.repo_root = request.repo_root.clone();
            watcher.hostname = request.hostname.clone();
            watcher.pid = request.pid;
            watcher.mode = request.mode.clone();
            watcher.started_at = request.started_at;
            watcher.last_heartbeat_at = now;
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
        });
    let after = watcher_summary_from_registry(&registry, &request.project);
    let changed = before.active_count != after.active_count
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

fn unregister_watcher(
    watchers: &Mutex<HashMap<String, WatcherPresence>>,
    request: &WatcherUnregisterRequest,
) -> (WatcherPresenceSummary, bool) {
    let mut registry = watchers.lock().expect("watcher registry mutex poisoned");
    let before = watcher_summary_from_registry(&registry, &request.project);
    prune_stale_watchers(&mut registry);
    let removed = registry.remove(&request.watcher_id).is_some();
    let after = watcher_summary_from_registry(&registry, &request.project);
    let changed = removed
        || before.active_count != after.active_count
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
    prune_stale_watchers(&mut registry);
    watcher_summary_from_registry(&registry, project)
}

fn prune_stale_watchers(registry: &mut HashMap<String, WatcherPresence>) {
    let stale_after =
        chrono::Duration::from_std(StdDuration::from_secs(WATCHER_STALE_AFTER_SECONDS))
            .expect("valid watcher stale duration");
    let now = chrono::Utc::now();
    registry.retain(|_, watcher| now - watcher.last_heartbeat_at <= stale_after);
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
    WatcherPresenceSummary {
        active_count: watchers.len(),
        stale_after_seconds: WATCHER_STALE_AFTER_SECONDS,
        last_heartbeat_at,
        watchers,
    }
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
        };

        let (first, first_changed) = register_watcher_heartbeat(&watchers, request.clone());
        let (second, second_changed) = register_watcher_heartbeat(&watchers, request);

        assert_eq!(first.active_count, 1);
        assert_eq!(second.active_count, 1);
        assert_eq!(second.watchers.len(), 1);
        assert_eq!(second.watchers[0].watcher_id, "watcher-1");
        assert!(first_changed);
        assert!(!second_changed);
    }

    #[test]
    fn watcher_summary_filters_by_project_and_prunes_stale_entries() {
        let now = chrono::Utc::now();
        let mut registry = HashMap::new();
        registry.insert(
            "watcher-live".to_string(),
            WatcherPresence {
                watcher_id: "watcher-live".to_string(),
                project: "memory".to_string(),
                repo_root: "/repo".to_string(),
                hostname: "host-a".to_string(),
                pid: 111,
                mode: AutomationMode::Suggest,
                started_at: now,
                last_heartbeat_at: now,
            },
        );
        registry.insert(
            "watcher-other".to_string(),
            WatcherPresence {
                watcher_id: "watcher-other".to_string(),
                project: "other".to_string(),
                repo_root: "/other".to_string(),
                hostname: "host-b".to_string(),
                pid: 222,
                mode: AutomationMode::Auto,
                started_at: now,
                last_heartbeat_at: now,
            },
        );
        registry.insert(
            "watcher-stale".to_string(),
            WatcherPresence {
                watcher_id: "watcher-stale".to_string(),
                project: "memory".to_string(),
                repo_root: "/repo".to_string(),
                hostname: "host-c".to_string(),
                pid: 333,
                mode: AutomationMode::Suggest,
                started_at: now,
                last_heartbeat_at: now
                    - chrono::Duration::seconds(WATCHER_STALE_AFTER_SECONDS as i64 + 1),
            },
        );
        let watchers = Mutex::new(registry);

        let summary = watcher_summary_for_project(&watchers, "memory");

        assert_eq!(summary.active_count, 1);
        assert_eq!(summary.watchers.len(), 1);
        assert_eq!(summary.watchers[0].watcher_id, "watcher-live");
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
