use crate::prelude::*;
use crate::*;

pub(crate) fn source_kind_name(source_kind: &SourceKind) -> &'static str {
    match source_kind {
        SourceKind::TaskPrompt => "task_prompt",
        SourceKind::File => "file",
        SourceKind::GitCommit => "git_commit",
        SourceKind::CommandOutput => "command_output",
        SourceKind::Test => "test",
        SourceKind::Note => "note",
    }
}

pub(crate) fn parse_activity_kind(value: &str) -> ActivityKind {
    match value {
        "checkpoint" => ActivityKind::Checkpoint,
        "scan" => ActivityKind::Scan,
        "plan" => ActivityKind::Plan,
        "commit_sync" => ActivityKind::CommitSync,
        "bundle_export" => ActivityKind::BundleExport,
        "bundle_import" => ActivityKind::BundleImport,
        "graph_extract" => ActivityKind::GraphExtract,
        "query" => ActivityKind::Query,
        "query_error" => ActivityKind::QueryError,
        "watcher_health" => ActivityKind::WatcherHealth,
        "memory_replacement" => ActivityKind::MemoryReplacement,
        "capture_task" => ActivityKind::CaptureTask,
        "curate" => ActivityKind::Curate,
        "reindex" => ActivityKind::Reindex,
        "reembed" => ActivityKind::Reembed,
        "archive" => ActivityKind::Archive,
        "delete_memory" => ActivityKind::DeleteMemory,
        "briefing" => ActivityKind::Briefing,
        "diagnostic" => ActivityKind::Diagnostic,
        "llm_audit" => ActivityKind::LlmAudit,
        _ => ActivityKind::Query,
    }
}

pub(crate) fn watcher_health_label(health: &WatcherHealth) -> &'static str {
    match health {
        WatcherHealth::Healthy => "healthy",
        WatcherHealth::Stale => "stale",
        WatcherHealth::Restarting => "restarting",
        WatcherHealth::Failed => "failed",
    }
}

pub(crate) async fn watcher_heartbeat(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<WatcherHeartbeatRequest>,
) -> Result<Json<WatcherPresenceSummary>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        let project = request.project.clone();
        let (_, changed, transition) = register_watcher_heartbeat(&state.watchers, request.clone());
        if changed {
            notify_project_refreshed(&state, project);
        }
        if let Some((summary, details)) = transition {
            notify_project_changed(
                &state,
                request.project.clone(),
                None,
                ActivityKind::WatcherHealth,
                summary,
                Some(details),
            );
        }
        return Ok(Json(
            proxy_post_json(&state, "/v1/watchers/heartbeat", &request, true).await?,
        ));
    }
    let project = request.project.clone();
    let (summary, changed, transition) = register_watcher_heartbeat(&state.watchers, request);
    if changed {
        notify_project_refreshed(&state, project.clone());
    }
    if let Some((summary, details)) = transition {
        notify_project_changed(
            &state,
            project,
            None,
            ActivityKind::WatcherHealth,
            summary,
            Some(details),
        );
    }
    Ok(Json(summary))
}

pub(crate) async fn watcher_unregister(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<WatcherUnregisterRequest>,
) -> Result<Json<WatcherPresenceSummary>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        let project = request.project.clone();
        let (_, changed) = unregister_watcher(&state.watchers, &request);
        if changed {
            notify_project_refreshed(&state, project);
        }
        return Ok(Json(
            proxy_post_json(&state, "/v1/watchers/unregister", &request, true).await?,
        ));
    }
    let project = request.project.clone();
    let (summary, changed) = unregister_watcher(&state.watchers, &request);
    if changed {
        notify_project_refreshed(&state, project);
    }
    Ok(Json(summary))
}

pub(crate) async fn watcher_restart_local(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<WatcherRestartRequest>,
) -> Result<Json<WatcherRestartResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if request.host_service_id != state.config.cluster.service_id {
        return Err(ApiError::status_message(
            StatusCode::BAD_REQUEST,
            "restart request was sent to the wrong host service",
        ));
    }

    restart_local_watcher_service_name(&local_watcher_restart_service_name(&request))
        .map_err(ApiError::io)?;
    update_local_watcher_restart_state(&state.watchers, &request.watcher_id);
    notify_project_refreshed(&state, request.project.clone());

    Ok(Json(WatcherRestartResponse {
        accepted: true,
        message: format!("requested restart for watcher {}", request.watcher_id),
    }))
}

pub(crate) fn persist_timeline_event(state: &AppState, event: &ServiceEvent) {
    let Ok(pool) = state.pool() else {
        return;
    };
    let project = event.project.clone();
    let kind = activity_kind_label(&event.kind).to_string();
    let id = event.id;
    let summary = event.summary.clone();
    let memory_id = event.memory_id;
    let recorded_at = event.recorded_at;
    let details = event.details.clone().map(sqlx::types::Json);
    let actor_id = event.actor_id.clone();
    let actor_name = event.actor_name.clone();
    let source = event.source.clone();
    let operation_id = event.operation_id.clone();
    let duration_ms = event.duration_ms.map(|value| value as i64);
    let provider = event.provider.clone();
    let model = event.model.clone();
    let input_tokens = event
        .token_usage
        .as_ref()
        .map(|usage| usage.input_tokens as i64);
    let output_tokens = event
        .token_usage
        .as_ref()
        .map(|usage| usage.output_tokens as i64);
    let cache_read_tokens = event
        .token_usage
        .as_ref()
        .map(|usage| usage.cache_read_tokens as i64);
    let cache_write_tokens = event
        .token_usage
        .as_ref()
        .map(|usage| usage.cache_write_tokens as i64);
    let total_tokens = event
        .token_usage
        .as_ref()
        .map(|usage| usage.total_tokens as i64);
    tokio::spawn(async move {
        let project_id = match sqlx::query("SELECT id FROM projects WHERE slug = $1")
            .bind(&project)
            .fetch_optional(&pool)
            .await
        {
            Ok(Some(row)) => match row.try_get::<Uuid, _>("id") {
                Ok(value) => value,
                Err(_) => return,
            },
            _ => return,
        };
        let _ = sqlx::query(
            r#"
            INSERT INTO project_timeline_events (
                id, project_id, recorded_at, kind, memory_id, summary, details_json,
                actor_id, actor_name, source, operation_id, duration_ms, provider, model,
                input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, total_tokens
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19)
            "#,
        )
        .bind(id)
        .bind(project_id)
        .bind(recorded_at)
        .bind(kind)
        .bind(memory_id)
        .bind(summary)
        .bind(details)
        .bind(actor_id)
        .bind(actor_name)
        .bind(source)
        .bind(operation_id)
        .bind(duration_ms)
        .bind(provider)
        .bind(model)
        .bind(input_tokens)
        .bind(output_tokens)
        .bind(cache_read_tokens)
        .bind(cache_write_tokens)
        .bind(total_tokens)
        .execute(&pool)
        .await;
    });
}

pub(crate) fn notify_project_changed(
    state: &AppState,
    project: String,
    memory_id: Option<Uuid>,
    kind: ActivityKind,
    summary: String,
    details: Option<ActivityDetails>,
) {
    notify_project_changed_with_metadata(
        state, project, memory_id, kind, summary, details, None, None, None, None, None, None,
        None, None,
    );
}

pub(crate) fn notify_project_diagnostic(
    state: &AppState,
    project: String,
    diagnostic: DiagnosticInfo,
) {
    notify_project_changed(
        state,
        project,
        None,
        ActivityKind::Diagnostic,
        diagnostic.message.clone(),
        Some(ActivityDetails::Diagnostic { diagnostic }),
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn notify_project_changed_with_metadata(
    state: &AppState,
    project: String,
    memory_id: Option<Uuid>,
    kind: ActivityKind,
    summary: String,
    details: Option<ActivityDetails>,
    actor_id: Option<String>,
    actor_name: Option<String>,
    source: Option<String>,
    operation_id: Option<String>,
    duration_ms: Option<u64>,
    provider: Option<String>,
    model: Option<String>,
    token_usage: Option<TokenUsage>,
) {
    let event = ServiceEvent {
        id: Uuid::new_v4(),
        project,
        memory_id,
        kind,
        summary,
        details,
        recorded_at: chrono::Utc::now(),
        actor_id,
        actor_name,
        source: source.or_else(|| Some("service".to_string())),
        operation_id,
        duration_ms,
        provider,
        model,
        token_usage,
        include_activity: true,
    };
    let _ = state.events.send(event.clone());
    if event.include_activity {
        persist_timeline_event(state, &event);
    }
    let mut history = state
        .recent_activity
        .lock()
        .expect("activity history mutex poisoned");
    history.push_front(event);
    while history.len() > 20 {
        history.pop_back();
    }
}

pub(crate) fn notify_project_refreshed(state: &AppState, project: String) {
    let event = ServiceEvent {
        id: Uuid::new_v4(),
        project,
        memory_id: None,
        kind: ActivityKind::Query,
        summary: String::new(),
        details: None,
        recorded_at: chrono::Utc::now(),
        actor_id: None,
        actor_name: None,
        source: Some("service".to_string()),
        operation_id: None,
        duration_ms: None,
        provider: None,
        model: None,
        token_usage: None,
        include_activity: false,
    };
    let _ = state.events.send(event);
}

pub(crate) fn summarize_query(query: &str) -> String {
    let compact = query.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = compact.chars();
    let summary = chars.by_ref().take(80).collect::<String>();
    if chars.next().is_some() {
        format!("{summary}...")
    } else {
        summary
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_llm_audit_activity(
    state: &AppState,
    project: &str,
    operation: &str,
    request_summary: String,
    request_body: &serde_json::Value,
    status: &str,
    error: Option<&str>,
    duration_ms: Option<u64>,
    token_usage: Option<TokenUsage>,
) {
    let audit = state
        .llm_audit
        .read()
        .expect("llm audit config lock poisoned")
        .clone();
    if !audit.enabled {
        return;
    }
    let (messages, truncated) = llm_audit_messages_from_request(state, &audit, request_body);
    notify_project_changed_with_metadata(
        state,
        project.to_string(),
        None,
        ActivityKind::LlmAudit,
        format!("LLM audit: {operation} {status}"),
        Some(ActivityDetails::LlmAudit {
            operation: operation.to_string(),
            request_summary,
            status: status.to_string(),
            redacted: audit.redact,
            truncated,
            messages,
            error: error.map(ToString::to_string),
        }),
        None,
        None,
        Some("llm_audit".to_string()),
        None,
        duration_ms,
        Some(state.config.llm.provider.clone()),
        Some(state.config.llm.model.clone()),
        token_usage,
    );
}

pub(crate) fn llm_audit_messages_from_request(
    state: &AppState,
    audit: &LlmAuditConfig,
    request_body: &serde_json::Value,
) -> (Vec<LlmAuditMessage>, bool) {
    let max_message_chars = audit.max_message_chars.max(1);
    let max_total_chars = audit.max_total_chars.max(1);
    let api_key = resolve_llm_api_key(&state.config.llm);
    let mut total_chars = 0usize;
    let mut any_truncated = false;
    let mut messages = Vec::new();

    for message in request_body
        .get("messages")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
    {
        let role = message
            .get("role")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let raw_content = message
            .get("content")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let mut content = if audit.redact {
            redact_llm_audit_content(raw_content, api_key.as_deref())
        } else {
            raw_content.to_string()
        };
        let remaining = max_total_chars.saturating_sub(total_chars);
        if remaining == 0 {
            any_truncated = true;
            break;
        }
        let limit = max_message_chars.min(remaining);
        let (limited, truncated) = truncate_chars(&content, limit);
        if truncated {
            any_truncated = true;
        }
        content = limited;
        total_chars = total_chars.saturating_add(content.chars().count());
        messages.push(LlmAuditMessage {
            role,
            content,
            truncated,
        });
        if total_chars >= max_total_chars {
            any_truncated = true;
            break;
        }
    }

    (messages, any_truncated)
}

pub(crate) fn redact_llm_audit_content(content: &str, explicit_secret: Option<&str>) -> String {
    let mut redacted = content.to_string();
    if let Some(secret) = explicit_secret
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        redacted = redacted.replace(secret, "[REDACTED]");
    }
    let patterns = [
        r"(?i)\b(bearer\s+)[A-Za-z0-9._~+/=-]{12,}",
        r#"(?i)\b(api[_-]?key|token|password|secret)\s*[:=]\s*['"]?[^'"\s,;]+"#,
        r"(?i)\b(postgres(?:ql)?|mysql|mongodb|redis)://([^:\s/@]+):([^@\s]+)@",
    ];
    for pattern in patterns {
        if let Ok(regex) = Regex::new(pattern) {
            redacted = match pattern {
                p if p.contains("bearer") => regex.replace_all(&redacted, "$1[REDACTED]").into(),
                p if p.contains("://") => {
                    regex.replace_all(&redacted, "$1://$2:[REDACTED]@").into()
                }
                _ => regex.replace_all(&redacted, "$1=[REDACTED]").into(),
            };
        }
    }
    redacted
}

pub(crate) fn truncate_chars(content: &str, limit: usize) -> (String, bool) {
    if limit == 0 {
        return (String::new(), !content.is_empty());
    }
    let mut chars = content.chars();
    let limited = chars.by_ref().take(limit).collect::<String>();
    if chars.next().is_none() {
        return (limited, false);
    }

    let suffix = "\n[truncated]";
    let suffix_len = suffix.chars().count();
    if limit <= suffix_len {
        return (suffix.chars().take(limit).collect(), true);
    }

    let prefix_limit = limit - suffix_len;
    let prefix = content.chars().take(prefix_limit).collect::<String>();
    (format!("{prefix}{suffix}"), true)
}

pub(crate) fn activity_kind_label(kind: &ActivityKind) -> &'static str {
    match kind {
        ActivityKind::Checkpoint => "checkpoint",
        ActivityKind::Scan => "scan",
        ActivityKind::Plan => "plan",
        ActivityKind::CommitSync => "commit_sync",
        ActivityKind::BundleExport => "bundle_export",
        ActivityKind::BundleImport => "bundle_import",
        ActivityKind::GraphExtract => "graph_extract",
        ActivityKind::Query => "query",
        ActivityKind::QueryError => "query_error",
        ActivityKind::WatcherHealth => "watcher_health",
        ActivityKind::MemoryReplacement => "memory_replacement",
        ActivityKind::CaptureTask => "capture_task",
        ActivityKind::Curate => "curate",
        ActivityKind::Reindex => "reindex",
        ActivityKind::Reembed => "reembed",
        ActivityKind::Archive => "archive",
        ActivityKind::DeleteMemory => "delete_memory",
        ActivityKind::Briefing => "briefing",
        ActivityKind::Diagnostic => "diagnostic",
        ActivityKind::LlmAudit => "llm_audit",
    }
}

pub(crate) async fn fetch_project_overview_with_watchers(
    state: &AppState,
    slug: &str,
) -> Result<ProjectOverviewResponse, sqlx::Error> {
    let pool = state
        .pool()
        .expect("project overview requires a primary database pool");
    let mut overview = fetch_project_overview(
        &pool,
        slug,
        &state.config.automation,
        state.config.embeddings.active_backend(),
    )
    .await?;
    overview.watchers = Some(watcher_summary_for_project(&state.watchers, slug));
    Ok(overview)
}

pub(crate) fn register_watcher_heartbeat(
    watchers: &Mutex<HashMap<String, WatcherPresence>>,
    request: WatcherHeartbeatRequest,
) -> (
    WatcherPresenceSummary,
    bool,
    Option<(String, ActivityDetails)>,
) {
    let mut registry = watchers.lock().expect("watcher registry mutex poisoned");
    let before = watcher_summary_from_registry(&registry, &request.project);
    expire_dead_watchers(&mut registry);
    let now = chrono::Utc::now();
    let mut transition = None;
    registry
        .entry(request.watcher_id.clone())
        .and_modify(|watcher| {
            let previous_health = watcher.health.clone();
            let previous_restart_attempt_count = watcher.restart_attempt_count;
            let recovered = previous_health != WatcherHealth::Healthy;
            watcher.project = request.project.clone();
            watcher.repo_root = request.repo_root.clone();
            watcher.hostname = request.hostname.clone();
            watcher.pid = request.pid;
            watcher.mode = request.mode.clone();
            watcher.started_at = request.started_at;
            watcher.last_heartbeat_at = now;
            watcher.host_service_id = request.host_service_id.clone();
            watcher.managed_by_service = request.managed_by_service;
            watcher.agent_cli = request.agent_cli.clone();
            watcher.agent_session_id = request.agent_session_id.clone();
            watcher.agent_pid = request.agent_pid;
            watcher.agent_started_at = request.agent_started_at;
            watcher.health = WatcherHealth::Healthy;
            watcher.last_restart_attempt_at = None;
            watcher.restart_attempt_count = 0;
            if recovered {
                transition = Some((
                    format!(
                        "Watcher {} recovered from {} after {} restart attempt(s)",
                        request.watcher_id,
                        watcher_health_label(&previous_health),
                        previous_restart_attempt_count
                    ),
                    ActivityDetails::WatcherHealth {
                        watcher_id: request.watcher_id.clone(),
                        hostname: request.hostname.clone(),
                        health: WatcherHealth::Healthy,
                        managed_by_service: request.managed_by_service,
                        restart_attempt_count: 0,
                        agent_cli: request.agent_cli.clone(),
                        agent_session_id: request.agent_session_id.clone(),
                        agent_pid: request.agent_pid,
                        previous_health: Some(previous_health),
                        recovered_after_restart_attempts: Some(previous_restart_attempt_count),
                        message: Some("watcher heartbeat recovered".to_string()),
                    },
                ));
            }
        })
        .or_insert_with(|| WatcherPresence {
            watcher_id: request.watcher_id.clone(),
            project: request.project.clone(),
            repo_root: request.repo_root.clone(),
            hostname: request.hostname.clone(),
            pid: request.pid,
            mode: request.mode.clone(),
            started_at: request.started_at,
            last_heartbeat_at: now,
            host_service_id: request.host_service_id.clone(),
            managed_by_service: request.managed_by_service,
            health: WatcherHealth::Healthy,
            agent_cli: request.agent_cli.clone(),
            agent_session_id: request.agent_session_id.clone(),
            agent_pid: request.agent_pid,
            agent_started_at: request.agent_started_at,
            last_restart_attempt_at: None,
            restart_attempt_count: 0,
        });
    let after = watcher_summary_from_registry(&registry, &request.project);
    let changed = before.active_count != after.active_count
        || before.unhealthy_count != after.unhealthy_count
        || before
            .watchers
            .iter()
            .map(|watcher| watcher.watcher_id.as_str())
            .collect::<Vec<_>>()
            != after
                .watchers
                .iter()
                .map(|watcher| watcher.watcher_id.as_str())
                .collect::<Vec<_>>();
    (after, changed, transition)
}

pub(crate) fn unregister_watcher(
    watchers: &Mutex<HashMap<String, WatcherPresence>>,
    request: &WatcherUnregisterRequest,
) -> (WatcherPresenceSummary, bool) {
    let mut registry = watchers.lock().expect("watcher registry mutex poisoned");
    let before = watcher_summary_from_registry(&registry, &request.project);
    expire_dead_watchers(&mut registry);
    let removed = registry.remove(&request.watcher_id).is_some();
    let after = watcher_summary_from_registry(&registry, &request.project);
    let changed = removed
        || before.active_count != after.active_count
        || before.unhealthy_count != after.unhealthy_count
        || before
            .watchers
            .iter()
            .map(|watcher| watcher.watcher_id.as_str())
            .collect::<Vec<_>>()
            != after
                .watchers
                .iter()
                .map(|watcher| watcher.watcher_id.as_str())
                .collect::<Vec<_>>();
    (after, changed)
}

pub(crate) fn watcher_summary_for_project(
    watchers: &Mutex<HashMap<String, WatcherPresence>>,
    project: &str,
) -> WatcherPresenceSummary {
    let mut registry = watchers.lock().expect("watcher registry mutex poisoned");
    expire_dead_watchers(&mut registry);
    refresh_watcher_health_from_heartbeats(&mut registry);
    watcher_summary_from_registry(&registry, project)
}

pub(crate) fn expire_dead_watchers(registry: &mut HashMap<String, WatcherPresence>) {
    let expiry_after =
        chrono::Duration::from_std(StdDuration::from_secs(WATCHER_EXPIRY_AFTER_SECONDS))
            .expect("valid watcher expiry duration");
    let now = chrono::Utc::now();
    registry.retain(|_, watcher| now - watcher.last_heartbeat_at <= expiry_after);
}

pub(crate) fn refresh_watcher_health_from_heartbeats(
    registry: &mut HashMap<String, WatcherPresence>,
) {
    let stale_after =
        chrono::Duration::from_std(StdDuration::from_secs(WATCHER_STALE_AFTER_SECONDS))
            .expect("valid watcher stale duration");
    let now = chrono::Utc::now();
    for watcher in registry.values_mut() {
        if now - watcher.last_heartbeat_at > stale_after && watcher.health == WatcherHealth::Healthy
        {
            watcher.health = WatcherHealth::Stale;
        }
    }
}

pub(crate) fn watcher_summary_from_registry(
    registry: &HashMap<String, WatcherPresence>,
    project: &str,
) -> WatcherPresenceSummary {
    let mut watchers = registry
        .values()
        .filter(|watcher| watcher.project == project)
        .cloned()
        .collect::<Vec<_>>();
    watchers.sort_by(|left, right| {
        right
            .last_heartbeat_at
            .cmp(&left.last_heartbeat_at)
            .then_with(|| left.watcher_id.cmp(&right.watcher_id))
    });
    let last_heartbeat_at = watchers.first().map(|watcher| watcher.last_heartbeat_at);
    let active_count = watchers
        .iter()
        .filter(|watcher| watcher.health == WatcherHealth::Healthy)
        .count();
    let unhealthy_count = watchers.len().saturating_sub(active_count);
    WatcherPresenceSummary {
        active_count,
        unhealthy_count,
        stale_after_seconds: WATCHER_STALE_AFTER_SECONDS,
        last_heartbeat_at,
        watchers,
    }
}

pub(crate) fn update_local_watcher_restart_state(
    watchers: &Mutex<HashMap<String, WatcherPresence>>,
    watcher_id: &str,
) {
    let mut registry = watchers.lock().expect("watcher registry mutex poisoned");
    if let Some(watcher) = registry.get_mut(watcher_id) {
        watcher.health = WatcherHealth::Restarting;
        watcher.last_restart_attempt_at = Some(chrono::Utc::now());
        watcher.restart_attempt_count = watcher.restart_attempt_count.saturating_add(1);
    }
}

pub(crate) async fn run_watcher_watchdog(state: AppState) -> Result<()> {
    let tick = Duration::from_secs(15);
    let stale_after =
        chrono::Duration::from_std(StdDuration::from_secs(WATCHER_STALE_AFTER_SECONDS))
            .expect("valid watcher stale duration");
    let restart_backoff =
        chrono::Duration::from_std(StdDuration::from_secs(WATCHER_RESTART_BACKOFF_SECONDS))
            .expect("valid watcher restart backoff");
    loop {
        tokio::time::sleep(tick).await;
        if !state.is_primary() {
            continue;
        }

        let mut activity_events = Vec::new();
        let mut restart_requests = Vec::new();
        {
            let mut registry = state
                .watchers
                .lock()
                .expect("watcher registry mutex poisoned");
            expire_dead_watchers(&mut registry);
            let now = chrono::Utc::now();
            for watcher in registry.values_mut() {
                if now - watcher.last_heartbeat_at <= stale_after {
                    continue;
                }

                if !watcher.managed_by_service {
                    if watcher.health != WatcherHealth::Stale {
                        watcher.health = WatcherHealth::Stale;
                        activity_events.push((
                            watcher.project.clone(),
                            format!("Watcher {} went stale", watcher.watcher_id),
                            ActivityDetails::WatcherHealth {
                                watcher_id: watcher.watcher_id.clone(),
                                hostname: watcher.hostname.clone(),
                                health: WatcherHealth::Stale,
                                managed_by_service: false,
                                restart_attempt_count: watcher.restart_attempt_count,
                                agent_cli: watcher.agent_cli.clone(),
                                agent_session_id: watcher.agent_session_id.clone(),
                                agent_pid: watcher.agent_pid,
                                previous_health: Some(WatcherHealth::Healthy),
                                recovered_after_restart_attempts: None,
                                message: Some(
                                    "heartbeat missed; manual watcher will not be restarted"
                                        .to_string(),
                                ),
                            },
                        ));
                    }
                    continue;
                }

                let retry_allowed = watcher
                    .last_restart_attempt_at
                    .map(|last| now - last >= restart_backoff)
                    .unwrap_or(true);
                if watcher.restart_attempt_count >= WATCHER_MAX_RESTART_ATTEMPTS {
                    if watcher.health != WatcherHealth::Failed {
                        watcher.health = WatcherHealth::Failed;
                        activity_events.push((
                            watcher.project.clone(),
                            format!("Watcher {} failed to recover", watcher.watcher_id),
                            ActivityDetails::WatcherHealth {
                                watcher_id: watcher.watcher_id.clone(),
                                hostname: watcher.hostname.clone(),
                                health: WatcherHealth::Failed,
                                managed_by_service: true,
                                restart_attempt_count: watcher.restart_attempt_count,
                                agent_cli: watcher.agent_cli.clone(),
                                agent_session_id: watcher.agent_session_id.clone(),
                                agent_pid: watcher.agent_pid,
                                previous_health: Some(WatcherHealth::Restarting),
                                recovered_after_restart_attempts: None,
                                message: Some("watcher exceeded restart attempt limit".to_string()),
                            },
                        ));
                    }
                    continue;
                }
                if !retry_allowed || watcher.health == WatcherHealth::Restarting {
                    continue;
                }

                watcher.health = WatcherHealth::Restarting;
                watcher.last_restart_attempt_at = Some(now);
                watcher.restart_attempt_count = watcher.restart_attempt_count.saturating_add(1);
                restart_requests.push(WatcherRestartRequest {
                    project: watcher.project.clone(),
                    watcher_id: watcher.watcher_id.clone(),
                    host_service_id: watcher.host_service_id.clone(),
                    agent_session_id: watcher.agent_session_id.clone(),
                });
                activity_events.push((
                    watcher.project.clone(),
                    format!("Restarting watcher {}", watcher.watcher_id),
                    ActivityDetails::WatcherHealth {
                        watcher_id: watcher.watcher_id.clone(),
                        hostname: watcher.hostname.clone(),
                        health: WatcherHealth::Restarting,
                        managed_by_service: true,
                        restart_attempt_count: watcher.restart_attempt_count,
                        agent_cli: watcher.agent_cli.clone(),
                        agent_session_id: watcher.agent_session_id.clone(),
                        agent_pid: watcher.agent_pid,
                        previous_health: Some(WatcherHealth::Stale),
                        recovered_after_restart_attempts: None,
                        message: Some("watcher heartbeat missed; requesting restart".to_string()),
                    },
                ));
            }
        }

        for (project, summary, details) in activity_events {
            notify_project_refreshed(&state, project.clone());
            notify_project_changed(
                &state,
                project,
                None,
                ActivityKind::WatcherHealth,
                summary,
                Some(details),
            );
        }

        for request in restart_requests {
            let dispatch = dispatch_watcher_restart(&state, &request).await;
            if let Err(error) = dispatch {
                let mut registry = state
                    .watchers
                    .lock()
                    .expect("watcher registry mutex poisoned");
                if let Some(watcher) = registry.get_mut(&request.watcher_id) {
                    watcher.health = WatcherHealth::Failed;
                    let details = ActivityDetails::WatcherHealth {
                        watcher_id: watcher.watcher_id.clone(),
                        hostname: watcher.hostname.clone(),
                        health: WatcherHealth::Failed,
                        managed_by_service: watcher.managed_by_service,
                        restart_attempt_count: watcher.restart_attempt_count,
                        agent_cli: watcher.agent_cli.clone(),
                        agent_session_id: watcher.agent_session_id.clone(),
                        agent_pid: watcher.agent_pid,
                        previous_health: Some(WatcherHealth::Restarting),
                        recovered_after_restart_attempts: None,
                        message: Some(format!("restart request failed: {error}")),
                    };
                    let project = watcher.project.clone();
                    drop(registry);
                    notify_project_refreshed(&state, project.clone());
                    notify_project_changed(
                        &state,
                        project,
                        None,
                        ActivityKind::WatcherHealth,
                        format!("Watcher {} restart failed", request.watcher_id),
                        Some(details),
                    );
                }
            }
        }
    }
}

pub(crate) async fn dispatch_watcher_restart(
    state: &AppState,
    request: &WatcherRestartRequest,
) -> Result<()> {
    if request.host_service_id == state.config.cluster.service_id {
        restart_local_watcher_service_name(&local_watcher_restart_service_name(request))?;
        return Ok(());
    }

    let peer = cluster_peer_by_service_id(state, &request.host_service_id)
        .ok_or_else(|| anyhow::anyhow!("host-local memory service is unavailable"))?;
    let response = state
        .http_client
        .post(format!(
            "http://{}/v1/watchers/restart-local",
            peer.advertise_addr
        ))
        .header("x-api-token", &state.api_token)
        .json(request)
        .send()
        .await
        .context("send remote watcher restart request")?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("remote restart failed with {status}: {body}");
    }
    Ok(())
}

pub(crate) fn local_watcher_restart_service_name(request: &WatcherRestartRequest) -> String {
    request
        .agent_session_id
        .as_deref()
        .filter(|session_id| !session_id.trim().is_empty())
        .map(managed_watch_service_name)
        .unwrap_or_else(|| watch_service_unit_name(&request.project))
}

pub(crate) fn stream_activity_response(event: ServiceEvent) -> StreamResponse {
    StreamResponse::Activity {
        event: ActivityEvent {
            id: event.id,
            recorded_at: event.recorded_at,
            project: event.project,
            kind: event.kind,
            memory_id: event.memory_id,
            summary: event.summary,
            details: event.details,
            actor_id: event.actor_id,
            actor_name: event.actor_name,
            source: event.source,
            operation_id: event.operation_id,
            duration_ms: event.duration_ms,
            provider: event.provider,
            model: event.model,
            token_usage: event.token_usage,
        },
    }
}
