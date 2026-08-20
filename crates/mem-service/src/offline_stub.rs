// SPDX-License-Identifier: AGPL-3.0-or-later

//! Compiled in place of `offline.rs` when the `offline` feature (bundled
//! DuckDB) is disabled. `build_offline_runtime` then always yields `None`,
//! so none of these methods can be reached at runtime.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use mem_record::{ActivityDetails, ActivityKind, CaptureTaskRequest, CaptureTaskResponse};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Clone)]
pub(crate) struct OfflineRuntime {
    pub(crate) store: OfflineStore,
    pub(crate) state: Arc<Mutex<OfflineSyncState>>,
}

impl OfflineRuntime {
    pub(crate) fn new(store: OfflineStore) -> Self {
        Self {
            store,
            state: Arc::new(Mutex::new(OfflineSyncState::default())),
        }
    }
}

#[derive(Clone)]
pub(crate) struct OfflineStore {
    _unconstructable: std::convert::Infallible,
}

#[derive(Clone, Default)]
pub(crate) struct OfflineSyncState {
    pub(crate) last_sync_at: Option<chrono::DateTime<chrono::Utc>>,
    pub(crate) last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct QueuedActivityEvent {
    pub(crate) event_id: Uuid,
    pub(crate) project: String,
    pub(crate) kind: ActivityKind,
    pub(crate) summary: String,
    pub(crate) details: Option<ActivityDetails>,
    pub(crate) recorded_at: chrono::DateTime<chrono::Utc>,
}

impl OfflineStore {
    pub(crate) async fn open(_path: PathBuf) -> Result<Self> {
        anyhow::bail!("this build of mem-service was compiled without the offline feature")
    }

    pub(crate) fn path(&self) -> &Path {
        unreachable!("offline store cannot exist without the offline feature")
    }

    pub(crate) async fn pending_count(&self) -> Result<u64> {
        unreachable!("offline store cannot exist without the offline feature")
    }

    pub(crate) async fn queue_capture(
        &self,
        _request: &CaptureTaskRequest,
    ) -> Result<CaptureTaskResponse> {
        unreachable!("offline store cannot exist without the offline feature")
    }

    pub(crate) async fn queue_activity(&self, _event: &QueuedActivityEvent) -> Result<Uuid> {
        unreachable!("offline store cannot exist without the offline feature")
    }
}

pub(crate) async fn sync_offline_batch(
    _pool: &PgPool,
    _offline: &OfflineRuntime,
    _batch_size: usize,
) -> Result<()> {
    unreachable!("offline runtime cannot exist without the offline feature")
}
