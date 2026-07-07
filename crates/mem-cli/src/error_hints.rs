//! Actionable hints appended to top-level CLI errors: every common failure
//! should name its own fix. Server-side API errors already carry structured
//! fix hints (see `format_api_error`); this covers what the server cannot —
//! transport failures, auth mismatches, and unresolved projects.

/// A fix-it hint for the given error chain, when one of the known failure
/// modes matches. Matching is on the error chain (typed where possible,
/// message text otherwise).
pub(crate) fn error_hint(error: &anyhow::Error) -> Option<String> {
    let mut is_connect = false;
    let mut is_timeout = false;
    for cause in error.chain() {
        if let Some(reqwest_error) = cause.downcast_ref::<reqwest::Error>() {
            is_connect |= reqwest_error.is_connect();
            is_timeout |= reqwest_error.is_timeout();
        }
    }
    let message = format!("{error:#}").to_lowercase();

    if is_connect || message.contains("connection refused") {
        return Some(
            "The Memory Layer service is not reachable.\n  \
             Start it:  docker compose up      (bundled stack)\n  \
             or:        memory service run     (native install)\n  \
             Diagnose:  memory doctor"
                .to_string(),
        );
    }
    if is_timeout || message.contains("operation timed out") {
        return Some(
            "The service did not respond within the client timeout. Long-running \
             work (curation, reindexing) may still complete server-side.\n  \
             Check:     memory health\n  \
             Diagnose:  memory doctor"
                .to_string(),
        );
    }
    if message.contains("invalid api token") || message.contains("401") {
        return Some(
            "The client's API token does not match the service. If both a dev and \
             an installed service are running, you may be talking to the wrong one.\n  \
             Check the effective endpoint and token: memory doctor\n  \
             The token lives in the global config under [service].api_token."
                .to_string(),
        );
    }
    if message.contains("extension \"vector\"") || message.contains("pgvector") {
        return Some(
            "PostgreSQL is reachable but the pgvector extension is missing.\n  \
             Fix automatically:  memory doctor --fix\n  \
             or manually:        install the pgvector package for your PostgreSQL \
             version, then run CREATE EXTENSION vector; in the target database."
                .to_string(),
        );
    }
    if message.contains("project not configured")
        || message.contains("could not resolve project")
        || message.contains("no repository index found")
    {
        return Some(
            "No project is configured here.\n  \
             Inside a repository:  memory setup   (one-pass machine + project setup)\n  \
             or pass an explicit:  --project <slug>"
                .to_string(),
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_refused_names_the_fix() {
        let error = anyhow::anyhow!("error sending request: connection refused");
        let hint = error_hint(&error).expect("hint");
        assert!(hint.contains("docker compose up"));
        assert!(hint.contains("memory doctor"));
    }

    #[test]
    fn token_mismatch_names_the_fix() {
        let error = anyhow::anyhow!("401 invalid api token");
        assert!(error_hint(&error).expect("hint").contains("api_token"));
    }

    #[test]
    fn unrelated_errors_get_no_hint() {
        let error = anyhow::anyhow!("some unrelated failure");
        assert!(error_hint(&error).is_none());
    }
}
