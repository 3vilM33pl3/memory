use crate::prelude::*;

use duckdb::{Connection, params};
use mem_api::{CaptureTaskResponse, OfflinePendingItem, OfflinePendingResponse};
use std::path::Path as StdPath;

#[derive(Clone, Debug)]
pub(crate) struct OfflineRuntime {
    pub(crate) store: OfflineStore,
    pub(crate) state: Arc<Mutex<OfflineSyncState>>,
}

#[derive(Clone, Debug)]
pub(crate) struct OfflineStore {
    path: Arc<PathBuf>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct OfflineSyncState {
    pub(crate) last_sync_at: Option<chrono::DateTime<chrono::Utc>>,
    pub(crate) last_error: Option<String>,
}

#[derive(Clone, Debug, Serialize, SerdeDeserialize)]
pub(crate) struct QueuedActivityEvent {
    pub(crate) event_id: Uuid,
    pub(crate) project: String,
    pub(crate) kind: ActivityKind,
    pub(crate) summary: String,
    pub(crate) details: Option<ActivityDetails>,
    pub(crate) recorded_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Clone, Debug)]
pub(crate) struct QueuedOutboxItem {
    pub(crate) queue_id: Uuid,
    pub(crate) item_kind: String,
    pub(crate) payload_json: String,
}

pub(crate) async fn sync_offline_batch(
    pool: &PgPool,
    offline: &OfflineRuntime,
    batch_size: usize,
) -> Result<()> {
    let items = offline.store.pending_batch(batch_size.max(1)).await?;
    for item in items {
        let result = match item.item_kind.as_str() {
            "capture_task" => sync_capture(pool, &item.payload_json).await,
            "activity_event" => sync_activity(pool, &item.payload_json).await,
            other => Err(anyhow::anyhow!("unknown offline item kind: {other}")),
        };
        match result {
            Ok(()) => offline.store.mark_synced(item.queue_id).await?,
            Err(error) => {
                let message = error.to_string();
                offline
                    .store
                    .mark_failed(item.queue_id, message.clone())
                    .await?;
                return Err(anyhow::anyhow!(message));
            }
        }
    }
    let mut state = offline
        .state
        .lock()
        .expect("offline sync state lock poisoned");
    state.last_sync_at = Some(chrono::Utc::now());
    state.last_error = None;
    Ok(())
}

async fn sync_capture(pool: &PgPool, payload_json: &str) -> Result<()> {
    let request: CaptureTaskRequest = serde_json::from_str(payload_json)?;
    store_capture(pool, &request).await?;
    Ok(())
}

async fn sync_activity(pool: &PgPool, payload_json: &str) -> Result<()> {
    let event: QueuedActivityEvent = serde_json::from_str(payload_json)?;
    let Some(project_id) = sqlx::query("SELECT id FROM projects WHERE slug = $1")
        .bind(&event.project)
        .fetch_optional(pool)
        .await?
        .and_then(|row| row.try_get::<Uuid, _>("id").ok())
    else {
        return Err(anyhow::anyhow!(
            "project {} does not exist for queued activity",
            event.project
        ));
    };
    sqlx::query(
        r#"
        INSERT INTO project_timeline_events (
            id, project_id, recorded_at, kind, memory_id, summary, details_json,
            source
        )
        VALUES ($1, $2, $3, $4, NULL, $5, $6, 'offline_sync')
        ON CONFLICT (id) DO NOTHING
        "#,
    )
    .bind(event.event_id)
    .bind(project_id)
    .bind(event.recorded_at)
    .bind(crate::repository::events::activity_kind_label(&event.kind))
    .bind(event.summary)
    .bind(event.details.map(sqlx::types::Json))
    .execute(pool)
    .await?;
    Ok(())
}

impl OfflineRuntime {
    pub(crate) fn new(store: OfflineStore) -> Self {
        Self {
            store,
            state: Arc::new(Mutex::new(OfflineSyncState::default())),
        }
    }
}

impl OfflineStore {
    pub(crate) async fn open(path: PathBuf) -> Result<Self> {
        let store = Self {
            path: Arc::new(path),
        };
        store.initialize().await?;
        Ok(store)
    }

    pub(crate) fn path(&self) -> &StdPath {
        self.path.as_ref().as_path()
    }

    async fn initialize(&self) -> Result<()> {
        let path = Arc::clone(&self.path);
        tokio::task::spawn_blocking(move || initialize_db(&path))
            .await
            .context("join offline db initialization")?
    }

    pub(crate) async fn queue_capture(
        &self,
        request: &CaptureTaskRequest,
    ) -> Result<CaptureTaskResponse> {
        let path = Arc::clone(&self.path);
        let request = request.clone();
        tokio::task::spawn_blocking(move || queue_capture_sync(&path, &request))
            .await
            .context("join offline capture queue")?
    }

    pub(crate) async fn queue_activity(&self, event: &QueuedActivityEvent) -> Result<Uuid> {
        let path = Arc::clone(&self.path);
        let event = event.clone();
        tokio::task::spawn_blocking(move || queue_activity_sync(&path, &event))
            .await
            .context("join offline activity queue")?
    }

    pub(crate) async fn pending_count(&self) -> Result<u64> {
        let path = Arc::clone(&self.path);
        tokio::task::spawn_blocking(move || pending_count_sync(&path))
            .await
            .context("join offline pending count")?
    }

    pub(crate) async fn pending_response(
        &self,
        project: Option<&str>,
        limit: usize,
    ) -> Result<OfflinePendingResponse> {
        let path = Arc::clone(&self.path);
        let project = project.map(ToOwned::to_owned);
        tokio::task::spawn_blocking(move || pending_response_sync(&path, project.as_deref(), limit))
            .await
            .context("join offline pending list")?
    }

    pub(crate) async fn pending_batch(&self, limit: usize) -> Result<Vec<QueuedOutboxItem>> {
        let path = Arc::clone(&self.path);
        tokio::task::spawn_blocking(move || pending_batch_sync(&path, limit))
            .await
            .context("join offline pending batch")?
    }

    pub(crate) async fn mark_synced(&self, queue_id: Uuid) -> Result<()> {
        let path = Arc::clone(&self.path);
        tokio::task::spawn_blocking(move || mark_synced_sync(&path, queue_id))
            .await
            .context("join offline mark synced")?
    }

    pub(crate) async fn mark_failed(&self, queue_id: Uuid, error: String) -> Result<()> {
        let path = Arc::clone(&self.path);
        tokio::task::spawn_blocking(move || mark_failed_sync(&path, queue_id, &error))
            .await
            .context("join offline mark failed")?
    }
}

fn initialize_db(path: &StdPath) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let conn = Connection::open(path).with_context(|| format!("open {}", path.display()))?;
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS offline_outbox (
            queue_id TEXT PRIMARY KEY,
            item_kind TEXT NOT NULL,
            project TEXT NOT NULL,
            summary TEXT,
            idempotency_key TEXT,
            payload_json TEXT NOT NULL,
            response_json TEXT,
            status TEXT NOT NULL DEFAULT 'pending',
            attempt_count UBIGINT NOT NULL DEFAULT 0,
            last_error TEXT,
            created_at TEXT NOT NULL,
            synced_at TEXT
        );
        CREATE INDEX IF NOT EXISTS offline_outbox_status_created_idx
            ON offline_outbox(status, created_at);
        CREATE INDEX IF NOT EXISTS offline_outbox_project_status_idx
            ON offline_outbox(project, status);
        "#,
    )?;
    Ok(())
}

fn open_initialized(path: &StdPath) -> Result<Connection> {
    initialize_db(path)?;
    Connection::open(path).with_context(|| format!("open {}", path.display()))
}

fn queue_capture_sync(path: &StdPath, request: &CaptureTaskRequest) -> Result<CaptureTaskResponse> {
    let conn = open_initialized(path)?;
    let idempotency_key = mem_ingest::idempotency_key(request);
    if let Some(existing) = queued_capture_response(&conn, &idempotency_key)? {
        return Ok(existing);
    }

    let queue_id = Uuid::new_v4();
    let response = CaptureTaskResponse {
        project_id: Uuid::nil(),
        session_id: Uuid::new_v4(),
        task_id: Uuid::new_v4(),
        raw_capture_id: Uuid::new_v4(),
        idempotency_key: idempotency_key.clone(),
        dry_run: false,
        queued_offline: true,
        offline_queue_id: Some(queue_id),
        offline_message: Some("queued locally; PostgreSQL sync is pending".to_string()),
    };
    let payload_json = serde_json::to_string(request)?;
    let response_json = serde_json::to_string(&response)?;
    conn.execute(
        r#"
        INSERT INTO offline_outbox (
            queue_id, item_kind, project, summary, idempotency_key, payload_json,
            response_json, status, created_at
        )
        VALUES (?1, 'capture_task', ?2, ?3, ?4, ?5, ?6, 'pending', ?7)
        "#,
        params![
            queue_id.to_string(),
            request.project,
            request.task_title,
            idempotency_key,
            payload_json,
            response_json,
            chrono::Utc::now().to_rfc3339(),
        ],
    )?;
    Ok(response)
}

fn queued_capture_response(
    conn: &Connection,
    idempotency_key: &str,
) -> Result<Option<CaptureTaskResponse>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT response_json
        FROM offline_outbox
        WHERE item_kind = 'capture_task'
          AND idempotency_key = ?1
          AND status = 'pending'
        ORDER BY created_at ASC
        LIMIT 1
        "#,
    )?;
    let mut rows = stmt.query(params![idempotency_key])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let response_json: String = row.get(0)?;
    Ok(Some(serde_json::from_str(&response_json)?))
}

fn queue_activity_sync(path: &StdPath, event: &QueuedActivityEvent) -> Result<Uuid> {
    let conn = open_initialized(path)?;
    let queue_id = Uuid::new_v4();
    let payload_json = serde_json::to_string(event)?;
    conn.execute(
        r#"
        INSERT INTO offline_outbox (
            queue_id, item_kind, project, summary, payload_json, status, created_at
        )
        VALUES (?1, 'activity_event', ?2, ?3, ?4, 'pending', ?5)
        "#,
        params![
            queue_id.to_string(),
            event.project,
            event.summary,
            payload_json,
            event.recorded_at.to_rfc3339(),
        ],
    )?;
    Ok(queue_id)
}

fn pending_count_sync(path: &StdPath) -> Result<u64> {
    let conn = open_initialized(path)?;
    let count: u64 = conn.query_row(
        "SELECT COUNT(*) FROM offline_outbox WHERE status = 'pending'",
        [],
        |row| row.get(0),
    )?;
    Ok(count)
}

fn pending_response_sync(
    path: &StdPath,
    project: Option<&str>,
    limit: usize,
) -> Result<OfflinePendingResponse> {
    let conn = open_initialized(path)?;
    let pending_count = pending_count_sync(path)?;
    let sql = if project.is_some() {
        r#"
        SELECT queue_id, item_kind, project, summary, idempotency_key, created_at,
               attempt_count, last_error
        FROM offline_outbox
        WHERE status = 'pending' AND project = ?1
        ORDER BY created_at ASC
        LIMIT ?2
        "#
    } else {
        r#"
        SELECT queue_id, item_kind, project, summary, idempotency_key, created_at,
               attempt_count, last_error
        FROM offline_outbox
        WHERE status = 'pending'
        ORDER BY created_at ASC
        LIMIT ?1
        "#
    };
    let mut stmt = conn.prepare(sql)?;
    let mut items = Vec::new();
    if let Some(project) = project {
        let mut rows = stmt.query(params![project, limit as u64])?;
        while let Some(row) = rows.next()? {
            items.push(row_to_pending_item(row)?);
        }
    } else {
        let mut rows = stmt.query(params![limit as u64])?;
        while let Some(row) = rows.next()? {
            items.push(row_to_pending_item(row)?);
        }
    }
    Ok(OfflinePendingResponse {
        enabled: true,
        database_path: Some(path.display().to_string()),
        pending_count,
        items,
    })
}

fn row_to_pending_item(row: &duckdb::Row<'_>) -> Result<OfflinePendingItem> {
    let queue_id: String = row.get(0)?;
    let created_at: String = row.get(5)?;
    let attempt_count: u64 = row.get(6)?;
    Ok(OfflinePendingItem {
        queue_id: Uuid::parse_str(&queue_id)?,
        item_kind: row.get(1)?,
        project: row.get(2)?,
        summary: row.get(3)?,
        idempotency_key: row.get(4)?,
        created_at: chrono::DateTime::parse_from_rfc3339(&created_at)?.with_timezone(&chrono::Utc),
        attempt_count,
        last_error: row.get(7)?,
    })
}

fn pending_batch_sync(path: &StdPath, limit: usize) -> Result<Vec<QueuedOutboxItem>> {
    let conn = open_initialized(path)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT queue_id, item_kind, payload_json
        FROM offline_outbox
        WHERE status = 'pending'
        ORDER BY created_at ASC
        LIMIT ?1
        "#,
    )?;
    let mut rows = stmt.query(params![limit as u64])?;
    let mut items = Vec::new();
    while let Some(row) = rows.next()? {
        let queue_id: String = row.get(0)?;
        items.push(QueuedOutboxItem {
            queue_id: Uuid::parse_str(&queue_id)?,
            item_kind: row.get(1)?,
            payload_json: row.get(2)?,
        });
    }
    Ok(items)
}

fn mark_synced_sync(path: &StdPath, queue_id: Uuid) -> Result<()> {
    let conn = open_initialized(path)?;
    conn.execute(
        r#"
        UPDATE offline_outbox
        SET status = 'synced', synced_at = ?2, last_error = NULL
        WHERE queue_id = ?1
        "#,
        params![queue_id.to_string(), chrono::Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

fn mark_failed_sync(path: &StdPath, queue_id: Uuid, error: &str) -> Result<()> {
    let conn = open_initialized(path)?;
    conn.execute(
        r#"
        UPDATE offline_outbox
        SET attempt_count = attempt_count + 1, last_error = ?2
        WHERE queue_id = ?1
        "#,
        params![queue_id.to_string(), error],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("memory-{name}-{}.duckdb", Uuid::new_v4()))
    }

    fn capture_request() -> CaptureTaskRequest {
        CaptureTaskRequest {
            project: "memory".to_string(),
            task_title: "Offline capture".to_string(),
            user_prompt: "keep working while postgres is down".to_string(),
            writer_id: "test-writer".to_string(),
            writer_name: Some("Test Writer".to_string()),
            agent_summary: "queued a capture locally".to_string(),
            files_changed: vec!["src/lib.rs".to_string()],
            git_diff_summary: None,
            tests: Vec::new(),
            notes: vec!["offline note".to_string()],
            structured_candidates: Vec::new(),
            command_output: None,
            idempotency_key: Some("offline-capture-key".to_string()),
            dry_run: false,
        }
    }

    #[tokio::test]
    async fn queues_capture_idempotently_and_lists_pending() {
        let path = temp_db_path("offline-capture");
        let store = OfflineStore::open(path.clone()).await.unwrap();
        let request = capture_request();

        let first = store.queue_capture(&request).await.unwrap();
        let second = store.queue_capture(&request).await.unwrap();
        let pending = store.pending_response(Some("memory"), 10).await.unwrap();

        assert!(first.queued_offline);
        assert_eq!(first.offline_queue_id, second.offline_queue_id);
        assert_eq!(pending.pending_count, 1);
        assert_eq!(pending.items.len(), 1);
        assert_eq!(pending.items[0].item_kind, "capture_task");
        assert_eq!(
            pending.items[0].idempotency_key.as_deref(),
            Some("offline-capture-key")
        );

        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn queues_activity_and_marks_synced() {
        let path = temp_db_path("offline-activity");
        let store = OfflineStore::open(path.clone()).await.unwrap();
        let event = QueuedActivityEvent {
            event_id: Uuid::new_v4(),
            project: "memory".to_string(),
            kind: ActivityKind::Checkpoint,
            summary: "Saved checkpoint".to_string(),
            details: None,
            recorded_at: chrono::Utc::now(),
        };

        let queue_id = store.queue_activity(&event).await.unwrap();
        assert_eq!(store.pending_count().await.unwrap(), 1);
        let batch = store.pending_batch(10).await.unwrap();
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].queue_id, queue_id);

        store.mark_synced(queue_id).await.unwrap();
        assert_eq!(store.pending_count().await.unwrap(), 0);

        let _ = fs::remove_file(path);
    }
}
