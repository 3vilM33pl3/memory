pub(crate) use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    fs,
    io::ErrorKind,
    io::Read,
    net::SocketAddr,
    path::{Path as FsPath, PathBuf},
    process::Command as ProcessCommand,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration as StdDuration, SystemTime},
};

#[cfg(target_vendor = "apple")]
pub(crate) use std::os::fd::AsRawFd;

pub(crate) use crate::repository::{
    fetch_project_commit, fetch_project_commits, fetch_project_memories, fetch_project_overview,
    parse_status_filter, preview_project_commit_sync, sync_project_commits,
};
pub(crate) use anyhow::{Context, Result};
pub(crate) use axum::{
    Json,
    body::Bytes,
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Response},
};
pub(crate) use futures_util::{SinkExt, StreamExt};
pub(crate) use mem_api::{
    ActivateEmbeddingBackendRequest, ActivityDetails, ActivityEvent, ActivityKind,
    ActivityListResponse, AppConfig, ArchiveRequest, ArchiveResponse, CaptureTaskRequest,
    CheckpointActivityRequest, CommitDetailResponse, CommitSyncRequest, CommitSyncResponse,
    CurateRequest, DeleteMemoryRequest, DeleteMemoryResponse, DiagnosticInfo, DiagnosticSeverity,
    EmbeddingBackendInfo, EmbeddingBackendsResponse, GraphActivityRequest, LlmAuditConfig,
    LlmAuditMessage, LlmAuditStatusResponse, MemoryEntryResponse, MemoryHistoryResponse,
    MemorySourceRecord, PlanActivityAction, PlanActivityRequest, ProjectCommitsResponse,
    ProjectMemoriesResponse, ProjectMemoryBundleEntry, ProjectMemoryBundleEntryRelation,
    ProjectMemoryBundleManifest, ProjectMemoryBundlePreview, ProjectMemoryBundleSource,
    ProjectMemoryExportOptions, ProjectMemoryImportPreview, ProjectMemoryImportResponse,
    ProjectMemoryListItem, ProjectOverviewResponse, ProvenanceVerificationRequest,
    ProvenanceVerificationResponse, PruneEmbeddingsRequest, PruneEmbeddingsResponse,
    PruneHistoryRequest, PruneHistoryResponse, QueryAnswerCitation, QueryAnswerGeneration,
    QueryAnswerMethod, QueryAnswerMode, QueryGraphConnection, QueryRequest, QueryResponse,
    ReembedRequest, ReembedResponse, ReindexRequest, ReindexResponse, RelatedMemorySummary,
    ReplacementPolicy, ReplacementPolicyRequest, ReplacementPolicyResponse,
    ReplacementProposalListResponse, ReplacementProposalResolutionResponse, ResumeAction,
    ResumeCheckpoint, ResumeRequest, ResumeResponse, ScanActivityRequest,
    SetEmbeddingCreationRequest, SetLlmAuditRequest, SourceKind, SourceProvenanceRecord,
    SourceProvenanceStatus, SourceProvenanceVerification, StatsResponse, StreamRequest,
    StreamResponse, TokenUsage, TokenUsageSummary, UpToSpeedRequest, UpToSpeedResponse,
    ValidationError, WatcherHealth, WatcherHeartbeatRequest, WatcherPresence,
    WatcherPresenceSummary, WatcherRestartRequest, WatcherRestartResponse,
    WatcherUnregisterRequest, effective_llm_base_url, is_supported_llm_provider,
    llm_max_output_tokens_field, llm_requires_api_key, load_repo_replacement_policy,
    read_capnp_text_frame, repo_agent_settings_path, resolve_llm_api_key, write_capnp_text_frame,
};
pub(crate) use mem_curate::{
    approve_replacement_proposal, curate, list_replacement_proposals, preview_capture,
    preview_curate, refresh_memory_relations, reject_replacement_proposal, store_capture,
};
pub(crate) use mem_platform::{
    managed_watch_service_name, preferred_user_state_dir, restart_local_watcher_service_name,
    watch_service_unit_name,
};
pub(crate) use mem_search::{
    EmbeddingRegistry, effective_embedding_base_url, parse_memory_type, parse_relation_type,
    parse_source_kind, prune_project_embeddings, query_memory_with_provenance_config,
    rebuild_chunks, rebuild_chunks_for_automatic_creation,
    rebuild_memory_chunks_for_automatic_creation, reembed_project_chunks,
};
pub(crate) use regex::Regex;
pub(crate) use serde::Deserialize;
pub(crate) use serde::{Deserialize as SerdeDeserialize, Serialize};
pub(crate) use sha2::{Digest, Sha256};
pub(crate) use socket2::{Domain, Protocol, Socket, Type};
pub(crate) use sqlx::{PgPool, Row, postgres::PgPoolOptions};
#[cfg(unix)]
pub(crate) use tokio::net::UnixListener;
pub(crate) use tokio::{
    net::{TcpListener, UdpSocket},
    sync::{broadcast, oneshot},
    task::JoinHandle,
    time::Duration,
};
pub(crate) use tokio_tungstenite::{connect_async, tungstenite::client::IntoClientRequest};
pub(crate) use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
pub(crate) use uuid::Uuid;
pub(crate) use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};
