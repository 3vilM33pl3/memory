use crate::prelude::*;
use crate::*;

#[derive(Debug, Deserialize)]
pub(crate) struct RuntimeStatusQuery {
    pub(crate) project: Option<String>,
    pub(crate) repo_root: Option<String>,
    pub(crate) skill_filter: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct RuntimeStatusResponse {
    pub(crate) generated_at: chrono::DateTime<chrono::Utc>,
    pub(crate) project: String,
    pub(crate) profile: String,
    pub(crate) web: RuntimeComponentStatus,
    pub(crate) service: RuntimeComponentStatus,
    pub(crate) manager: RuntimeManagerStatus,
    pub(crate) watchers: RuntimeWatcherStatus,
    pub(crate) provenance: RuntimeProvenanceStatus,
    pub(crate) skills: RuntimeSkillStatus,
    pub(crate) restart_notice: Option<RuntimeRestartNotice>,
}

#[derive(Debug, Serialize)]
pub(crate) struct RuntimeComponentStatus {
    pub(crate) version: String,
    pub(crate) status: String,
    pub(crate) detail: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct RuntimeManagerStatus {
    pub(crate) version: String,
    pub(crate) state: String,
    pub(crate) mode: Option<String>,
    pub(crate) detail: Option<String>,
    pub(crate) tracked_sessions: usize,
    pub(crate) warning_count: usize,
    pub(crate) runtime_mode: Option<String>,
    pub(crate) last_reconcile_reason: Option<String>,
    pub(crate) event_count: u64,
    pub(crate) fallback_scan_count: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct RuntimeWatcherStatus {
    pub(crate) version: String,
    pub(crate) status: String,
    pub(crate) detail: Option<String>,
    pub(crate) active_count: usize,
    pub(crate) unhealthy_count: usize,
    pub(crate) stale_after_seconds: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct RuntimeProvenanceStatus {
    pub(crate) status: String,
    pub(crate) enabled: bool,
    pub(crate) interval_seconds: u64,
    pub(crate) last_started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub(crate) last_finished_at: Option<chrono::DateTime<chrono::Utc>>,
    pub(crate) last_project: Option<String>,
    pub(crate) checked_count: usize,
    pub(crate) stale_count: usize,
    pub(crate) error: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct RuntimeSkillStatus {
    pub(crate) bundle_version: String,
    pub(crate) status: String,
    pub(crate) summary: String,
    pub(crate) filter: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct RuntimeRestartNotice {
    pub(crate) version: String,
    pub(crate) reason: String,
    pub(crate) marker_path: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ManagerRuntimeStateFile {
    #[serde(default)]
    pub(crate) mode: String,
    #[serde(default)]
    pub(crate) last_reconcile_reason: String,
    #[serde(default)]
    pub(crate) event_count: u64,
    #[serde(default)]
    pub(crate) fallback_scan_count: u64,
    #[serde(default)]
    pub(crate) sessions: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RestartMarker {
    pub(crate) version: String,
    pub(crate) marked_at: chrono::DateTime<chrono::Utc>,
    pub(crate) reason: String,
}

pub(crate) const TUI_RESTART_MARKER_FILE: &str = "tui-restart-required.json";
#[cfg(not(target_os = "macos"))]
pub(crate) const GLOBAL_TUI_RESTART_MARKER: &str =
    "/var/lib/memory-layer/tui-restart-required.json";
#[cfg(target_os = "macos")]
pub(crate) const GLOBAL_TUI_RESTART_MARKER: &str =
    "/usr/local/var/memory-layer/tui-restart-required.json";

pub(crate) const MEMORY_SKILL_NAMES: &[&str] = &[
    "memory-direct-task-start",
    "memory-github-init",
    "memory-layer",
    "memory-plan-execution",
    "memory-project-init",
    "memory-review-proposals",
    "memory-query-resume",
    "memory-remember",
];
const DEFAULT_RUNTIME_SKILL_FILTER: &[&str] = &["memory-layer"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeSkillFilter {
    MemoryLayer,
    All,
}

impl RuntimeSkillFilter {
    fn from_query(value: Option<&str>) -> Self {
        match value.map(str::trim).filter(|value| !value.is_empty()) {
            Some("all") => Self::All,
            _ => Self::MemoryLayer,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::MemoryLayer => "memory-layer",
            Self::All => "all",
        }
    }

    fn skills(self) -> &'static [&'static str] {
        match self {
            Self::MemoryLayer => DEFAULT_RUNTIME_SKILL_FILTER,
            Self::All => MEMORY_SKILL_NAMES,
        }
    }
}

pub(crate) async fn runtime_status(
    State(state): State<AppState>,
    Query(query): Query<RuntimeStatusQuery>,
) -> Result<Json<RuntimeStatusResponse>, ApiError> {
    let project = query
        .project
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("memory")
        .to_string();
    let repo_root = query.repo_root;
    let version = state
        .config
        .profile
        .display_version(env!("CARGO_PKG_VERSION"));
    let profile = state.config.profile;
    let profile_label = profile.to_string();
    let startup_at = state.startup_at;
    let watchers = Arc::clone(&state.watchers);
    let provenance_runtime = Arc::clone(&state.provenance);
    let provenance_enabled = state.config.provenance.reverify_enabled;
    let provenance_interval_seconds = state.config.provenance.reverify_interval.as_secs();
    let service_id = state.config.cluster.service_id.clone();
    let is_primary = state.is_primary();
    let skill_filter = RuntimeSkillFilter::from_query(query.skill_filter.as_deref());

    let response = tokio::task::spawn_blocking(move || {
        let watcher_summary = watcher_summary_for_project(&watchers, &project);
        let provenance = provenance_runtime
            .lock()
            .expect("provenance runtime mutex poisoned")
            .clone();
        let manager = runtime_manager_status(&profile, &version);
        let skills = runtime_skill_status(repo_root.as_deref(), &version, skill_filter);
        let restart_notice = runtime_restart_notice(startup_at, &version);

        RuntimeStatusResponse {
            generated_at: chrono::Utc::now(),
            project,
            profile: profile_label,
            web: RuntimeComponentStatus {
                version: version.clone(),
                status: if restart_notice.is_some() {
                    "restart".to_string()
                } else {
                    "ok".to_string()
                },
                detail: restart_notice
                    .as_ref()
                    .map(|notice| format!("restart for {}", notice.reason)),
            },
            service: RuntimeComponentStatus {
                version: version.clone(),
                status: if is_primary { "ok" } else { "relay" }.to_string(),
                detail: Some(format!(
                    "{} {}",
                    service_id,
                    if is_primary { "primary" } else { "relay" }
                )),
            },
            manager,
            watchers: RuntimeWatcherStatus {
                version: version.clone(),
                status: if watcher_summary.unhealthy_count == 0 {
                    "ok".to_string()
                } else {
                    "warn".to_string()
                },
                detail: Some(format!(
                    "{} active, {} unhealthy",
                    watcher_summary.active_count, watcher_summary.unhealthy_count
                )),
                active_count: watcher_summary.active_count,
                unhealthy_count: watcher_summary.unhealthy_count,
                stale_after_seconds: watcher_summary.stale_after_seconds,
            },
            provenance: RuntimeProvenanceStatus {
                status: provenance.status,
                enabled: provenance_enabled,
                interval_seconds: provenance_interval_seconds,
                last_started_at: provenance.last_started_at,
                last_finished_at: provenance.last_finished_at,
                last_project: provenance.last_project,
                checked_count: provenance.checked_count,
                stale_count: provenance.stale_count,
                error: provenance.error,
            },
            skills,
            restart_notice,
        }
    })
    .await
    .map_err(|e| ApiError::io(anyhow::anyhow!("runtime status task failed: {e}")))?;

    Ok(Json(response))
}

pub(crate) async fn agents_snapshot() -> Result<Json<serde_json::Value>, ApiError> {
    let snapshot = tokio::task::spawn_blocking(|| {
        let mut top = mem_agenttop::AgentTop::new();
        top.collect_snapshot()
    })
    .await
    .map_err(|e| ApiError::io(anyhow::anyhow!("agent snapshot task failed: {e}")))?;

    let sessions: Vec<serde_json::Value> = snapshot
        .sessions
        .iter()
        .map(|s| {
            let status = match s.status {
                mem_agenttop::SessionStatus::Working => "working",
                mem_agenttop::SessionStatus::Waiting => "waiting",
                mem_agenttop::SessionStatus::Done => "done",
            };
            let children: Vec<serde_json::Value> = s
                .children
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "pid": c.pid,
                        "command": c.command,
                        "mem_kb": c.mem_kb,
                        "port": c.port,
                    })
                })
                .collect();
            let subagents: Vec<serde_json::Value> = s
                .subagents
                .iter()
                .map(|a| {
                    serde_json::json!({
                        "name": a.name,
                        "status": a.status,
                        "tokens": a.tokens,
                    })
                })
                .collect();
            serde_json::json!({
                "agent_cli": s.agent_cli,
                "pid": s.pid,
                "session_id": s.session_id,
                "cwd": s.cwd,
                "project_name": s.project_name,
                "started_at": s.started_at,
                "status": status,
                "model": s.model,
                "context_percent": s.context_percent,
                "total_input_tokens": s.total_input_tokens,
                "total_output_tokens": s.total_output_tokens,
                "total_cache_read": s.total_cache_read,
                "total_cache_create": s.total_cache_create,
                "turn_count": s.turn_count,
                "current_tasks": s.current_tasks,
                "mem_mb": s.mem_mb,
                "version": s.version,
                "git_branch": s.git_branch,
                "git_added": s.git_added,
                "git_modified": s.git_modified,
                "token_history": s.token_history,
                "subagents": subagents,
                "mem_file_count": s.mem_file_count,
                "mem_line_count": s.mem_line_count,
                "children": children,
                "initial_prompt": s.initial_prompt,
                "first_assistant_text": s.first_assistant_text,
            })
        })
        .collect();

    let orphan_ports: Vec<serde_json::Value> = snapshot
        .orphan_ports
        .iter()
        .map(|o| {
            serde_json::json!({
                "port": o.port,
                "pid": o.pid,
                "command": o.command,
                "project_name": o.project_name,
            })
        })
        .collect();
    let rate_limits: Vec<serde_json::Value> = snapshot
        .rate_limits
        .iter()
        .map(|rate_limit| {
            serde_json::json!({
                "source": rate_limit.source,
                "five_hour_pct": rate_limit.five_hour_pct,
                "five_hour_resets_at": rate_limit.five_hour_resets_at,
                "seven_day_pct": rate_limit.seven_day_pct,
                "seven_day_resets_at": rate_limit.seven_day_resets_at,
                "updated_at": rate_limit.updated_at,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "collected_at": snapshot.collected_at.to_rfc3339(),
        "sessions": sessions,
        "orphan_ports": orphan_ports,
        "rate_limits": rate_limits,
    })))
}

pub(crate) fn runtime_manager_status(
    profile: &mem_api::Profile,
    version: &str,
) -> RuntimeManagerStatus {
    let unit_installed = manager_unit_path(profile).is_some_and(|path| path.exists());
    let unit_enabled = manager_service_enabled(profile);
    let unit_active = manager_service_running(profile);
    let foreground_active = foreground_manager_process_running(profile);
    let state_file = load_manager_state_file(profile);
    let tracked_sessions = state_file
        .as_ref()
        .map(|state| state.sessions.len())
        .unwrap_or(0);
    let warning_count = state_file
        .as_ref()
        .map(|state| state.warnings.len())
        .unwrap_or(0);
    let runtime_mode = state_file
        .as_ref()
        .and_then(|state| (!state.mode.is_empty()).then(|| state.mode.clone()));
    let last_reconcile_reason = state_file.as_ref().and_then(|state| {
        (!state.last_reconcile_reason.is_empty()).then(|| state.last_reconcile_reason.clone())
    });
    let event_count = state_file
        .as_ref()
        .map(|state| state.event_count)
        .unwrap_or(0);
    let fallback_scan_count = state_file
        .as_ref()
        .map(|state| state.fallback_scan_count)
        .unwrap_or(0);
    let state = if unit_active || foreground_active {
        "active"
    } else if unit_installed || unit_enabled {
        "installed"
    } else if state_file.is_some() || manager_unit_path(profile).is_some() {
        "off"
    } else {
        "error"
    };
    let mode = if unit_active {
        Some("service".to_string())
    } else if foreground_active {
        Some("manual".to_string())
    } else {
        None
    };
    let detail = Some(format!(
        "{} session{}, {} warn{}",
        tracked_sessions,
        plural(tracked_sessions),
        warning_count,
        plural(warning_count)
    ));

    RuntimeManagerStatus {
        version: version.to_string(),
        state: state.to_string(),
        mode,
        detail,
        tracked_sessions,
        warning_count,
        runtime_mode,
        last_reconcile_reason,
        event_count,
        fallback_scan_count,
    }
}

pub(crate) fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

pub(crate) fn load_manager_state_file(
    profile: &mem_api::Profile,
) -> Option<ManagerRuntimeStateFile> {
    let filename = match profile {
        mem_api::Profile::Dev => "watcher-manager-state-dev.json",
        mem_api::Profile::Prod => "watcher-manager-state.json",
    };
    let path = preferred_user_state_dir()?.join(filename);
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

pub(crate) fn foreground_manager_process_running(profile: &mem_api::Profile) -> bool {
    #[cfg(target_os = "macos")]
    let output = ProcessCommand::new("ps")
        .args(["-ww", "-axo", "pid=,command="])
        .output();

    #[cfg(not(target_os = "macos"))]
    let output = ProcessCommand::new("ps")
        .args(["-ww", "-eo", "pid=,command="])
        .output();

    let Ok(output) = output else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let current_pid = std::process::id().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.lines().any(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return false;
        }
        let mut parts = trimmed.split_whitespace();
        let Some(pid) = parts.next() else {
            return false;
        };
        if pid == current_pid {
            return false;
        }
        let command = parts.collect::<Vec<_>>().join(" ");
        command_is_manager_for_profile(&command, profile)
    })
}

pub(crate) fn command_is_manager_for_profile(command: &str, profile: &mem_api::Profile) -> bool {
    if !(command.contains(" watcher manager run")
        || command.ends_with("watcher manager run")
        || command.contains("watcher manager run "))
    {
        return false;
    }
    match profile {
        mem_api::Profile::Prod => !command_looks_dev_stack(command),
        mem_api::Profile::Dev => command_looks_dev_stack(command),
    }
}

pub(crate) fn command_looks_dev_stack(command: &str) -> bool {
    command.contains("target/debug/memory")
        || command.contains("target/release/memory")
        || command.contains("MEMORY_LAYER_PROFILE=dev")
        || command.contains("MEMORY_LAYER_PROFILE=\"dev\"")
        || command.contains("MEMORY_LAYER_PROFILE='dev'")
        || command.contains("config.dev.toml")
        || command.contains("/.mem/runtime/dev/")
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn linux_manager_unit_path() -> Option<PathBuf> {
    let config_home = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|home| PathBuf::from(home).join(".config"))
        })?;
    Some(
        config_home
            .join("systemd")
            .join("user")
            .join("memory-watch-manager.service"),
    )
}

pub(crate) fn manager_unit_path(profile: &mem_api::Profile) -> Option<PathBuf> {
    if matches!(profile, mem_api::Profile::Dev) {
        return None;
    }

    #[cfg(target_os = "macos")]
    {
        mem_platform::watch_manager_launch_agent_path()
    }

    #[cfg(not(target_os = "macos"))]
    {
        linux_manager_unit_path()
    }
}

pub(crate) fn manager_service_enabled(profile: &mem_api::Profile) -> bool {
    if matches!(profile, mem_api::Profile::Dev) {
        return false;
    }

    #[cfg(target_os = "macos")]
    {
        mem_platform::watch_manager_launch_agent_path().is_some_and(|path| path.exists())
    }

    #[cfg(not(target_os = "macos"))]
    {
        systemctl_user_check("is-enabled", "memory-watch-manager.service")
    }
}

pub(crate) fn manager_service_running(profile: &mem_api::Profile) -> bool {
    if matches!(profile, mem_api::Profile::Dev) {
        return false;
    }

    #[cfg(target_os = "macos")]
    {
        launchctl_print_succeeds(mem_platform::watch_manager_launch_agent_label())
    }

    #[cfg(not(target_os = "macos"))]
    {
        systemctl_user_check("is-active", "memory-watch-manager.service")
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn systemctl_user_check(action: &str, unit: &str) -> bool {
    ProcessCommand::new("systemctl")
        .args(["--user", action, unit])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
pub(crate) fn launchctl_print_succeeds(label: &str) -> bool {
    let Ok(output) = ProcessCommand::new("id").arg("-u").output() else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let uid = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let target = format!("gui/{uid}/{label}");
    ProcessCommand::new("launchctl")
        .args(["print", &target])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub(crate) fn runtime_skill_status(
    repo_root: Option<&str>,
    expected_version: &str,
    filter: RuntimeSkillFilter,
) -> RuntimeSkillStatus {
    let Some(repo_root) = repo_root.map(str::trim).filter(|value| !value.is_empty()) else {
        return RuntimeSkillStatus {
            bundle_version: expected_version.to_string(),
            status: "unknown".to_string(),
            summary: "repo root not resolved".to_string(),
            filter: filter.label().to_string(),
        };
    };
    let root = FsPath::new(repo_root);
    if !root.exists() {
        return RuntimeSkillStatus {
            bundle_version: expected_version.to_string(),
            status: "error".to_string(),
            summary: format!("repo root does not exist: {repo_root}"),
            filter: filter.label().to_string(),
        };
    }
    let skill_root = root.join(".agents").join("skills");
    let mut missing = 0usize;
    let mut outdated = 0usize;
    let skills = filter.skills();
    for skill in skills {
        let path = skill_root.join(skill).join("SKILL.md");
        let Some(version) = read_skill_version(&path) else {
            missing += 1;
            continue;
        };
        if version.trim() != expected_version.trim() {
            outdated += 1;
        }
    }
    let status = if missing == 0 && outdated == 0 {
        "ok"
    } else {
        "warn"
    };
    let summary = match (filter, status) {
        (RuntimeSkillFilter::MemoryLayer, "ok") => "memory-layer skill current".to_string(),
        (RuntimeSkillFilter::MemoryLayer, _) => {
            format!("memory-layer skill: {missing} missing, {outdated} outdated")
        }
        (RuntimeSkillFilter::All, "ok") => format!("{} skills current", skills.len()),
        (RuntimeSkillFilter::All, _) => format!("{missing} missing, {outdated} outdated"),
    };
    RuntimeSkillStatus {
        bundle_version: expected_version.to_string(),
        status: status.to_string(),
        summary,
        filter: filter.label().to_string(),
    }
}

pub(crate) fn read_skill_version(path: &FsPath) -> Option<String> {
    let contents = fs::read_to_string(path).ok()?;
    contents.lines().find_map(|line| {
        line.trim()
            .strip_prefix("version:")
            .map(|value| {
                value
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string()
            })
            .filter(|value| !value.is_empty())
    })
}

pub(crate) fn runtime_restart_notice(
    startup_at: chrono::DateTime<chrono::Utc>,
    running_version: &str,
) -> Option<RuntimeRestartNotice> {
    tui_restart_marker_paths()
        .into_iter()
        .filter_map(|path| {
            let contents = fs::read_to_string(&path).ok()?;
            let marker: RestartMarker = serde_json::from_str(&contents).ok()?;
            if !restart_marker_requires_restart(
                &marker.version,
                running_version,
                marker.marked_at,
                startup_at,
            ) {
                return None;
            }
            Some(RuntimeRestartNotice {
                version: marker.version,
                reason: marker.reason,
                marker_path: path.display().to_string(),
            })
        })
        .max_by_key(|notice| {
            fs::read_to_string(&notice.marker_path)
                .ok()
                .and_then(|contents| serde_json::from_str::<RestartMarker>(&contents).ok())
                .map(|marker| marker.marked_at)
        })
}

pub(crate) fn restart_marker_requires_restart(
    marker_version: &str,
    running_version: &str,
    marked_at: chrono::DateTime<chrono::Utc>,
    startup_at: chrono::DateTime<chrono::Utc>,
) -> bool {
    if version_profile_suffix(marker_version) != version_profile_suffix(running_version) {
        return false;
    }
    if marked_at > startup_at {
        return true;
    }
    match (
        semver::Version::parse(marker_version.trim()),
        semver::Version::parse(running_version.trim()),
    ) {
        (Ok(marker), Ok(running)) => marker > running,
        _ => marker_version.trim() != running_version.trim(),
    }
}

pub(crate) fn tui_restart_marker_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(dir) = preferred_user_state_dir() {
        paths.push(dir.join(TUI_RESTART_MARKER_FILE));
    }
    paths.push(PathBuf::from(GLOBAL_TUI_RESTART_MARKER));
    paths.sort();
    paths.dedup();
    paths
}

pub(crate) fn version_profile_suffix(version: &str) -> &'static str {
    if version.trim().ends_with("-dev") {
        "dev"
    } else {
        "prod"
    }
}
