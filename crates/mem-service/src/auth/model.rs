// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::BTreeMap;

use mem_record::{
    AuthPrincipalKind, AuthPrincipalResponse, AuthProjectAccess, AuthRole, Permission,
    PermissionSet,
};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CredentialSource {
    AnonymousSingleUser,
    /// The machine-local installation token in single-user mode.
    LocalApiToken,
    LegacyServiceToken,
    ServiceToken,
    BrowserSession,
}

#[derive(Debug, Clone)]
pub(crate) struct AuthenticatedPrincipal {
    pub(crate) id: Uuid,
    pub(crate) kind: AuthPrincipalKind,
    pub(crate) display_name: String,
    pub(crate) email: Option<String>,
    /// OIDC issuer/subject pair when the principal came from an identity
    /// provider - the natural slot for a future decentralized identifier.
    pub(crate) issuer: Option<String>,
    pub(crate) subject: Option<String>,
    pub(crate) groups: Vec<String>,
    /// Display-level preset name of the global grant, when it came from one.
    pub(crate) global_role: Option<AuthRole>,
    /// Effective global permissions (preset expansion or custom set).
    pub(crate) global: PermissionSet,
    pub(crate) project_roles: BTreeMap<String, ProjectRoleGrant>,
    pub(crate) credential_source: CredentialSource,
    pub(crate) token_id: Option<Uuid>,
    pub(crate) session_id: Option<Uuid>,
    pub(crate) session_csrf_hash: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub(crate) struct ProjectRoleGrant {
    /// Display-level preset name, when the grant came from one.
    pub(crate) role: AuthRole,
    /// Effective permissions of this grant.
    pub(crate) permissions: PermissionSet,
    pub(crate) source: String,
}

impl AuthenticatedPrincipal {
    pub(crate) fn actor_label(&self) -> String {
        format!("{} ({})", self.display_name, self.id)
    }

    /// Effective permissions within one project: the global set plus any
    /// project grant.
    pub(crate) fn permissions_for_project(&self, project: &str) -> PermissionSet {
        self.project_roles
            .get(project)
            .map(|grant| grant.permissions)
            .unwrap_or(PermissionSet::EMPTY)
            .union(self.global)
    }

    pub(crate) fn has_for_project(&self, project: &str, permission: Permission) -> bool {
        self.permissions_for_project(project).contains(permission)
    }

    /// Whether ANY grant (global or any project) carries the permission.
    /// Only valid for unscoped resources; project resources must use
    /// `has_for_project`.
    pub(crate) fn has_anywhere(&self, permission: Permission) -> bool {
        self.global.contains(permission)
            || self
                .project_roles
                .values()
                .any(|grant| grant.permissions.contains(permission))
    }

    pub(crate) fn has_global(&self, permission: Permission) -> bool {
        self.global.contains(permission)
    }

    /// Projects explicitly granted the permission. Empty means "no project
    /// filter": the caller holds it globally.
    pub(crate) fn projects_with(&self, permission: Permission) -> Vec<String> {
        if self.global.contains(permission) {
            return Vec::new();
        }
        self.project_roles
            .iter()
            .filter(|(_, grant)| grant.permissions.contains(permission))
            .map(|(project, _)| project.clone())
            .collect()
    }

    pub(crate) fn to_response(&self) -> AuthPrincipalResponse {
        AuthPrincipalResponse {
            id: self.id,
            kind: self.kind,
            display_name: self.display_name.clone(),
            email: self.email.clone(),
            issuer: self.issuer.clone(),
            subject: self.subject.clone(),
            groups: self.groups.clone(),
            global_role: self.global_role,
            global_permissions: self.global.names(),
            projects: self
                .project_roles
                .iter()
                .map(|(project, grant)| AuthProjectAccess {
                    project: project.clone(),
                    role: grant.role,
                    permissions: grant.permissions.names(),
                    source: grant.source.clone(),
                })
                .collect(),
        }
    }
}

pub(crate) fn parse_role(value: &str) -> Option<AuthRole> {
    match value {
        "reader" => Some(AuthRole::Reader),
        "writer" => Some(AuthRole::Writer),
        "operator" => Some(AuthRole::Operator),
        "admin" => Some(AuthRole::Admin),
        _ => None,
    }
}

pub(crate) fn parse_principal_kind(value: &str) -> Option<AuthPrincipalKind> {
    match value {
        "human_oidc" => Some(AuthPrincipalKind::HumanOidc),
        "service_token" => Some(AuthPrincipalKind::ServiceToken),
        "legacy_service_token" => Some(AuthPrincipalKind::LegacyServiceToken),
        "internal" => Some(AuthPrincipalKind::Internal),
        _ => None,
    }
}

pub(crate) fn max_role(left: Option<AuthRole>, right: Option<AuthRole>) -> Option<AuthRole> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(role), None) | (None, Some(role)) => Some(role),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_label_contains_human_name_and_stable_id() {
        let id = Uuid::new_v4();
        let principal = AuthenticatedPrincipal {
            id,
            kind: AuthPrincipalKind::HumanOidc,
            display_name: "Memory Admin".to_string(),
            email: None,
            issuer: None,
            subject: None,
            groups: Vec::new(),
            global_role: Some(AuthRole::Admin),
            global: AuthRole::Admin.permissions(),
            project_roles: BTreeMap::new(),
            credential_source: CredentialSource::BrowserSession,
            token_id: None,
            session_id: None,
            session_csrf_hash: None,
        };

        assert_eq!(principal.actor_label(), format!("Memory Admin ({id})"));
    }
}
