use crate::prelude::*;

#[derive(Debug)]
pub(crate) struct ApiError {
    pub(crate) status: StatusCode,
    pub(crate) message: String,
    pub(crate) diagnostic: Option<Box<DiagnosticInfo>>,
}

impl ApiError {
    pub(crate) fn validation(error: ValidationError) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: error.to_string(),
            diagnostic: None,
        }
    }

    pub(crate) fn unauthorized(message: &str) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.to_string(),
            diagnostic: None,
        }
    }

    pub(crate) fn not_found(message: &str) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.to_string(),
            diagnostic: None,
        }
    }

    pub(crate) fn service_unavailable(message: &str) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: message.to_string(),
            diagnostic: None,
        }
    }

    pub(crate) fn status_message(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            diagnostic: None,
        }
    }

    pub(crate) fn sql(error: sqlx::Error) -> Self {
        let message = error.to_string();
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            diagnostic: Some(Box::new(classify_diagnostic(
                &message,
                "database",
                "sql_request",
                DiagnosticSeverity::Error,
            ))),
            message,
        }
    }

    pub(crate) fn io(error: anyhow::Error) -> Self {
        let message = anyhow_error_message(&error);
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            diagnostic: Some(Box::new(classify_diagnostic(
                &message,
                "service",
                "request",
                DiagnosticSeverity::Error,
            ))),
            message,
        }
    }

    pub(crate) fn diagnostic(status: StatusCode, diagnostic: DiagnosticInfo) -> Self {
        Self {
            status,
            message: diagnostic.message.clone(),
            diagnostic: Some(Box::new(diagnostic)),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = if let Some(diagnostic) = self.diagnostic {
            serde_json::json!({
                "error": self.message,
                "code": diagnostic.code,
                "source": diagnostic.source,
                "component": diagnostic.component,
                "operation": diagnostic.operation,
                "severity": diagnostic.severity,
                "explanation": diagnostic.explanation,
                "fix_hint": diagnostic.fix_hint,
                "doctor_hint": diagnostic.doctor_hint,
                "command_hint": diagnostic.command_hint,
                "diagnostic": diagnostic
            })
        } else {
            serde_json::json!({
                "error": self.message
            })
        };
        (self.status, Json(body)).into_response()
    }
}

pub(crate) fn anyhow_error_message(error: &anyhow::Error) -> String {
    let mut message = error.to_string();
    for cause in error.chain().skip(1) {
        message.push_str(": ");
        message.push_str(&cause.to_string());
    }
    message
}

pub(crate) fn classify_anyhow_diagnostic(
    error: &anyhow::Error,
    component: &str,
    operation: &str,
    severity: DiagnosticSeverity,
) -> DiagnosticInfo {
    classify_diagnostic(&anyhow_error_message(error), component, operation, severity)
}

pub(crate) fn classify_diagnostic(
    raw_error: &str,
    component: &str,
    operation: &str,
    severity: DiagnosticSeverity,
) -> DiagnosticInfo {
    let lower = raw_error.to_lowercase();
    let mut diagnostic = DiagnosticInfo {
        code: "internal_error".to_string(),
        source: "service".to_string(),
        component: component.to_string(),
        operation: operation.to_string(),
        severity,
        message: raw_error.to_string(),
        raw_error: Some(raw_error.to_string()),
        explanation: Some("Memory Layer hit an internal operation failure.".to_string()),
        fix_hint: Some(
            "Run `memory doctor` and inspect the service log for the recorded diagnostic."
                .to_string(),
        ),
        doctor_hint: Some("memory doctor".to_string()),
        command_hint: None,
    };

    if lower.contains("insufficient_quota") || (lower.contains("429") && lower.contains("quota")) {
        diagnostic.code = if component == "llm" {
            "llm_quota_exceeded".to_string()
        } else {
            "embedding_quota_exceeded".to_string()
        };
        diagnostic.source = "provider".to_string();
        diagnostic.message = if component == "llm" {
            "The configured LLM provider rejected the request because quota or billing is exhausted."
        } else {
            "The configured embedding provider rejected the request because quota or billing is exhausted."
        }
        .to_string();
        diagnostic.explanation = Some(
            "The memory write can succeed while follow-up provider work, such as answer generation or embedding creation, fails at the provider boundary."
                .to_string(),
        );
        diagnostic.fix_hint = Some(
            "Restore provider quota/billing or disable automatic creation for the failing backend until quota is available."
                .to_string(),
        );
        diagnostic.command_hint = Some("memory embeddings list".to_string());
        return diagnostic;
    }

    if lower.contains("401")
        || lower.contains("unauthorized")
        || lower.contains("invalid api token")
    {
        diagnostic.code = "auth_invalid_token".to_string();
        diagnostic.source = "configuration".to_string();
        diagnostic.message =
            "Authentication failed because the configured API token was rejected.".to_string();
        diagnostic.explanation = Some(
            "A client, watcher, manager, or provider used a token that the receiver did not accept."
                .to_string(),
        );
        diagnostic.fix_hint = Some(
            "Refresh the Memory Layer token/configuration and restart the affected component."
                .to_string(),
        );
        diagnostic.command_hint = Some("memory doctor".to_string());
        return diagnostic;
    }

    if lower.contains("pgvector")
        || lower.contains("extension 'vector'")
        || lower.contains("type \"vector\"")
    {
        diagnostic.code = "database_pgvector_missing".to_string();
        diagnostic.source = "database".to_string();
        diagnostic.component = "database".to_string();
        diagnostic.message =
            "PostgreSQL is missing the pgvector extension required for embeddings.".to_string();
        diagnostic.explanation = Some(
            "Semantic search stores vectors in PostgreSQL using pgvector; migrations cannot complete without it."
                .to_string(),
        );
        diagnostic.fix_hint = Some(
            "Install pgvector for PostgreSQL and run `CREATE EXTENSION IF NOT EXISTS vector;` in the memory database."
                .to_string(),
        );
        diagnostic.command_hint = Some("memory doctor".to_string());
        return diagnostic;
    }

    if lower.contains("migration") || lower.contains("database") || lower.contains("sql") {
        diagnostic.code = "database_operation_failed".to_string();
        diagnostic.source = "database".to_string();
        diagnostic.component = "database".to_string();
        diagnostic.message = "A database operation failed.".to_string();
        diagnostic.explanation = Some(
            "The request reached PostgreSQL but failed during a query or migration step."
                .to_string(),
        );
        diagnostic.fix_hint = Some(
            "Run `memory doctor`, verify the configured database URL, and inspect migrations."
                .to_string(),
        );
    }

    diagnostic
}
