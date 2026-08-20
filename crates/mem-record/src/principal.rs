// SPDX-License-Identifier: AGPL-3.0-or-later

#[allow(unused_imports)]
use std::{
    collections::HashMap,
    fmt,
    path::{Path, PathBuf},
    time::Duration,
};

#[allow(unused_imports)]
use chrono::{DateTime, Utc};
#[allow(unused_imports)]
use serde::{Deserialize, Serialize};
#[allow(unused_imports)]
use thiserror::Error;
#[allow(unused_imports)]
use uuid::Uuid;

#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    #[default]
    SingleUser,
    MultiUser,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthRole {
    Reader,
    Writer,
    Operator,
    Admin,
}

impl AuthRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reader => "reader",
            Self::Writer => "writer",
            Self::Operator => "operator",
            Self::Admin => "admin",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthPrincipalKind {
    HumanOidc,
    ServiceToken,
    LegacyServiceToken,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthProjectAccess {
    pub project: String,
    pub role: AuthRole,
    #[serde(default)]
    pub permissions: Vec<String>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthPrincipalResponse {
    pub id: Uuid,
    pub kind: AuthPrincipalKind,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global_role: Option<AuthRole>,
    #[serde(default)]
    pub global_permissions: Vec<String>,
    #[serde(default)]
    pub projects: Vec<AuthProjectAccess>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthMeResponse {
    pub authenticated: bool,
    pub mode: AuthMode,
    pub read_only: bool,
    pub principal: AuthPrincipalResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthServiceTokenCreateRequest {
    pub name: String,
    pub project: String,
    pub role: AuthRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_in_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthServiceTokenResponse {
    pub id: Uuid,
    pub principal_id: Uuid,
    pub name: String,
    pub token_prefix: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(default)]
    pub projects: Vec<AuthProjectAccess>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthMembershipGrantRequest {
    pub principal_id: Uuid,
    pub project: String,
    pub role: AuthRole,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthMembershipResponse {
    pub id: Uuid,
    pub principal_id: Uuid,
    pub project: String,
    pub role: AuthRole,
    pub source: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// One grantable capability. Routes require exactly one permission;
/// role names survive as named permission-set presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    MemoryRead,
    ActivityCapture,
    MemoryCurate,
    MemoryDelete,
    LoopsRun,
    LoopsConfigure,
    BundleExport,
    BundleImport,
    EmbeddingsManage,
    AuthManage,
    SystemAdmin,
}

impl Permission {
    pub const ALL: [Permission; 11] = [
        Permission::MemoryRead,
        Permission::ActivityCapture,
        Permission::MemoryCurate,
        Permission::MemoryDelete,
        Permission::LoopsRun,
        Permission::LoopsConfigure,
        Permission::BundleExport,
        Permission::BundleImport,
        Permission::EmbeddingsManage,
        Permission::AuthManage,
        Permission::SystemAdmin,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Permission::MemoryRead => "memory_read",
            Permission::ActivityCapture => "activity_capture",
            Permission::MemoryCurate => "memory_curate",
            Permission::MemoryDelete => "memory_delete",
            Permission::LoopsRun => "loops_run",
            Permission::LoopsConfigure => "loops_configure",
            Permission::BundleExport => "bundle_export",
            Permission::BundleImport => "bundle_import",
            Permission::EmbeddingsManage => "embeddings_manage",
            Permission::AuthManage => "auth_manage",
            Permission::SystemAdmin => "system_admin",
        }
    }

    pub fn parse(value: &str) -> Option<Permission> {
        Permission::ALL
            .into_iter()
            .find(|permission| permission.as_str() == value)
    }

    const fn bit(self) -> u16 {
        1 << (self as u16)
    }
}

/// A set of permissions. Grants are unions of sets - there is no ordering
/// between permissions, unlike the old Reader<Writer<Operator<Admin ladder.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PermissionSet(u16);

impl PermissionSet {
    pub const EMPTY: PermissionSet = PermissionSet(0);

    pub const fn contains(self, permission: Permission) -> bool {
        self.0 & permission.bit() != 0
    }

    pub const fn with(self, permission: Permission) -> PermissionSet {
        PermissionSet(self.0 | permission.bit())
    }

    pub const fn union(self, other: PermissionSet) -> PermissionSet {
        PermissionSet(self.0 | other.0)
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn from_permissions(permissions: &[Permission]) -> PermissionSet {
        permissions
            .iter()
            .fold(PermissionSet::EMPTY, |set, permission| {
                set.with(*permission)
            })
    }

    pub fn names(self) -> Vec<String> {
        Permission::ALL
            .into_iter()
            .filter(|permission| self.contains(*permission))
            .map(|permission| permission.as_str().to_string())
            .collect()
    }
}

impl AuthRole {
    /// The permission-set preset this role name expands to.
    pub fn permissions(self) -> PermissionSet {
        let reader = PermissionSet::EMPTY
            .with(Permission::MemoryRead)
            .with(Permission::BundleExport);
        let writer = reader.with(Permission::ActivityCapture);
        let operator = writer
            .with(Permission::MemoryCurate)
            .with(Permission::LoopsRun)
            .with(Permission::BundleImport);
        match self {
            AuthRole::Reader => reader,
            AuthRole::Writer => writer,
            AuthRole::Operator => operator,
            AuthRole::Admin => PermissionSet::from_permissions(&Permission::ALL),
        }
    }
}

#[cfg(test)]
mod permission_tests {
    use super::*;

    #[test]
    fn presets_expand_exactly() {
        let reader = AuthRole::Reader.permissions();
        assert!(reader.contains(Permission::MemoryRead));
        assert!(reader.contains(Permission::BundleExport));
        assert!(!reader.contains(Permission::ActivityCapture));

        let writer = AuthRole::Writer.permissions();
        assert!(writer.contains(Permission::ActivityCapture));
        assert!(!writer.contains(Permission::MemoryCurate));

        let operator = AuthRole::Operator.permissions();
        assert!(operator.contains(Permission::MemoryCurate));
        assert!(operator.contains(Permission::LoopsRun));
        assert!(operator.contains(Permission::BundleImport));
        assert!(!operator.contains(Permission::MemoryDelete));
        assert!(!operator.contains(Permission::LoopsConfigure));
        assert!(!operator.contains(Permission::AuthManage));

        let admin = AuthRole::Admin.permissions();
        for permission in Permission::ALL {
            assert!(admin.contains(permission));
        }
    }

    #[test]
    fn permission_names_round_trip() {
        for permission in Permission::ALL {
            assert_eq!(Permission::parse(permission.as_str()), Some(permission));
        }
    }
}
