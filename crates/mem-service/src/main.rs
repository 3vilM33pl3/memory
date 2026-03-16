use std::{net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use mem_api::{
    AppConfig, ArchiveRequest, ArchiveResponse, CaptureTaskRequest, CurateRequest,
    MemoryEntryResponse, MemorySourceRecord, QueryRequest, ReindexRequest, ReindexResponse,
    StatsResponse, ValidationError,
};
use mem_curate::{curate, store_capture};
use mem_search::{parse_memory_type, parse_source_kind, query_memory, rebuild_chunks};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    pool: PgPool,
    api_token: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer().json())
        .init();

    let config_path = std::env::args().nth(1).map(PathBuf::from);
    let config = AppConfig::load_from_path(config_path).context("load config")?;
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database.url)
        .await
        .context("connect postgres")?;
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .context("run migrations")?;

    let state = AppState {
        pool,
        api_token: config.service.api_token.clone(),
    };

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/query", post(query))
        .route("/v1/capture/task", post(capture_task))
        .route("/v1/curate", post(curate_memory))
        .route("/v1/reindex", post(reindex))
        .route("/v1/memory/{id}", get(get_memory))
        .route("/v1/stats", get(stats))
        .route("/v1/archive", post(archive))
        .with_state(state)
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr = config
        .service
        .bind_addr
        .parse()
        .context("parse bind_addr")?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "memory-layer listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn healthz(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    sqlx::query("SELECT 1")
        .execute(&state.pool)
        .await
        .map_err(ApiError::sql)?;
    Ok(Json(serde_json::json!({
        "status": "ok",
        "database": "up"
    })))
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
    Ok(Json(ReindexResponse {
        reindexed_entries: count,
    }))
}

async fn get_memory(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<MemoryEntryResponse>, ApiError> {
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
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::sql)?
    .ok_or_else(|| ApiError::not_found("memory entry not found"))?;

    let tags = sqlx::query("SELECT tag FROM memory_tags WHERE memory_entry_id = $1 ORDER BY tag")
        .bind(id)
        .fetch_all(&state.pool)
        .await
        .map_err(ApiError::sql)?
        .into_iter()
        .map(|row| row.try_get::<String, _>("tag"))
        .collect::<Result<Vec<_>, _>>()
        .map_err(ApiError::sql)?;

    let sources = sqlx::query(
        r#"
        SELECT id, task_id, file_path, git_commit, source_kind, excerpt
        FROM memory_sources
        WHERE memory_entry_id = $1
        ORDER BY created_at ASC
        "#,
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::sql)?
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
    .collect::<Result<Vec<_>, sqlx::Error>>()
    .map_err(ApiError::sql)?;

    Ok(Json(MemoryEntryResponse {
        id,
        project: row.try_get("slug").map_err(ApiError::sql)?,
        canonical_text: row.try_get("canonical_text").map_err(ApiError::sql)?,
        summary: row.try_get("summary").map_err(ApiError::sql)?,
        memory_type: parse_memory_type(
            &row.try_get::<String, _>("memory_type")
                .map_err(ApiError::sql)?,
        ),
        importance: row.try_get("importance").map_err(ApiError::sql)?,
        confidence: row.try_get("confidence").map_err(ApiError::sql)?,
        status: match row
            .try_get::<String, _>("status")
            .map_err(ApiError::sql)?
            .as_str()
        {
            "archived" => mem_api::MemoryStatus::Archived,
            _ => mem_api::MemoryStatus::Active,
        },
        tags,
        sources,
        created_at: row.try_get("created_at").map_err(ApiError::sql)?,
        updated_at: row.try_get("updated_at").map_err(ApiError::sql)?,
    }))
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

    Ok(Json(ArchiveResponse {
        archived_count: result.rows_affected(),
    }))
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
