//! Strictly opt-in usage telemetry. `record_event` is a no-op unless the
//! global config sets BOTH `[telemetry].enabled = true` AND an endpoint —
//! there is no default collector. Payloads are counts only: event name,
//! version, OS, and an anonymous random instance id generated locally. Never
//! project names, queries, file paths, or memory content. Sending is
//! fire-and-forget with a short timeout; failures are silent by design.

use std::fs;
use std::time::Duration;

use mem_api::AppConfig;
use mem_platform::preferred_user_state_dir;

const SEND_TIMEOUT: Duration = Duration::from_secs(3);

/// Anonymous, locally generated instance id (a random UUID persisted in the
/// user state directory). Carries no machine or user information.
fn instance_id() -> Option<String> {
    let path = preferred_user_state_dir()?.join("telemetry-instance-id");
    if let Ok(existing) = fs::read_to_string(&path) {
        let trimmed = existing.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    let fresh = uuid::Uuid::new_v4().to_string();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok()?;
    }
    fs::write(&path, &fresh).ok()?;
    Some(fresh)
}

/// Record a usage event. No-op unless telemetry is fully opted in.
pub(crate) async fn record_event(config: &AppConfig, event: &str) {
    if !config.telemetry.enabled {
        return;
    }
    let Some(endpoint) = config.telemetry.endpoint.clone() else {
        return;
    };
    let Some(instance) = instance_id() else {
        return;
    };
    let payload = serde_json::json!({
        "event": event,
        "version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "instance_id": instance,
    });
    let Ok(client) = reqwest::Client::builder().timeout(SEND_TIMEOUT).build() else {
        return;
    };
    // Fire-and-forget: telemetry must never slow down or fail a command.
    let _ = client.post(endpoint).json(&payload).send().await;
}
