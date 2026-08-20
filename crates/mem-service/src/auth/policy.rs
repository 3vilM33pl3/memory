// SPDX-License-Identifier: AGPL-3.0-or-later

use axum::{
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use mem_record::Permission;
use serde_json::Value;
use sqlx::Row;
use uuid::Uuid;

use super::{
    AuthenticatedPrincipal, CSRF_COOKIE_NAME, CookiePolicy, CredentialSource, cookie_value,
    hash_secret, resolve_request_principal,
};
use crate::{ApiError, AppState};

const MAX_AUTH_INSPECTION_BODY: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProjectScope {
    Public,
    Unscoped,
    Global,
    PathProject,
    BodyProject,
    BodyMemoryResource,
    QueryProjectOrGlobal,
    MemoryResource,
    ValidationResource,
    LoopRunResource,
    LoopApprovalResource,
    LoopProposalResource,
    WorkspaceResource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RoutePolicy {
    pub(crate) permission: Permission,
    pub(crate) scope: ProjectScope,
    /// Allowed in read-only (student) mode even though the method mutates:
    /// queries, resume/up-to-speed briefings, and bundle exports are
    /// semantic reads carried over POST.
    pub(crate) semantic_read: bool,
}

/// The single authoritative route -> policy table. Registered together with
/// the axum routes by `PolicyRouter`; a path or method with no entry is
/// DENIED by the authorization guard (fail closed).
pub(crate) fn build_policy_table() -> matchit::Router<Vec<(Method, RoutePolicy)>> {
    let mut table = matchit::Router::new();
    let entries: &[(&str, &[(Method, RoutePolicy)])] = &[
        (
            "/healthz",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::Public,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/openapi.yaml",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::Public,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/ws",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::Unscoped,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/web/auth-token",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::Public,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/auth/login",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::Public,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/auth/callback",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::Public,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/auth/me",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::Unscoped,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/auth/logout",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::Unscoped,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/auth/tokens",
            &[
                (
                    Method::GET,
                    RoutePolicy {
                        permission: Permission::AuthManage,
                        scope: ProjectScope::Global,
                        semantic_read: false,
                    },
                ),
                (
                    Method::POST,
                    RoutePolicy {
                        permission: Permission::AuthManage,
                        scope: ProjectScope::Global,
                        semantic_read: false,
                    },
                ),
            ],
        ),
        (
            "/v1/auth/tokens/{p}/revoke",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::AuthManage,
                    scope: ProjectScope::Global,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/auth/principals",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::AuthManage,
                    scope: ProjectScope::Global,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/auth/memberships",
            &[
                (
                    Method::GET,
                    RoutePolicy {
                        permission: Permission::AuthManage,
                        scope: ProjectScope::Global,
                        semantic_read: false,
                    },
                ),
                (
                    Method::POST,
                    RoutePolicy {
                        permission: Permission::AuthManage,
                        scope: ProjectScope::Global,
                        semantic_read: false,
                    },
                ),
            ],
        ),
        (
            "/v1/auth/memberships/{p}",
            &[(
                Method::DELETE,
                RoutePolicy {
                    permission: Permission::AuthManage,
                    scope: ProjectScope::Global,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/admin/shutdown",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::SystemAdmin,
                    scope: ProjectScope::Global,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/runtime/status",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::QueryProjectOrGlobal,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/skills",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::Unscoped,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/skills/repair",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::SystemAdmin,
                    scope: ProjectScope::Global,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/skills/{p}",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::Unscoped,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/query",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::BodyProject,
                    semantic_read: true,
                },
            )],
        ),
        (
            "/v1/query/global",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::Unscoped,
                    semantic_read: true,
                },
            )],
        ),
        (
            "/v1/checkpoint/activity",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::ActivityCapture,
                    scope: ProjectScope::BodyProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/plan/activity",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::ActivityCapture,
                    scope: ProjectScope::BodyProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/scan/activity",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::ActivityCapture,
                    scope: ProjectScope::BodyProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/graph/activity",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::ActivityCapture,
                    scope: ProjectScope::BodyProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/commits/sync",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::ActivityCapture,
                    scope: ProjectScope::BodyProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/capture/task",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::ActivityCapture,
                    scope: ProjectScope::BodyProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/curate",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::MemoryCurate,
                    scope: ProjectScope::BodyProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/provenance/verify",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::MemoryCurate,
                    scope: ProjectScope::BodyProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/reindex",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::MemoryCurate,
                    scope: ProjectScope::BodyProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/reembed",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::MemoryCurate,
                    scope: ProjectScope::BodyProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/prune-embeddings",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::MemoryCurate,
                    scope: ProjectScope::BodyProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/embeddings/backends",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::Unscoped,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/embeddings/activate",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::EmbeddingsManage,
                    scope: ProjectScope::Global,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/embeddings/deactivate",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::EmbeddingsManage,
                    scope: ProjectScope::Global,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/embeddings/create-enabled",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::EmbeddingsManage,
                    scope: ProjectScope::Global,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/config/llm-audit",
            &[
                (
                    Method::GET,
                    RoutePolicy {
                        permission: Permission::MemoryRead,
                        scope: ProjectScope::Unscoped,
                        semantic_read: false,
                    },
                ),
                (
                    Method::POST,
                    RoutePolicy {
                        permission: Permission::SystemAdmin,
                        scope: ProjectScope::Global,
                        semantic_read: false,
                    },
                ),
            ],
        ),
        (
            "/v1/loops",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::Unscoped,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/loops/global-kill-switch",
            &[
                (
                    Method::GET,
                    RoutePolicy {
                        permission: Permission::MemoryRead,
                        scope: ProjectScope::Unscoped,
                        semantic_read: false,
                    },
                ),
                (
                    Method::POST,
                    RoutePolicy {
                        permission: Permission::LoopsConfigure,
                        scope: ProjectScope::Global,
                        semantic_read: false,
                    },
                ),
            ],
        ),
        (
            "/v1/loops/runs",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::QueryProjectOrGlobal,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/loops/runs/{p}",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::LoopRunResource,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/loops/runs/{p}/context-pack",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::LoopRunResource,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/loops/runs/{p}/cancel",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::LoopsRun,
                    scope: ProjectScope::LoopRunResource,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/loops/runs/{p}/feedback",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::LoopsRun,
                    scope: ProjectScope::LoopRunResource,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/loops/approvals",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::QueryProjectOrGlobal,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/loops/memory-proposals",
            &[
                (
                    Method::GET,
                    RoutePolicy {
                        permission: Permission::MemoryRead,
                        scope: ProjectScope::QueryProjectOrGlobal,
                        semantic_read: false,
                    },
                ),
                (
                    Method::POST,
                    RoutePolicy {
                        permission: Permission::LoopsRun,
                        scope: ProjectScope::QueryProjectOrGlobal,
                        semantic_read: false,
                    },
                ),
            ],
        ),
        (
            "/v1/loops/triggers/route",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::LoopsRun,
                    scope: ProjectScope::BodyProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/loops/approvals/{p}/approve",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::LoopsRun,
                    scope: ProjectScope::LoopApprovalResource,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/loops/approvals/{p}/reject",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::LoopsRun,
                    scope: ProjectScope::LoopApprovalResource,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/loops/approvals/{p}/edit",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::LoopsRun,
                    scope: ProjectScope::LoopApprovalResource,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/loops/memory-proposals/{p}/approve",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::LoopsRun,
                    scope: ProjectScope::LoopProposalResource,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/loops/memory-proposals/{p}/reject",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::LoopsRun,
                    scope: ProjectScope::LoopProposalResource,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/loops/memory-proposals/{p}/edit",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::LoopsRun,
                    scope: ProjectScope::LoopProposalResource,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/loops/{p}",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::QueryProjectOrGlobal,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/loops/{p}/enable",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::LoopsConfigure,
                    scope: ProjectScope::BodyProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/loops/{p}/disable",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::LoopsConfigure,
                    scope: ProjectScope::BodyProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/loops/{p}/pause",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::LoopsConfigure,
                    scope: ProjectScope::BodyProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/loops/{p}/snooze",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::LoopsConfigure,
                    scope: ProjectScope::BodyProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/loops/{p}/run",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::LoopsRun,
                    scope: ProjectScope::BodyProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/loops/{p}/context-pack",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::QueryProjectOrGlobal,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/memory/{p}",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::MemoryResource,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/memory/{p}/validate",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::MemoryCurate,
                    scope: ProjectScope::MemoryResource,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/validation-runs/{p}/review",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::MemoryCurate,
                    scope: ProjectScope::ValidationResource,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/memory/{p}/archive",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::MemoryCurate,
                    scope: ProjectScope::MemoryResource,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/memory/{p}/history",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::MemoryResource,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/memory",
            &[(
                Method::DELETE,
                RoutePolicy {
                    permission: Permission::MemoryDelete,
                    scope: ProjectScope::BodyMemoryResource,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/prune-history",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::MemoryCurate,
                    scope: ProjectScope::BodyProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/projects/{p}/memory-scores",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::PathProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/projects/{p}/structure",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::PathProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/projects/{p}/validation-runs",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::PathProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/projects/{p}/commits",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::PathProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/projects/{p}/commits/{p}",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::PathProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/projects/{p}/bundle/export/preview",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::BundleExport,
                    scope: ProjectScope::PathProject,
                    semantic_read: true,
                },
            )],
        ),
        (
            "/v1/projects/{p}/bundle/export",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::BundleExport,
                    scope: ProjectScope::PathProject,
                    semantic_read: true,
                },
            )],
        ),
        (
            "/v1/projects/{p}/bundle/import/preview",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::BundleImport,
                    scope: ProjectScope::PathProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/projects/{p}/bundle/import",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::BundleImport,
                    scope: ProjectScope::PathProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/projects/{p}/replacement-proposals",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::PathProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/projects/{p}/replacement-proposals/{p}/approve",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::MemoryCurate,
                    scope: ProjectScope::PathProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/projects/{p}/replacement-proposals/{p}/reject",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::MemoryCurate,
                    scope: ProjectScope::PathProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/projects/{p}/replacement-policy",
            &[
                (
                    Method::GET,
                    RoutePolicy {
                        permission: Permission::MemoryRead,
                        scope: ProjectScope::PathProject,
                        semantic_read: false,
                    },
                ),
                (
                    Method::POST,
                    RoutePolicy {
                        permission: Permission::MemoryCurate,
                        scope: ProjectScope::PathProject,
                        semantic_read: false,
                    },
                ),
                (
                    Method::PUT,
                    RoutePolicy {
                        permission: Permission::MemoryCurate,
                        scope: ProjectScope::PathProject,
                        semantic_read: false,
                    },
                ),
            ],
        ),
        (
            "/v1/projects/{p}/memories",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::PathProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/projects/{p}/memory-graph",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::PathProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/projects/{p}/overview",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::PathProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/projects/{p}/graph/status",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::PathProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/projects/{p}/graph/extract",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::MemoryCurate,
                    scope: ProjectScope::PathProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/projects/{p}/graph",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::PathProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/projects/{p}/resume",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::PathProject,
                    semantic_read: true,
                },
            )],
        ),
        (
            "/v1/projects/{p}/activities",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::PathProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/projects/{p}/up-to-speed",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::MemoryRead,
                    scope: ProjectScope::PathProject,
                    semantic_read: true,
                },
            )],
        ),
        (
            "/v1/watchers/heartbeat",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::ActivityCapture,
                    scope: ProjectScope::BodyProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/watchers/unregister",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::ActivityCapture,
                    scope: ProjectScope::BodyProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/watchers/restart-local",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::ActivityCapture,
                    scope: ProjectScope::BodyProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/archive",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::MemoryCurate,
                    scope: ProjectScope::BodyProject,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/agents",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::SystemAdmin,
                    scope: ProjectScope::Global,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/agents/workspaces",
            &[(
                Method::GET,
                RoutePolicy {
                    permission: Permission::SystemAdmin,
                    scope: ProjectScope::Global,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/agents/workspaces/start",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::ActivityCapture,
                    scope: ProjectScope::WorkspaceResource,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/agents/workspaces/{p}/heartbeat",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::ActivityCapture,
                    scope: ProjectScope::WorkspaceResource,
                    semantic_read: false,
                },
            )],
        ),
        (
            "/v1/agents/workspaces/{p}/finish",
            &[(
                Method::POST,
                RoutePolicy {
                    permission: Permission::ActivityCapture,
                    scope: ProjectScope::WorkspaceResource,
                    semantic_read: false,
                },
            )],
        ),
    ];
    for (path, policies) in entries {
        table
            .insert(path.to_string(), policies.to_vec())
            .expect("route policy table entries are unique");
    }
    table
}

fn policy_table() -> &'static matchit::Router<Vec<(Method, RoutePolicy)>> {
    static TABLE: std::sync::OnceLock<matchit::Router<Vec<(Method, RoutePolicy)>>> =
        std::sync::OnceLock::new();
    TABLE.get_or_init(build_policy_table)
}

/// Policy for a concrete request path and method. `None` means the request
/// has no registered policy and MUST be denied.
pub(crate) fn policy_for(method: &Method, path: &str) -> Option<RoutePolicy> {
    let matched = policy_table().at(path).ok()?;
    matched
        .value
        .iter()
        .find(|(m, _)| m == method)
        .map(|(_, policy)| *policy)
}

#[axum::debug_middleware]
pub(crate) async fn authorization_guard(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    if !state.is_primary() {
        if request.uri().path() == "/ws" {
            return next.run(request).await;
        }
        if request.uri().path().starts_with("/v1/") {
            return match proxy_relay_request(&state, request).await {
                Ok(response) => response,
                Err(error) => error.into_response(),
            };
        }
    }

    let Some(policy) = policy_for(request.method(), request.uri().path()) else {
        // Fail closed: a route without an explicit policy entry is denied.
        return ApiError::forbidden("no authorization policy registered for this route")
            .into_response();
    };
    if policy.scope == ProjectScope::Public {
        return next.run(request).await;
    }

    let principal =
        match resolve_request_principal(&state, request.headers(), CookiePolicy::Allow).await {
            Ok(Some(principal)) => principal,
            Ok(None) => {
                return ApiError::unauthorized("authentication required").into_response();
            }
            Err(error) => return error.into_response(),
        };

    if principal.credential_source == CredentialSource::BrowserSession
        && is_mutating(request.method())
        && let Err(error) = validate_browser_mutation(&state, request.headers(), &principal)
    {
        return error.into_response();
    }

    let project = match resolve_request_project(&state, &mut request, policy.scope).await {
        Ok(project) => project,
        Err(error) => return error.into_response(),
    };
    let authorized = is_authorized(&principal, policy, project.as_deref());
    if !authorized {
        return ApiError::forbidden("principal does not have the required role for this resource")
            .into_response();
    }

    request.extensions_mut().insert(principal);
    next.run(request).await
}

/// Pure authorization decision for a resolved (policy, project) pair.
/// Project-scoped policies whose project could not be resolved require a
/// GLOBAL grant — never "any grant anywhere", which would let a principal
/// scoped to one project act on resources it cannot even name.
fn is_authorized(
    principal: &AuthenticatedPrincipal,
    policy: RoutePolicy,
    project: Option<&str>,
) -> bool {
    match policy.scope {
        ProjectScope::Public => true,
        ProjectScope::Global => principal.has_global(policy.permission),
        ProjectScope::Unscoped => principal.has_anywhere(policy.permission),
        _ => match project {
            Some(project) => principal.has_for_project(project, policy.permission),
            None => principal.has_global(policy.permission),
        },
    }
}

pub(crate) async fn proxy_relay_request(
    state: &AppState,
    request: Request,
) -> Result<Response, ApiError> {
    let peer = crate::selected_primary_peer(state).ok_or_else(|| {
        ApiError::service_unavailable("no primary memory service available on the local network")
    })?;
    let (parts, body) = request.into_parts();
    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or_else(|| parts.uri.path());
    let body = to_bytes(body, MAX_AUTH_INSPECTION_BODY)
        .await
        .map_err(|error| ApiError::io(error.into()))?;
    let mut headers = parts.headers;
    for name in [
        header::HOST,
        header::CONTENT_LENGTH,
        header::CONNECTION,
        header::TRANSFER_ENCODING,
        header::ACCEPT_ENCODING,
    ] {
        headers.remove(name);
    }
    let upstream = state
        .http_client
        .request(
            parts.method,
            format!("http://{}{}", peer.advertise_addr, path_and_query),
        )
        .headers(headers)
        .body(body)
        .send()
        .await
        .map_err(|error| ApiError::io(error.into()))?;
    let status = upstream.status();
    let upstream_headers = upstream.headers().clone();
    let mut response = Response::new(Body::from_stream(upstream.bytes_stream()));
    *response.status_mut() = status;
    for (name, value) in &upstream_headers {
        if !matches!(
            name.as_str(),
            "content-length" | "transfer-encoding" | "content-encoding" | "connection"
        ) {
            response.headers_mut().append(name, value.clone());
        }
    }
    Ok(response)
}

fn is_mutating(method: &Method) -> bool {
    !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

fn validate_browser_mutation(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    principal: &AuthenticatedPrincipal,
) -> Result<(), ApiError> {
    let public_base_url = state
        .config
        .auth
        .public_base_url
        .as_deref()
        .ok_or_else(|| {
            ApiError::forbidden("auth.public_base_url is required for browser writes")
        })?;
    let expected_origin = reqwest::Url::parse(public_base_url)
        .map_err(|_| ApiError::internal("auth.public_base_url is invalid"))?
        .origin()
        .ascii_serialization();
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::forbidden("browser write is missing Origin"))?;
    if origin != expected_origin {
        return Err(ApiError::forbidden("browser write Origin is not allowed"));
    }
    let csrf_header = headers
        .get("x-csrf-token")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::forbidden("browser write is missing x-csrf-token"))?;
    let csrf_cookie = cookie_value(headers, CSRF_COOKIE_NAME)
        .ok_or_else(|| ApiError::forbidden("browser write is missing CSRF cookie"))?;
    if csrf_header != csrf_cookie
        || principal.session_csrf_hash.as_deref() != Some(hash_secret(csrf_header).as_slice())
    {
        return Err(ApiError::forbidden("invalid browser CSRF token"));
    }
    Ok(())
}

async fn resolve_request_project(
    state: &AppState,
    request: &mut Request,
    scope: ProjectScope,
) -> Result<Option<String>, ApiError> {
    match scope {
        ProjectScope::Public | ProjectScope::Unscoped | ProjectScope::Global => Ok(None),
        ProjectScope::PathProject => Ok(request
            .uri()
            .path()
            .split('/')
            .nth(3)
            .and_then(|value| urlencoding::decode(value).ok())
            .map(|value| value.into_owned())),
        ProjectScope::BodyProject => project_from_body(request).await,
        ProjectScope::BodyMemoryResource => {
            let memory_id = memory_id_from_body(request).await?;
            resource_project(state, memory_id, ResourceKind::Memory).await
        }
        ProjectScope::QueryProjectOrGlobal => Ok(query_value(request, "project")),
        ProjectScope::MemoryResource => {
            resource_project(state, resource_id(request), ResourceKind::Memory).await
        }
        ProjectScope::ValidationResource => {
            resource_project(state, resource_id(request), ResourceKind::Validation).await
        }
        ProjectScope::LoopRunResource => {
            resource_project(state, resource_id(request), ResourceKind::LoopRun).await
        }
        ProjectScope::LoopApprovalResource => {
            resource_project(state, resource_id(request), ResourceKind::LoopApproval).await
        }
        ProjectScope::LoopProposalResource => {
            resource_project(state, resource_id(request), ResourceKind::LoopProposal).await
        }
        ProjectScope::WorkspaceResource => {
            resource_project(state, resource_id(request), ResourceKind::Workspace).await
        }
    }
}

async fn project_from_body(request: &mut Request) -> Result<Option<String>, ApiError> {
    body_string_field(request, "project").await
}

async fn memory_id_from_body(request: &mut Request) -> Result<Option<Uuid>, ApiError> {
    Ok(body_string_field(request, "memory_id")
        .await?
        .and_then(|value| Uuid::parse_str(&value).ok()))
}

async fn body_string_field(request: &mut Request, field: &str) -> Result<Option<String>, ApiError> {
    let body = std::mem::replace(request.body_mut(), Body::empty());
    let bytes = to_bytes(body, MAX_AUTH_INSPECTION_BODY)
        .await
        .map_err(|_| {
            ApiError::status_message(StatusCode::PAYLOAD_TOO_LARGE, "request body is too large")
        })?;
    let value = if bytes.is_empty() {
        None
    } else {
        serde_json::from_slice::<Value>(&bytes)
            .ok()
            .and_then(|value| value.get(field).and_then(Value::as_str).map(str::to_owned))
    };
    *request.body_mut() = Body::from(bytes);
    Ok(value)
}

fn query_value(request: &Request, name: &str) -> Option<String> {
    request.uri().query().and_then(|query| {
        query.split('&').find_map(|part| {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            (key == name)
                .then(|| {
                    urlencoding::decode(value)
                        .ok()
                        .map(|value| value.into_owned())
                })
                .flatten()
        })
    })
}

#[derive(Debug, Clone, Copy)]
enum ResourceKind {
    Memory,
    Validation,
    LoopRun,
    LoopApproval,
    LoopProposal,
    Workspace,
}

async fn resource_project(
    state: &AppState,
    id: Option<Uuid>,
    kind: ResourceKind,
) -> Result<Option<String>, ApiError> {
    let Some(id) = id else {
        return Ok(None);
    };
    let pool = state.pool()?;
    let sql = match kind {
        ResourceKind::Memory => {
            "SELECT p.slug FROM memory_entries r JOIN projects p ON p.id = r.project_id WHERE r.id = $1 OR r.canonical_id = $1 LIMIT 1"
        }
        ResourceKind::Validation => {
            "SELECT p.slug FROM memory_validation_runs r JOIN projects p ON p.id = r.project_id WHERE r.id = $1"
        }
        ResourceKind::LoopRun => {
            "SELECT p.slug FROM loop_runs r JOIN projects p ON p.id = r.project_id WHERE r.id = $1"
        }
        ResourceKind::LoopApproval => {
            "SELECT p.slug FROM approval_requests r JOIN projects p ON p.id = r.project_id WHERE r.id = $1"
        }
        ResourceKind::LoopProposal => {
            "SELECT p.slug FROM memory_proposals r JOIN projects p ON p.id = r.project_id WHERE r.id = $1"
        }
        ResourceKind::Workspace => {
            "SELECT p.slug FROM agent_workspaces r JOIN projects p ON p.id = r.project_id WHERE r.id = $1"
        }
    };
    let row = sqlx::query(sql)
        .bind(id)
        .fetch_optional(&pool)
        .await
        .map_err(ApiError::sql)?;
    row.map(|row| row.try_get("slug").map_err(ApiError::sql))
        .transpose()
}

fn resource_id(request: &Request) -> Option<Uuid> {
    request
        .uri()
        .path()
        .split('/')
        .find_map(|part| Uuid::parse_str(part).ok())
}

/// Whether a request is allowed while the service runs in read-only mode:
/// plain reads always, mutating methods only when their policy entry marks
/// them as semantic reads.
pub(crate) fn read_only_request_allowed(method: &Method, path: &str) -> bool {
    if matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return true;
    }
    policy_for(method, path).is_some_and(|policy| policy.semantic_read)
}

pub(crate) async fn read_only_guard(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if state.config.service.read_only
        && !read_only_request_allowed(request.method(), request.uri().path())
    {
        return ApiError::status_message(
            axum::http::StatusCode::FORBIDDEN,
            "this Memory Layer runs in read-only (student) mode; writes are disabled",
        )
        .into_response();
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use axum::http::Method;
    use mem_record::{AuthPrincipalKind, AuthRole, Permission, PermissionSet};
    use uuid::Uuid;

    use super::{
        AuthenticatedPrincipal, CredentialSource, ProjectScope, RoutePolicy, is_authorized,
        policy_for, read_only_request_allowed,
    };
    use crate::auth::ProjectRoleGrant;

    fn project_admin(project: &str) -> AuthenticatedPrincipal {
        let mut project_roles = BTreeMap::new();
        project_roles.insert(
            project.to_string(),
            ProjectRoleGrant {
                role: AuthRole::Admin,
                permissions: AuthRole::Admin.permissions(),
                source: "test".to_string(),
            },
        );
        AuthenticatedPrincipal {
            id: Uuid::new_v4(),
            kind: AuthPrincipalKind::HumanOidc,
            display_name: "Project Admin".to_string(),
            email: None,
            issuer: None,
            subject: None,
            groups: Vec::new(),
            global_role: None,
            global: PermissionSet::EMPTY,
            project_roles,
            credential_source: CredentialSource::BrowserSession,
            token_id: None,
            session_id: None,
            session_csrf_hash: None,
        }
    }

    #[test]
    fn delete_memory_resolves_the_owning_project() {
        assert_eq!(
            policy_for(&Method::DELETE, "/v1/memory")
                .expect("policy")
                .scope,
            ProjectScope::BodyMemoryResource
        );
    }

    #[test]
    fn project_scoped_admin_cannot_act_across_projects() {
        let principal = project_admin("project-a");
        let delete_policy = RoutePolicy {
            permission: Permission::MemoryDelete,
            scope: ProjectScope::BodyMemoryResource,
            semantic_read: false,
        };
        // Memory resolved to another project: denied.
        assert!(!is_authorized(&principal, delete_policy, Some("project-b")));
        // Memory in the granted project: allowed.
        assert!(is_authorized(&principal, delete_policy, Some("project-a")));
        // Unresolvable project (unknown memory id, or a body without a
        // project on a BodyProject route) requires a GLOBAL grant.
        assert!(!is_authorized(&principal, delete_policy, None));
        let prune_policy = RoutePolicy {
            permission: Permission::MemoryCurate,
            scope: ProjectScope::BodyProject,
            semantic_read: false,
        };
        assert!(!is_authorized(&principal, prune_policy, None));
        assert!(is_authorized(&principal, prune_policy, Some("project-a")));
    }

    #[test]
    fn global_admin_still_authorized_without_project() {
        let mut principal = project_admin("project-a");
        principal.global_role = Some(AuthRole::Admin);
        principal.global = AuthRole::Admin.permissions();
        let delete_policy = RoutePolicy {
            permission: Permission::MemoryDelete,
            scope: ProjectScope::BodyMemoryResource,
            semantic_read: false,
        };
        assert!(is_authorized(&principal, delete_policy, None));
        assert!(is_authorized(&principal, delete_policy, Some("project-b")));
    }

    #[test]
    fn route_roles_cover_role_boundaries() {
        assert_eq!(
            policy_for(&Method::POST, "/v1/capture/task")
                .expect("policy")
                .permission,
            Permission::ActivityCapture
        );
        assert_eq!(
            policy_for(&Method::POST, "/v1/curate")
                .expect("policy")
                .permission,
            Permission::MemoryCurate
        );
        assert_eq!(
            policy_for(&Method::DELETE, "/v1/memory")
                .expect("policy")
                .permission,
            Permission::MemoryDelete
        );
        assert_eq!(
            policy_for(&Method::GET, "/v1/projects/demo/overview")
                .expect("policy")
                .scope,
            ProjectScope::PathProject
        );
        assert_eq!(
            policy_for(&Method::GET, "/healthz").expect("policy").scope,
            ProjectScope::Public
        );
        assert_eq!(
            policy_for(&Method::GET, "/v1/loops/context_pack_refresh/context-pack")
                .expect("policy")
                .scope,
            ProjectScope::QueryProjectOrGlobal
        );
        assert_eq!(
            policy_for(&Method::POST, "/v1/auth/tokens")
                .expect("policy")
                .permission,
            Permission::AuthManage
        );
    }

    #[test]
    fn reads_and_query_paths_are_allowed() {
        assert!(read_only_request_allowed(&Method::GET, "/v1/loops"));
        assert!(read_only_request_allowed(
            &Method::GET,
            "/v1/projects/demo/structure"
        ));
        assert!(read_only_request_allowed(&Method::POST, "/v1/query"));
        assert!(read_only_request_allowed(&Method::POST, "/v1/query/global"));
        assert!(read_only_request_allowed(
            &Method::POST,
            "/v1/projects/classroom/resume"
        ));
        assert!(read_only_request_allowed(
            &Method::POST,
            "/v1/projects/classroom/up-to-speed"
        ));
        assert!(read_only_request_allowed(
            &Method::POST,
            "/v1/projects/classroom/bundle/export"
        ));
        assert!(read_only_request_allowed(
            &Method::POST,
            "/v1/projects/classroom/bundle/export/preview"
        ));
    }

    #[test]
    fn mutating_endpoints_are_blocked() {
        assert!(!read_only_request_allowed(&Method::POST, "/v1/curate"));
        assert!(!read_only_request_allowed(
            &Method::POST,
            "/v1/capture/task"
        ));
        assert!(!read_only_request_allowed(
            &Method::POST,
            "/v1/projects/classroom/bundle/import"
        ));
        assert!(!read_only_request_allowed(
            &Method::POST,
            "/v1/projects/classroom/bundle/import/preview"
        ));
        assert!(!read_only_request_allowed(&Method::POST, "/v1/archive"));
        assert!(!read_only_request_allowed(
            &Method::POST,
            "/v1/loops/memory_consolidation/run"
        ));
        assert!(!read_only_request_allowed(&Method::DELETE, "/v1/memory"));
        assert!(!read_only_request_allowed(
            &Method::PUT,
            "/v1/projects/classroom/replacement-policy"
        ));
        assert!(!read_only_request_allowed(
            &Method::POST,
            "/v1/admin/shutdown"
        ));
    }

    #[test]
    fn unregistered_routes_are_denied() {
        assert!(policy_for(&Method::GET, "/v1/not-a-route").is_none());
        assert!(policy_for(&Method::DELETE, "/v1/query").is_none());
        assert!(!read_only_request_allowed(&Method::POST, "/v1/not-a-route"));
    }

    /// Every route registered in routes.rs must have a policy entry for each of
    /// its methods. A missing entry is fail-closed at runtime, but this test
    /// makes the drift loud at build time instead of at first request.
    #[test]
    fn every_registered_route_has_a_policy() {
        let source = include_str!("../routes.rs");
        let mut missing = Vec::new();
        let mut index = 0;
        while let Some(offset) = source[index..].find(".route(") {
            let start = index + offset + ".route(".len();
            let rest = &source[start..];
            let Some(open_quote) = rest.find('"') else {
                break;
            };
            let after = &rest[open_quote + 1..];
            let Some(close_quote) = after.find('"') else {
                break;
            };
            let path = &after[..close_quote];
            // capture handler expression up to the matching close paren
            let mut depth = 1;
            let mut end = start;
            let bytes = source.as_bytes();
            while depth > 0 && end < source.len() {
                match bytes[end] {
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    _ => {}
                }
                end += 1;
            }
            let handlers = &source[start..end - 1];
            for (needle, method) in [
                ("get(", Method::GET),
                ("post(", Method::POST),
                ("put(", Method::PUT),
                ("delete(", Method::DELETE),
                ("patch(", Method::PATCH),
            ] {
                if handlers.contains(needle) && policy_for(&method, &sample_path(path)).is_none() {
                    missing.push(format!("{method} {path}"));
                }
            }
            index = end;
        }
        assert!(
            missing.is_empty(),
            "routes without a policy entry (add them to build_policy_table): {missing:?}"
        );
    }

    fn sample_path(template: &str) -> String {
        template
            .split('/')
            .map(|segment| {
                if segment.starts_with('{') {
                    "0be04b5e-0000-4000-8000-000000000000"
                } else {
                    segment
                }
            })
            .collect::<Vec<_>>()
            .join("/")
    }
}
