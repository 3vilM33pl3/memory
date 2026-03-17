use std::{
    net::SocketAddr,
    path::{Path as FsPath, PathBuf},
    time::SystemTime,
};

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use mem_api::{
    AppConfig, ArchiveRequest, ArchiveResponse, CaptureTaskRequest, CurateRequest,
    DeleteMemoryRequest, DeleteMemoryResponse, MemoryEntryResponse, MemorySourceRecord,
    ProjectMemoriesResponse, ProjectOverviewResponse, QueryRequest, ReindexRequest,
    ReindexResponse, StatsResponse, StreamRequest, StreamResponse, ValidationError,
    read_capnp_text_frame, write_capnp_text_frame,
};
use mem_curate::{curate, store_capture};
use mem_search::{parse_memory_type, parse_source_kind, query_memory, rebuild_chunks};
use mem_service::{fetch_project_memories, fetch_project_overview, parse_status_filter};
use serde::Deserialize;
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use tokio::{
    net::{TcpListener, UnixListener},
    sync::{broadcast, oneshot},
    time::Duration,
};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    pool: PgPool,
    api_token: String,
    config: AppConfig,
    events: broadcast::Sender<ServiceEvent>,
}

#[derive(Clone, Debug)]
struct ServiceEvent {
    project: String,
    memory_id: Option<Uuid>,
}

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
        let proto_servers = start_proto_servers(state.clone()).await?;
        let mut proto_unix =
            tokio::spawn(run_proto_unix(proto_servers.unix_listener, state.clone()));
        let mut proto_tcp = tokio::spawn(run_proto_tcp(proto_servers.tcp_listener, state.clone()));

        tracing::info!(
            %addr,
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
                result = &mut proto_unix => {
                    result.context("join capnp unix task")??;
                    break;
                }
                result = &mut proto_tcp => {
                    result.context("join capnp tcp task")??;
                    break;
                }
                result = tokio::signal::ctrl_c() => {
                    result.context("listen for ctrl-c")?;
                    let _ = shutdown_tx.send(());
                    http_server.await.context("join mem-service task")??;
                    proto_unix.abort();
                    proto_tcp.abort();
                    break;
                }
                result = wait_for_config_change(path, config_fingerprint) => {
                    config_fingerprint = result.context("watch config file")?;
                    tracing::info!(path = %path.display(), "config changed; restarting backend");
                    let _ = shutdown_tx.send(());
                    http_server.await.context("join mem-service task")??;
                    proto_unix.abort();
                    proto_tcp.abort();
                }
            }
        } else {
            tokio::select! {
                result = &mut http_server => {
                    result.context("join mem-service task")??;
                    break;
                }
                result = &mut proto_unix => {
                    result.context("join capnp unix task")??;
                    break;
                }
                result = &mut proto_tcp => {
                    result.context("join capnp tcp task")??;
                    break;
                }
                result = tokio::signal::ctrl_c() => {
                    result.context("listen for ctrl-c")?;
                    let _ = shutdown_tx.send(());
                    http_server.await.context("join mem-service task")??;
                    proto_unix.abort();
                    proto_tcp.abort();
                    break;
                }
            }
        }
    }

    Ok(())
}

async fn build_state(config: AppConfig) -> Result<AppState> {
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database.url)
        .await
        .context("connect postgres")?;
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .context("run migrations")?;
    let (events, _) = broadcast::channel(128);

    Ok(AppState {
        pool,
        api_token: config.service.api_token.clone(),
        config,
        events,
    })
}

fn build_http_app(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/query", post(query))
        .route("/v1/capture/task", post(capture_task))
        .route("/v1/curate", post(curate_memory))
        .route("/v1/reindex", post(reindex))
        .route("/v1/memory/{id}", get(get_memory))
        .route("/v1/memory", delete(delete_memory))
        .route("/v1/stats", get(stats))
        .route("/v1/projects/{slug}/memories", get(project_memories))
        .route("/v1/projects/{slug}/overview", get(project_overview))
        .route("/v1/archive", post(archive))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
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
                if let Some(response) = render_subscription_update(&state, &subscriptions, &event).await? {
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
    let mut responses = Vec::new();
    match request {
        StreamRequest::Health => responses.push(StreamResponse::Health {
            value: health_payload(state).await?,
        }),
        StreamRequest::ProjectOverview { project } => {
            responses.push(StreamResponse::ProjectOverview {
                value: fetch_project_overview(&state.pool, &project, &state.config.automation)
                    .await?,
            });
        }
        StreamRequest::ProjectMemories { project } => {
            responses.push(StreamResponse::ProjectMemories {
                value: fetch_project_memories(&state.pool, &project, None, 500, 0).await?,
            });
        }
        StreamRequest::MemoryDetail { memory_id } => {
            responses.push(StreamResponse::MemoryDetail {
                value: fetch_memory_entry(&state.pool, memory_id).await?,
            });
        }
        StreamRequest::SubscribeProject { project } => {
            subscriptions.project = Some(project.clone());
            let overview =
                fetch_project_overview(&state.pool, &project, &state.config.automation).await?;
            let memories = fetch_project_memories(&state.pool, &project, None, 500, 0).await?;
            responses.push(StreamResponse::ProjectSnapshot { overview, memories });
        }
        StreamRequest::SubscribeMemory { memory_id } => {
            subscriptions.memory_id = Some(memory_id);
            let detail = fetch_memory_entry(&state.pool, memory_id).await?;
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

async fn render_subscription_update(
    state: &AppState,
    subscriptions: &ConnectionSubscriptions,
    event: &ServiceEvent,
) -> Result<Option<StreamResponse>> {
    if let Some(project) = &subscriptions.project {
        if project == &event.project {
            let overview =
                fetch_project_overview(&state.pool, project, &state.config.automation).await?;
            let memories = fetch_project_memories(&state.pool, project, None, 500, 0).await?;
            return Ok(Some(StreamResponse::ProjectChanged { overview, memories }));
        }
    }

    if let Some(memory_id) = subscriptions.memory_id {
        if event.memory_id == Some(memory_id) {
            let detail = fetch_memory_entry(&state.pool, memory_id).await?;
            return Ok(Some(StreamResponse::MemoryChanged { detail }));
        }
    }

    Ok(None)
}

async fn health_payload(state: &AppState) -> Result<serde_json::Value> {
    sqlx::query("SELECT 1").execute(&state.pool).await?;
    Ok(serde_json::json!({
        "status": "ok",
        "database": "up",
        "version": env!("CARGO_PKG_VERSION")
    }))
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

async fn query(
    State(state): State<AppState>,
    Json(request): Json<QueryRequest>,
) -> Result<Json<mem_api::QueryResponse>, ApiError> {
    request.validate().map_err(ApiError::validation)?;
    let response = query_memory(&state.pool, &request)
        .await
        .map_err(ApiError::sql)?;
    Ok(Json(response))
}

async fn capture_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CaptureTaskRequest>,
) -> Result<Json<mem_api::CaptureTaskResponse>, ApiError> {
    require_token(&headers, &state.api_token)?;
    request.validate().map_err(ApiError::validation)?;
    let response = store_capture(&state.pool, &request)
        .await
        .map_err(ApiError::sql)?;
    notify_project_changed(&state, request.project, None);
    Ok(Json(response))
}

async fn curate_memory(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CurateRequest>,
) -> Result<Json<mem_api::CurateResponse>, ApiError> {
    require_token(&headers, &state.api_token)?;
    request.validate().map_err(ApiError::validation)?;
    let response = curate(&state.pool, &request).await.map_err(ApiError::sql)?;
    notify_project_changed(&state, request.project, None);
    Ok(Json(response))
}

async fn reindex(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ReindexRequest>,
) -> Result<Json<ReindexResponse>, ApiError> {
    require_token(&headers, &state.api_token)?;
    request.validate().map_err(ApiError::validation)?;
    let count = rebuild_chunks(&state.pool, &request.project)
        .await
        .map_err(ApiError::sql)?;
    notify_project_changed(&state, request.project, None);
    Ok(Json(ReindexResponse {
        reindexed_entries: count,
    }))
}

async fn get_memory(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<MemoryEntryResponse>, ApiError> {
    let detail = fetch_memory_entry(&state.pool, id)
        .await
        .map_err(ApiError::sql)?
        .ok_or_else(|| ApiError::not_found("memory entry not found"))?;
    Ok(Json(detail))
}

async fn stats(State(state): State<AppState>) -> Result<Json<StatsResponse>, ApiError> {
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
            .fetch_one(&state.pool)
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

#[derive(Debug, Default, Deserialize)]
struct ProjectMemoriesParams {
    status: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn project_memories(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(params): Query<ProjectMemoriesParams>,
) -> Result<Json<ProjectMemoriesResponse>, ApiError> {
    let limit = params.limit.unwrap_or(200).clamp(1, 500);
    let offset = params.offset.unwrap_or(0).max(0);
    let status_filter = params
        .status
        .as_deref()
        .map(parse_status_filter)
        .transpose()
        .map_err(ApiError::validation)?;

    Ok(Json(
        fetch_project_memories(&state.pool, &slug, status_filter.as_deref(), limit, offset)
            .await
            .map_err(ApiError::sql)?,
    ))
}

async fn project_overview(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<ProjectOverviewResponse>, ApiError> {
    Ok(Json(
        fetch_project_overview(&state.pool, &slug, &state.config.automation)
            .await
            .map_err(ApiError::sql)?,
    ))
}

async fn archive(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ArchiveRequest>,
) -> Result<Json<ArchiveResponse>, ApiError> {
    require_token(&headers, &state.api_token)?;
    request.validate().map_err(ApiError::validation)?;
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
    .execute(&state.pool)
    .await
    .map_err(ApiError::sql)?;
    notify_project_changed(&state, request.project, None);

    Ok(Json(ArchiveResponse {
        archived_count: result.rows_affected(),
    }))
}

async fn delete_memory(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<DeleteMemoryRequest>,
) -> Result<Json<DeleteMemoryResponse>, ApiError> {
    require_token(&headers, &state.api_token)?;
    request.validate().map_err(ApiError::validation)?;

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
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::sql)?
    .ok_or_else(|| ApiError::not_found("memory entry not found"))?;

    let memory_id = record.try_get("id").map_err(ApiError::sql)?;
    let project: String = record.try_get("slug").map_err(ApiError::sql)?;
    let summary: String = record.try_get("summary").map_err(ApiError::sql)?;
    notify_project_changed(&state, project.clone(), Some(memory_id));

    Ok(Json(DeleteMemoryResponse {
        memory_id,
        project,
        summary,
        deleted: true,
    }))
}

fn notify_project_changed(state: &AppState, project: String, memory_id: Option<Uuid>) {
    let _ = state.events.send(ServiceEvent { project, memory_id });
}

fn require_token(headers: &HeaderMap, expected: &str) -> Result<(), ApiError> {
    let provided = headers
        .get("x-api-token")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::unauthorized("missing x-api-token header"))?;
    if provided != expected {
        return Err(ApiError::unauthorized("invalid api token"));
    }
    Ok(())
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

    fn sql(error: sqlx::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: error.to_string(),
        }
    }

    fn io(error: anyhow::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: error.to_string(),
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
