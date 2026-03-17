use std::{net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use mem_api::{
    AppConfig, ArchiveRequest, ArchiveResponse, CaptureTaskRequest, CurateRequest,
    MemoryEntryResponse, MemorySourceRecord, MemoryTypeCount, ProjectMemoriesResponse,
    ProjectMemoryListItem, ProjectOverviewResponse, QueryRequest, ReindexRequest, ReindexResponse,
    SourceKindCount, StatsResponse, ValidationError,
};
use mem_curate::{curate, store_capture};
use mem_search::{parse_memory_type, parse_source_kind, query_memory, rebuild_chunks};
use serde::Deserialize;
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
        .route("/v1/projects/{slug}/memories", get(project_memories))
        .route("/v1/projects/{slug}/overview", get(project_overview))
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

    let total_row = sqlx::query(
        r#"
        SELECT COUNT(*) AS count
        FROM memory_entries m
        JOIN projects p ON p.id = m.project_id
        WHERE p.slug = $1
          AND ($2::text IS NULL OR m.status = $2)
        "#,
    )
    .bind(&slug)
    .bind(status_filter.as_deref())
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::sql)?;
    let total = total_row.try_get("count").map_err(ApiError::sql)?;

    let rows = sqlx::query(
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
            COUNT(DISTINCT mt.tag) AS tag_count,
            COUNT(DISTINCT ms.id) AS source_count
        FROM memory_entries m
        JOIN projects p ON p.id = m.project_id
        LEFT JOIN memory_tags mt ON mt.memory_entry_id = m.id
        LEFT JOIN memory_sources ms ON ms.memory_entry_id = m.id
        WHERE p.slug = $1
          AND ($2::text IS NULL OR m.status = $2)
        GROUP BY m.id
        ORDER BY m.updated_at DESC, m.id DESC
        LIMIT $3 OFFSET $4
        "#,
    )
    .bind(&slug)
    .bind(status_filter.as_deref())
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::sql)?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(ProjectMemoryListItem {
            id: row.try_get("id").map_err(ApiError::sql)?,
            summary: row.try_get("summary").map_err(ApiError::sql)?,
            preview: row.try_get("preview").map_err(ApiError::sql)?,
            memory_type: parse_memory_type(
                &row.try_get::<String, _>("memory_type")
                    .map_err(ApiError::sql)?,
            ),
            status: match row
                .try_get::<String, _>("status")
                .map_err(ApiError::sql)?
                .as_str()
            {
                "archived" => mem_api::MemoryStatus::Archived,
                _ => mem_api::MemoryStatus::Active,
            },
            confidence: row.try_get("confidence").map_err(ApiError::sql)?,
            importance: row.try_get("importance").map_err(ApiError::sql)?,
            updated_at: row.try_get("updated_at").map_err(ApiError::sql)?,
            tag_count: row.try_get("tag_count").map_err(ApiError::sql)?,
            source_count: row.try_get("source_count").map_err(ApiError::sql)?,
        });
    }

    Ok(Json(ProjectMemoriesResponse {
        project: slug,
        total,
        items,
    }))
}

async fn project_overview(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<ProjectOverviewResponse>, ApiError> {
    let row = sqlx::query(
        r#"
        SELECT
            p.slug,
            COUNT(DISTINCT m.id) AS memory_entries_total,
            COUNT(DISTINCT m.id) FILTER (WHERE m.status = 'active') AS active_memories,
            COUNT(DISTINCT m.id) FILTER (WHERE m.status = 'archived') AS archived_memories,
            COUNT(DISTINCT rc.id) AS raw_captures_total,
            COUNT(DISTINCT rc.id) FILTER (WHERE rc.curated_at IS NULL) AS uncurated_raw_captures,
            COUNT(DISTINCT t.id) AS tasks_total,
            COUNT(DISTINCT s.id) AS sessions_total,
            COUNT(DISTINCT cr.id) AS curation_runs_total,
            MAX(m.updated_at) AS last_memory_at,
            MAX(rc.created_at) AS last_capture_at,
            MAX(cr.created_at) AS last_curation_at
        FROM projects p
        LEFT JOIN memory_entries m ON m.project_id = p.id
        LEFT JOIN sessions s ON s.project_id = p.id
        LEFT JOIN tasks t ON t.session_id = s.id
        LEFT JOIN raw_captures rc ON rc.task_id = t.id
        LEFT JOIN curation_runs cr ON cr.project_id = p.id
        WHERE p.slug = $1
        GROUP BY p.slug
        "#,
    )
    .bind(&slug)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::sql)?;

    let Some(row) = row else {
        return Ok(Json(ProjectOverviewResponse {
            project: slug,
            service_status: "ok".to_string(),
            database_status: "up".to_string(),
            memory_entries_total: 0,
            active_memories: 0,
            archived_memories: 0,
            raw_captures_total: 0,
            uncurated_raw_captures: 0,
            tasks_total: 0,
            sessions_total: 0,
            curation_runs_total: 0,
            last_memory_at: None,
            last_capture_at: None,
            last_curation_at: None,
            memory_type_breakdown: Vec::new(),
            source_kind_breakdown: Vec::new(),
        }));
    };

    let memory_type_rows = sqlx::query(
        r#"
        SELECT m.memory_type, COUNT(*) AS count
        FROM memory_entries m
        JOIN projects p ON p.id = m.project_id
        WHERE p.slug = $1
        GROUP BY m.memory_type
        ORDER BY count DESC, m.memory_type ASC
        "#,
    )
    .bind(&slug)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::sql)?;

    let source_kind_rows = sqlx::query(
        r#"
        SELECT ms.source_kind, COUNT(*) AS count
        FROM memory_sources ms
        JOIN memory_entries m ON m.id = ms.memory_entry_id
        JOIN projects p ON p.id = m.project_id
        WHERE p.slug = $1
        GROUP BY ms.source_kind
        ORDER BY count DESC, ms.source_kind ASC
        "#,
    )
    .bind(&slug)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::sql)?;

    Ok(Json(ProjectOverviewResponse {
        project: slug,
        service_status: "ok".to_string(),
        database_status: "up".to_string(),
        memory_entries_total: row.try_get("memory_entries_total").map_err(ApiError::sql)?,
        active_memories: row.try_get("active_memories").map_err(ApiError::sql)?,
        archived_memories: row.try_get("archived_memories").map_err(ApiError::sql)?,
        raw_captures_total: row.try_get("raw_captures_total").map_err(ApiError::sql)?,
        uncurated_raw_captures: row
            .try_get("uncurated_raw_captures")
            .map_err(ApiError::sql)?,
        tasks_total: row.try_get("tasks_total").map_err(ApiError::sql)?,
        sessions_total: row.try_get("sessions_total").map_err(ApiError::sql)?,
        curation_runs_total: row.try_get("curation_runs_total").map_err(ApiError::sql)?,
        last_memory_at: row.try_get("last_memory_at").map_err(ApiError::sql)?,
        last_capture_at: row.try_get("last_capture_at").map_err(ApiError::sql)?,
        last_curation_at: row.try_get("last_curation_at").map_err(ApiError::sql)?,
        memory_type_breakdown: memory_type_rows
            .into_iter()
            .map(|row| {
                Ok(MemoryTypeCount {
                    memory_type: parse_memory_type(&row.try_get::<String, _>("memory_type")?),
                    count: row.try_get("count")?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()
            .map_err(ApiError::sql)?,
        source_kind_breakdown: source_kind_rows
            .into_iter()
            .map(|row| {
                Ok(SourceKindCount {
                    source_kind: parse_source_kind(&row.try_get::<String, _>("source_kind")?),
                    count: row.try_get("count")?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()
            .map_err(ApiError::sql)?,
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

fn parse_status_filter(input: &str) -> Result<String, ValidationError> {
    match input {
        "active" | "archived" => Ok(input.to_string()),
        _ => Err(ValidationError::new("status must be active or archived")),
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
