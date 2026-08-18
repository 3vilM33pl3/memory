use std::collections::BTreeMap;

use mem_api::{AuthPrincipalKind, AuthPrincipalResponse, AuthProjectAccess, AuthRole};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CredentialSource {
    AnonymousSingleUser,
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
    pub(crate) groups: Vec<String>,
    pub(crate) global_role: Option<AuthRole>,
    pub(crate) project_roles: BTreeMap<String, ProjectRoleGrant>,
    pub(crate) credential_source: CredentialSource,
    pub(crate) token_id: Option<Uuid>,
    pub(crate) session_id: Option<Uuid>,
    pub(crate) session_csrf_hash: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub(crate) struct ProjectRoleGrant {
    pub(crate) role: AuthRole,
    pub(crate) source: String,
}

impl AuthenticatedPrincipal {
    pub(crate) fn actor_label(&self) -> String {
        format!("{} ({})", self.display_name, self.id)
    }

    pub(crate) fn role_for_project(&self, project: &str) -> Option<AuthRole> {
        max_role(
            self.global_role,
            self.project_roles.get(project).map(|grant| grant.role),
        )
    }

    pub(crate) fn has_any_role(&self, required: AuthRole) -> bool {
        self.global_role.is_some_and(|role| role >= required)
            || self
                .project_roles
                .values()
                .any(|grant| grant.role >= required)
    }

    pub(crate) fn has_global_role(&self, required: AuthRole) -> bool {
        self.global_role.is_some_and(|role| role >= required)
    }

    pub(crate) fn authorized_projects(&self, required: AuthRole) -> Vec<String> {
        if self.global_role.is_some_and(|role| role >= required) {
            return Vec::new();
        }
        self.project_roles
            .iter()
            .filter(|(_, grant)| grant.role >= required)
            .map(|(project, _)| project.clone())
            .collect()
    }

    pub(crate) fn to_response(&self) -> AuthPrincipalResponse {
        AuthPrincipalResponse {
            id: self.id,
            kind: self.kind,
            display_name: self.display_name.clone(),
            email: self.email.clone(),
            groups: self.groups.clone(),
            global_role: self.global_role,
            projects: self
                .project_roles
                .iter()
                .map(|(project, grant)| AuthProjectAccess {
                    project: project.clone(),
                    role: grant.role,
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
            groups: Vec::new(),
            global_role: Some(AuthRole::Admin),
            project_roles: BTreeMap::new(),
            credential_source: CredentialSource::BrowserSession,
            token_id: None,
            session_id: None,
            session_csrf_hash: None,
        };

        assert_eq!(principal.actor_label(), format!("Memory Admin ({id})"));
    }
}
