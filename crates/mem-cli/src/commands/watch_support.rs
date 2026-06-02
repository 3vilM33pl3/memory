use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    time::Instant,
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use mem_agenttop::LightweightAgentSession;
use mem_api::{AppConfig, Profile, read_repo_project_slug};
use mem_platform as platform;
use serde::{Deserialize, Serialize};

use crate::commands::{
    init_support::initialize_repo,
    runtime::{
        WatcherCommand, WatcherManagerArgs, WatcherManagerCommand, default_global_config_path,
    },
    status_support::repair_repo_bootstrap,
};

pub(crate) fn enable_watch_service(repo_root: &Path, project: &str) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        let plist_path = watch_launch_agent_path(project)?;
        write_launch_agent(
            &plist_path,
            render_watch_launch_agent(repo_root, project)?,
            &watch_launch_agent_label(project),
        )?;
        bootstrap_launch_agent(&plist_path, &watch_launch_agent_label(project))?;
        Ok(format!(
            "Installed and started watcher LaunchAgent {}.\nPlist: {}\nRepo: {}\nProject: {}\n\nManage it with:\n- memory watcher status --project {}\n- memory watcher disable --project {}\n- launchctl kickstart -k {}/{}",
            watch_launch_agent_label(project),
            plist_path.display(),
            repo_root.display(),
            project,
            project,
            project,
            launchctl_domain_target()?,
            watch_launch_agent_label(project),
        ))
    }

    #[cfg(not(target_os = "macos"))]
    {
        let unit_name = watch_unit_name(project);
        let unit_dir = user_systemd_unit_dir()?;
        let unit_path = unit_dir.join(&unit_name);
        fs::create_dir_all(&unit_dir).with_context(|| format!("create {}", unit_dir.display()))?;
        fs::write(&unit_path, render_watch_unit(repo_root, project)?)
            .with_context(|| format!("write {}", unit_path.display()))?;
        run_systemctl_user(["daemon-reload"])?;
        run_systemctl_user(["enable", "--now", &unit_name])?;
        Ok(format!(
            "Installed and started user service {}.\nUnit: {}\nRepo: {}\nProject: {}\n\nManage it with:\n- memory watcher status --project {}\n- memory watcher disable --project {}\n- systemctl --user restart {}",
            unit_name,
            unit_path.display(),
            repo_root.display(),
            project,
            project,
            project,
            unit_name
        ))
    }
}

pub(crate) fn preview_enable_watch_service(repo_root: &Path, project: &str) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        Ok(format!(
            "Dry run: would install and start watcher LaunchAgent {}.\nPlist: {}\nRepo: {}\nProject: {}",
            watch_launch_agent_label(project),
            watch_launch_agent_path(project)?.display(),
            repo_root.display(),
            project,
        ))
    }

    #[cfg(not(target_os = "macos"))]
    {
        let unit_name = watch_unit_name(project);
        let unit_path = user_systemd_unit_dir()?.join(&unit_name);
        Ok(format!(
            "Dry run: would install and start user service {}.\nUnit: {}\nRepo: {}\nProject: {}",
            unit_name,
            unit_path.display(),
            repo_root.display(),
            project,
        ))
    }
}

pub(crate) fn disable_watch_service(project: &str) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        let plist_path = watch_launch_agent_path(project)?;
        let label = watch_launch_agent_label(project);
        let _ = bootout_launch_agent(&plist_path, &label);
        if plist_path.exists() {
            fs::remove_file(&plist_path)
                .with_context(|| format!("remove {}", plist_path.display()))?;
        }
        Ok(format!(
            "Disabled watcher LaunchAgent {}.\nRemoved plist: {}",
            label,
            plist_path.display()
        ))
    }

    #[cfg(not(target_os = "macos"))]
    {
        let unit_name = watch_unit_name(project);
        let unit_path = user_systemd_unit_dir()?.join(&unit_name);
        let _ = run_systemctl_user(["disable", "--now", &unit_name]);
        if unit_path.exists() {
            fs::remove_file(&unit_path)
                .with_context(|| format!("remove {}", unit_path.display()))?;
        }
        run_systemctl_user(["daemon-reload"])?;
        Ok(format!(
            "Disabled user service {}.\nRemoved unit: {}",
            unit_name,
            unit_path.display()
        ))
    }
}

pub(crate) fn preview_disable_watch_service(project: &str) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        Ok(format!(
            "Dry run: would disable watcher LaunchAgent {} and remove {}",
            watch_launch_agent_label(project),
            watch_launch_agent_path(project)?.display(),
        ))
    }

    #[cfg(not(target_os = "macos"))]
    {
        let unit_name = watch_unit_name(project);
        let unit_path = user_systemd_unit_dir()?.join(&unit_name);
        Ok(format!(
            "Dry run: would disable user service {} and remove {}",
            unit_name,
            unit_path.display(),
        ))
    }
}

pub(crate) fn watch_service_status(repo_root: &Path, project: &str) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        let plist_path = watch_launch_agent_path(project)?;
        let label = watch_launch_agent_label(project);
        let status = launch_agent_status(&label)?;
        Ok(format!(
            "Watcher service for project {}:\n- label: {}\n- plist: {}\n- repo: {}\n- installed: {}\n- loaded: {}\n- running: {}\n\nInspect with:\n- launchctl print {}/{}",
            project,
            label,
            plist_path.display(),
            repo_root.display(),
            yes_no(plist_path.exists()),
            yes_no(status.loaded),
            yes_no(status.running),
            launchctl_domain_target()?,
            label
        ))
    }

    #[cfg(not(target_os = "macos"))]
    {
        let unit_name = watch_unit_name(project);
        let unit_path = user_systemd_unit_dir()?.join(&unit_name);
        let is_enabled = run_systemctl_user(["is-enabled", &unit_name]).is_ok();
        let is_active = run_systemctl_user(["is-active", &unit_name]).is_ok();
        Ok(format!(
            "Watcher service for project {}:\n- unit: {}\n- repo: {}\n- installed: {}\n- enabled: {}\n- active: {}\n\nInspect with:\n- systemctl --user status {}",
            project,
            unit_path.display(),
            repo_root.display(),
            yes_no(unit_path.exists()),
            yes_no(is_enabled),
            yes_no(is_active),
            unit_name
        ))
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) const WATCH_MANAGER_UNIT_NAME: &str = "memory-watch-manager.service";
const WATCH_MANAGER_EVENT_DEBOUNCE_MS: u64 = 500;
const WATCH_MANAGER_FALLBACK_SCAN_SECONDS: u64 = 30;
const WATCH_MANAGER_HEALTH_SCAN_SECONDS: u64 = 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub(crate) struct WatcherManagerState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) updated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub(crate) mode: String,
    #[serde(default)]
    pub(crate) last_reconcile_reason: String,
    #[serde(default)]
    pub(crate) last_reconcile_duration_ms: u128,
    #[serde(default)]
    pub(crate) event_count: u64,
    #[serde(default)]
    pub(crate) fallback_scan_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) lock_owner_pid: Option<u32>,
    #[serde(default)]
    pub(crate) sessions: std::collections::BTreeMap<String, ManagedWatcherSession>,
    #[serde(default)]
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ManagedWatcherSession {
    pub(crate) unit_name: String,
    pub(crate) project: String,
    pub(crate) repo_root: String,
    pub(crate) agent_cli: String,
    pub(crate) agent_session_id: String,
    pub(crate) agent_pid: u32,
    pub(crate) agent_started_at: DateTime<Utc>,
}

pub(crate) async fn run_watcher_manager(
    config: AppConfig,
    config_path: Option<PathBuf>,
) -> Result<()> {
    let _lock = WatcherManagerLock::acquire(config.profile)?;
    let version = config.profile.display_version(env!("CARGO_PKG_VERSION"));
    eprintln!(
        "watcher manager v{version} starting (profile={profile}, service={service_addr}, mode=event-driven, fallback={fallback}s)",
        profile = config.profile,
        service_addr = config.service.bind_addr,
        fallback = WATCH_MANAGER_FALLBACK_SCAN_SECONDS,
    );
    if let Some(path) = config.resolved_config_path.as_deref() {
        eprintln!("  config: {}", path.display());
    }
    if let Some(path) = config.resolved_dev_overlay_path.as_deref() {
        eprintln!("  dev overlay: {}", path.display());
    }
    eprintln!(
        "  state: {}",
        watcher_manager_state_path(config.profile)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "unknown".to_string())
    );
    let mut event_rx = match start_watcher_manager_event_source() {
        Ok(rx) => Some(rx),
        Err(error) => {
            eprintln!(
                "watcher manager session file events unavailable; using fallback scans only: {error}"
            );
            None
        }
    };
    reconcile_watcher_manager(&config, config_path.as_deref(), "startup", true, 0, 0).await?;
    let mut debounce: Option<std::pin::Pin<Box<tokio::time::Sleep>>> = None;
    let mut fallback = tokio::time::interval(std::time::Duration::from_secs(
        WATCH_MANAGER_FALLBACK_SCAN_SECONDS,
    ));
    fallback.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    fallback.tick().await;
    let mut health = tokio::time::interval(std::time::Duration::from_secs(
        WATCH_MANAGER_HEALTH_SCAN_SECONDS,
    ));
    health.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    health.tick().await;
    let mut event_count = 0u64;
    let mut fallback_scan_count = 0u64;
    loop {
        tokio::select! {
            Some(_) = async {
                match event_rx.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            }, if event_rx.is_some() => {
                event_count = event_count.saturating_add(1);
                debounce = Some(Box::pin(tokio::time::sleep(std::time::Duration::from_millis(
                    WATCH_MANAGER_EVENT_DEBOUNCE_MS,
                ))));
            }
            _ = async {
                if let Some(delay) = debounce.as_mut() {
                    delay.as_mut().await;
                }
            }, if debounce.is_some() => {
                debounce = None;
                if let Err(error) = reconcile_watcher_manager(
                    &config,
                    config_path.as_deref(),
                    "session-file-event",
                    false,
                    event_count,
                    fallback_scan_count,
                ).await {
                    eprintln!("watcher manager reconcile failed: {error}");
                }
            }
            _ = fallback.tick() => {
                fallback_scan_count = fallback_scan_count.saturating_add(1);
                if let Err(error) = reconcile_watcher_manager(
                    &config,
                    config_path.as_deref(),
                    "fallback-scan",
                    false,
                    event_count,
                    fallback_scan_count,
                ).await {
                    eprintln!("watcher manager reconcile failed: {error}");
                }
            }
            _ = health.tick() => {
                if let Err(error) = reconcile_watcher_manager(
                    &config,
                    config_path.as_deref(),
                    "health-scan",
                    true,
                    event_count,
                    fallback_scan_count,
                ).await {
                    eprintln!("watcher manager reconcile failed: {error}");
                }
            }
        }
    }
}

pub(crate) async fn reconcile_watcher_manager(
    config: &AppConfig,
    config_path: Option<&Path>,
    reason: &str,
    verify_units: bool,
    event_count: u64,
    fallback_scan_count: u64,
) -> Result<()> {
    let started = Instant::now();
    let mut state = load_watcher_manager_state(config.profile)?;
    let previous_state = state.clone();
    state.warnings.clear();

    let sessions = mem_agenttop::collect_lightweight_agent_sessions();
    let mut seen = std::collections::BTreeSet::new();

    for session in sessions {
        let Some(repo_root) = resolve_agent_repo_root(&session.cwd)? else {
            continue;
        };
        if !repo_agent_watch_enabled(&repo_root)? {
            state.warnings.push(format!(
                "Skipped {} session {} in {} because repo opted out of agent-linked watchers.",
                session.agent_cli,
                session.session_id,
                repo_root.display()
            ));
            continue;
        }

        let project = resolve_manager_project_slug(&repo_root);
        ensure_agent_watch_repo_bootstrap(&repo_root, &project)?;

        if legacy_watch_service_is_active(&project) {
            state.warnings.push(format!(
                "Skipped agent-linked watcher for project {} because legacy watcher service {} is active.",
                project,
                legacy_watch_service_name(&project)
            ));
            continue;
        }

        let unit_name = managed_watch_service_name(&session.session_id);
        let tracked = state.sessions.contains_key(&session.session_id);
        let mut unit_loaded = tracked;
        let mut unit_running = tracked;
        if !tracked || verify_units {
            unit_loaded = managed_watch_service_loaded(&session.session_id);
            unit_running = managed_watch_service_running(&session.session_id);
        }
        if should_start_agent_watcher(tracked, unit_loaded, unit_running) {
            if unit_loaded {
                let _ = stop_managed_watch_service(&session.session_id);
            }
            start_managed_agent_watcher(&repo_root, &project, &session, config_path)?;
        }

        let agent_started_at = DateTime::<Utc>::from_timestamp_millis(session.started_at as i64)
            .unwrap_or_else(Utc::now);
        state.sessions.insert(
            session.session_id.clone(),
            ManagedWatcherSession {
                unit_name: unit_name.clone(),
                project,
                repo_root: repo_root.display().to_string(),
                agent_cli: session.agent_cli.to_string(),
                agent_session_id: session.session_id.clone(),
                agent_pid: session.pid,
                agent_started_at,
            },
        );
        seen.insert(session.session_id.clone());
    }

    let stale = state
        .sessions
        .keys()
        .filter(|session_id| !seen.contains(*session_id))
        .cloned()
        .collect::<Vec<_>>();
    for session_id in stale {
        if let Some(entry) = state.sessions.remove(&session_id) {
            let _ = stop_managed_watch_service(&session_id);
            #[cfg(target_os = "macos")]
            if let Ok(path) = managed_watch_launch_agent_path(&session_id) {
                if path.exists() {
                    let _ = fs::remove_file(path);
                }
            }
            let _ = entry;
        }
    }

    state.updated_at = Some(Utc::now());
    state.mode = "event-driven".to_string();
    state.last_reconcile_reason = reason.to_string();
    state.last_reconcile_duration_ms = started.elapsed().as_millis();
    state.event_count = event_count;
    state.fallback_scan_count = fallback_scan_count;
    state.lock_owner_pid = Some(std::process::id());
    save_watcher_manager_state_if_changed(config.profile, &previous_state, &state)?;
    Ok(())
}

pub(crate) fn resolve_agent_repo_root(cwd: &str) -> Result<Option<PathBuf>> {
    // The session cwd may no longer exist (deleted repo, unmounted volume, etc.).
    if !Path::new(cwd).is_dir() {
        return Ok(None);
    }
    let output = ProcessCommand::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output()
        .with_context(|| format!("run git rev-parse in {cwd}"))?;
    if !output.status.success() {
        return Ok(None);
    }
    let repo_root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if repo_root.is_empty() {
        return Ok(None);
    }
    // When the agent runs inside a git worktree (e.g. Claude Code -w), --show-toplevel
    // returns the worktree path, which lacks .mem/project.toml. Resolve through to
    // the main repo root via --git-common-dir so both agents share the same project slug.
    let common_dir_output = ProcessCommand::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(cwd)
        .output();
    if let Ok(common_output) = common_dir_output
        && common_output.status.success()
    {
        let common_dir = String::from_utf8_lossy(&common_output.stdout)
            .trim()
            .to_string();
        if let Some(main_root) = PathBuf::from(&common_dir).parent()
            && main_root.join(".mem").join("project.toml").exists()
        {
            return Ok(Some(main_root.to_path_buf()));
        }
    }
    Ok(Some(PathBuf::from(repo_root)))
}

pub(crate) fn repo_agent_watch_enabled(repo_root: &Path) -> Result<bool> {
    let path = repo_root.join(".agents").join("memory-layer.toml");
    if !path.is_file() {
        return Ok(true);
    }
    let content = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let value: toml::Value = content
        .parse()
        .with_context(|| format!("parse {}", path.display()))?;
    Ok(value
        .get("agent_watch")
        .and_then(|section| section.get("enabled"))
        .and_then(|value| value.as_bool())
        .unwrap_or(true))
}

pub(crate) fn resolve_manager_project_slug(repo_root: &Path) -> String {
    read_repo_project_slug(repo_root)
        .or_else(|| {
            repo_root
                .file_name()
                .and_then(|value| value.to_str())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| "memory".to_string())
}

pub(crate) fn ensure_agent_watch_repo_bootstrap(repo_root: &Path, project: &str) -> Result<()> {
    if !repo_root.join(".mem").is_dir() {
        initialize_repo(repo_root, project, false, false)?;
    } else {
        repair_repo_bootstrap(repo_root, project)?;
    }
    ensure_agent_watch_repo_config(repo_root)
}

pub(crate) fn ensure_agent_watch_repo_config(repo_root: &Path) -> Result<()> {
    let project = resolve_manager_project_slug(repo_root);
    let path = mem_platform::project_paths(repo_root, &project)
        .map(|paths| paths.config_path())
        .filter(|path| path.is_file())
        .unwrap_or_else(|| repo_root.join(".mem").join("config.toml"));
    let content = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let mut value: toml::Value = content
        .parse()
        .with_context(|| format!("parse {}", path.display()))?;
    let root = value
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("{} does not contain a TOML table root", path.display()))?;
    let automation = root
        .entry("automation")
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("{} [automation] is not a table", path.display()))?;
    automation.insert("enabled".to_string(), toml::Value::Boolean(true));
    automation.insert("mode".to_string(), toml::Value::String("auto".to_string()));
    automation.insert(
        "repo_root".to_string(),
        toml::Value::String(repo_root.display().to_string()),
    );
    write_file_if_changed(&path, toml::to_string_pretty(&value)?.as_bytes())?;
    Ok(())
}

pub(crate) fn legacy_watch_service_is_active(project: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        launch_agent_status(&watch_launch_agent_label(project))
            .map(|status| status.running)
            .unwrap_or(false)
    }

    #[cfg(not(target_os = "macos"))]
    {
        unit_is_active(&watch_unit_name(project))
    }
}

pub(crate) fn legacy_watch_service_name(project: &str) -> String {
    #[cfg(target_os = "macos")]
    {
        watch_launch_agent_label(project)
    }

    #[cfg(not(target_os = "macos"))]
    {
        watch_unit_name(project)
    }
}

pub(crate) fn managed_watch_service_name(session_id: &str) -> String {
    #[cfg(target_os = "macos")]
    {
        managed_watch_launch_agent_label(session_id)
    }

    #[cfg(not(target_os = "macos"))]
    {
        format!(
            "memory-watch-codex-{}.service",
            platform::sanitize_service_fragment(session_id)
        )
    }
}

pub(crate) fn managed_watch_service_loaded(session_id: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        launch_agent_status(&managed_watch_launch_agent_label(session_id))
            .map(|status| status.loaded)
            .unwrap_or(false)
    }

    #[cfg(not(target_os = "macos"))]
    {
        unit_is_loaded(&managed_watch_service_name(session_id))
    }
}

pub(crate) fn managed_watch_service_running(session_id: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        launch_agent_status(&managed_watch_launch_agent_label(session_id))
            .map(|status| status.running)
            .unwrap_or(false)
    }

    #[cfg(not(target_os = "macos"))]
    {
        unit_is_active(&managed_watch_service_name(session_id))
    }
}

pub(crate) fn start_managed_agent_watcher(
    repo_root: &Path,
    project: &str,
    session: &LightweightAgentSession,
    config_path: Option<&Path>,
) -> Result<()> {
    let started_at = DateTime::<Utc>::from_timestamp_millis(session.started_at as i64)
        .unwrap_or_else(Utc::now)
        .to_rfc3339();

    #[cfg(target_os = "macos")]
    {
        let plist_path = managed_watch_launch_agent_path(&session.session_id)?;
        let label = managed_watch_launch_agent_label(&session.session_id);
        write_launch_agent(
            &plist_path,
            render_managed_watch_launch_agent(
                repo_root,
                project,
                session,
                &started_at,
                config_path,
            )?,
            &label,
        )?;
        bootstrap_launch_agent(&plist_path, &label)?;
        return Ok(());
    }

    #[cfg(not(target_os = "macos"))]
    let memory_binary = memory_binary_path()?;

    #[cfg(not(target_os = "macos"))]
    let unit_name = managed_watch_service_name(&session.session_id);

    #[cfg(not(target_os = "macos"))]
    let mut cmd = ProcessCommand::new("systemd-run");
    #[cfg(not(target_os = "macos"))]
    cmd.args([
        "--user",
        "--unit",
        &unit_name,
        "--property",
        &format!("WorkingDirectory={}", repo_root.display()),
        "--property",
        "Restart=no",
        "--setenv=MEMORY_LAYER_WATCH_SERVICE_MANAGED=1",
        "--collect",
    ]);
    #[cfg(not(target_os = "macos"))]
    cmd.arg(memory_binary);
    // Prefer the resolved project config so the watcher talks to the same
    // service instance the TUI and CLI use for this project.
    #[cfg(not(target_os = "macos"))]
    {
        if let Some(path) = config_path {
            cmd.arg("--config").arg(path);
        }
    }
    #[cfg(not(target_os = "macos"))]
    let output = cmd
        .arg("watcher")
        .arg("run")
        .arg("--project")
        .arg(project)
        .arg("--repo-root")
        .arg(repo_root)
        .arg("--agent-cli")
        .arg(session.agent_cli)
        .arg("--agent-session-id")
        .arg(&session.session_id)
        .arg("--agent-pid")
        .arg(session.pid.to_string())
        .arg("--agent-started-at")
        .arg(started_at)
        .output()
        .with_context(|| format!("run systemd-run for {}", session.session_id))?;
    #[cfg(not(target_os = "macos"))]
    if output.status.success() {
        return Ok(());
    }
    #[cfg(not(target_os = "macos"))]
    if unit_is_loaded(&unit_name) {
        return Ok(());
    }
    #[cfg(not(target_os = "macos"))]
    anyhow::bail!(
        "systemd-run failed for {}: {}",
        unit_name,
        String::from_utf8_lossy(&output.stderr).trim()
    )
}

pub(crate) fn load_watcher_manager_state(profile: Profile) -> Result<WatcherManagerState> {
    let path = watcher_manager_state_path(profile)?;
    if !path.is_file() {
        return Ok(WatcherManagerState::default());
    }
    let content = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&content).with_context(|| format!("parse {}", path.display()))
}

pub(crate) fn save_watcher_manager_state(
    profile: Profile,
    state: &WatcherManagerState,
) -> Result<()> {
    let path = watcher_manager_state_path(profile)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(&path, serde_json::to_vec_pretty(state)?)
        .with_context(|| format!("write {}", path.display()))
}

pub(crate) fn save_watcher_manager_state_if_changed(
    profile: Profile,
    previous: &WatcherManagerState,
    next: &WatcherManagerState,
) -> Result<()> {
    let mut comparable_previous = previous.clone();
    let mut comparable_next = next.clone();
    comparable_previous.updated_at = None;
    comparable_next.updated_at = None;
    comparable_previous.last_reconcile_duration_ms = 0;
    comparable_next.last_reconcile_duration_ms = 0;
    if comparable_previous == comparable_next {
        return Ok(());
    }
    save_watcher_manager_state(profile, next)
}

pub(crate) fn write_file_if_changed(path: &Path, next: &[u8]) -> Result<()> {
    if let Ok(current) = fs::read(path)
        && current == next
    {
        return Ok(());
    }
    fs::write(path, next).with_context(|| format!("write {}", path.display()))
}

pub(crate) fn clear_watcher_manager_state(profile: Profile) -> Result<()> {
    let path = watcher_manager_state_path(profile)?;
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
    }
    Ok(())
}

pub(crate) fn watcher_manager_state_path(profile: Profile) -> Result<PathBuf> {
    let filename = match profile {
        Profile::Dev => "watcher-manager-state-dev.json",
        Profile::Prod => "watcher-manager-state.json",
    };
    Ok(platform::preferred_user_state_dir()
        .ok_or_else(|| anyhow::anyhow!("HOME is not set"))?
        .join(filename))
}

pub(crate) fn watcher_manager_lock_path(profile: Profile) -> Result<PathBuf> {
    Ok(watcher_manager_state_path(profile)?.with_extension("lock"))
}

pub(crate) struct WatcherManagerLock {
    pub(crate) path: PathBuf,
}

impl WatcherManagerLock {
    fn acquire(profile: Profile) -> Result<Self> {
        let path = watcher_manager_lock_path(profile)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        let pid = std::process::id();
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                writeln!(file, "{pid}")?;
                Ok(Self { path })
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let owner = fs::read_to_string(&path)
                    .ok()
                    .and_then(|value| value.trim().parse::<u32>().ok());
                if let Some(owner) = owner
                    && process_is_alive(owner)
                {
                    anyhow::bail!(
                        "watcher manager is already running with pid {owner}; stop it before starting another manager"
                    );
                }
                let _ = fs::remove_file(&path);
                Self::acquire(profile)
            }
            Err(error) => Err(error).with_context(|| format!("create {}", path.display())),
        }
    }
}

impl Drop for WatcherManagerLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub(crate) fn process_is_alive(pid: u32) -> bool {
    ProcessCommand::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub(crate) fn start_watcher_manager_event_source()
-> Result<tokio::sync::mpsc::UnboundedReceiver<()>> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
        if result.is_ok() {
            let _ = tx.send(());
        }
    })
    .context("create watcher manager filesystem watcher")?;

    for dir in watcher_manager_session_dirs() {
        if dir.is_dir() {
            notify::Watcher::watch(&mut watcher, &dir, notify::RecursiveMode::Recursive)
                .with_context(|| format!("watch {}", dir.display()))?;
        }
    }

    std::mem::forget(watcher);
    Ok(rx)
}

pub(crate) fn watcher_manager_session_dirs() -> Vec<PathBuf> {
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let mut dirs = vec![home.join(".codex").join("sessions")];
    let claude_base = env::var("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home.join(".claude"));
    dirs.push(claude_base.join("sessions"));
    dirs
}

pub(crate) fn enable_watch_manager_service(config_path: &Path) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        let plist_path = watch_manager_launch_agent_path()?;
        let label = watch_manager_launch_agent_label();
        write_launch_agent(
            &plist_path,
            render_watch_manager_launch_agent(config_path)?,
            label,
        )?;
        bootstrap_launch_agent(&plist_path, label)?;
        return Ok(format!(
            "Installed and started watcher manager LaunchAgent {}.\nPlist: {}\nConfig: {}\n\nManage it with:\n- memory watcher manager status\n- memory watcher manager disable\n- launchctl kickstart -k {}/{}",
            label,
            plist_path.display(),
            config_path.display(),
            launchctl_domain_target()?,
            label
        ));
    }

    #[cfg(not(target_os = "macos"))]
    {
        let unit_dir = user_systemd_unit_dir()?;
        let unit_path = unit_dir.join(WATCH_MANAGER_UNIT_NAME);
        fs::create_dir_all(&unit_dir).with_context(|| format!("create {}", unit_dir.display()))?;
        fs::write(&unit_path, render_watch_manager_unit(config_path)?)
            .with_context(|| format!("write {}", unit_path.display()))?;
        run_systemctl_user(["daemon-reload"])?;
        run_systemctl_user(["enable", "--now", WATCH_MANAGER_UNIT_NAME])?;
        Ok(format!(
            "Installed and started user service {}.\nUnit: {}\nConfig: {}\n\nManage it with:\n- memory watcher manager status\n- memory watcher manager disable\n- systemctl --user restart {}",
            WATCH_MANAGER_UNIT_NAME,
            unit_path.display(),
            config_path.display(),
            WATCH_MANAGER_UNIT_NAME
        ))
    }
}

pub(crate) fn preview_enable_watch_manager_service() -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        return Ok(format!(
            "Dry run: would install and start watcher manager LaunchAgent {}.\nPlist: {}",
            watch_manager_launch_agent_label(),
            watch_manager_launch_agent_path()?.display(),
        ));
    }

    #[cfg(not(target_os = "macos"))]
    {
        let unit_path = user_systemd_unit_dir()?.join(WATCH_MANAGER_UNIT_NAME);
        Ok(format!(
            "Dry run: would install and start user service {}.\nUnit: {}",
            WATCH_MANAGER_UNIT_NAME,
            unit_path.display()
        ))
    }
}

pub(crate) fn disable_watch_manager_service(profile: Profile) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        let plist_path = watch_manager_launch_agent_path()?;
        let label = watch_manager_launch_agent_label();
        let _ = bootout_launch_agent(&plist_path, label);
        if plist_path.exists() {
            fs::remove_file(&plist_path)
                .with_context(|| format!("remove {}", plist_path.display()))?;
        }
        if let Ok(state) = load_watcher_manager_state(profile) {
            for session_id in state.sessions.keys() {
                let _ = stop_managed_watch_service(session_id);
                if let Ok(path) = managed_watch_launch_agent_path(session_id) {
                    if path.exists() {
                        let _ = fs::remove_file(path);
                    }
                }
            }
        }
        clear_watcher_manager_state(profile)?;
        return Ok(format!(
            "Disabled watcher manager LaunchAgent {}.\nRemoved plist: {}",
            label,
            plist_path.display()
        ));
    }

    #[cfg(not(target_os = "macos"))]
    {
        let unit_path = user_systemd_unit_dir()?.join(WATCH_MANAGER_UNIT_NAME);
        let _ = run_systemctl_user(["disable", "--now", WATCH_MANAGER_UNIT_NAME]);
        if unit_path.exists() {
            fs::remove_file(&unit_path)
                .with_context(|| format!("remove {}", unit_path.display()))?;
        }
        if let Ok(state) = load_watcher_manager_state(profile) {
            for entry in state.sessions.values() {
                let _ = stop_unit_if_present(&entry.unit_name);
            }
        }
        clear_watcher_manager_state(profile)?;
        run_systemctl_user(["daemon-reload"])?;
        Ok(format!(
            "Disabled user service {}.\nRemoved unit: {}",
            WATCH_MANAGER_UNIT_NAME,
            unit_path.display()
        ))
    }
}

pub(crate) fn preview_disable_watch_manager_service() -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        return Ok(format!(
            "Dry run: would disable watcher manager LaunchAgent {} and remove {}",
            watch_manager_launch_agent_label(),
            watch_manager_launch_agent_path()?.display(),
        ));
    }

    #[cfg(not(target_os = "macos"))]
    {
        let unit_path = user_systemd_unit_dir()?.join(WATCH_MANAGER_UNIT_NAME);
        Ok(format!(
            "Dry run: would disable user service {} and remove {}",
            WATCH_MANAGER_UNIT_NAME,
            unit_path.display()
        ))
    }
}

pub(crate) fn watch_manager_service_status(profile: Profile) -> Result<String> {
    let state = load_watcher_manager_state(profile).unwrap_or_default();
    let warning_lines = if state.warnings.is_empty() {
        "- warnings: none".to_string()
    } else {
        format!("- warnings: {}", state.warnings.join(" | "))
    };
    let runtime_lines = format!(
        "- mode: {}\n- last reconcile reason: {}\n- last reconcile duration: {} ms\n- event count: {}\n- fallback scans: {}\n- lock owner pid: {}",
        if state.mode.is_empty() {
            "unknown"
        } else {
            state.mode.as_str()
        },
        if state.last_reconcile_reason.is_empty() {
            "n/a"
        } else {
            state.last_reconcile_reason.as_str()
        },
        state.last_reconcile_duration_ms,
        state.event_count,
        state.fallback_scan_count,
        state
            .lock_owner_pid
            .map(|pid| pid.to_string())
            .unwrap_or_else(|| "n/a".to_string())
    );

    #[cfg(target_os = "macos")]
    {
        let plist_path = watch_manager_launch_agent_path()?;
        let label = watch_manager_launch_agent_label();
        let status = launch_agent_status(label)?;
        return Ok(format!(
            "Watcher manager service:\n- label: {}\n- plist: {}\n- installed: {}\n- loaded: {}\n- running: {}\n- tracked sessions: {}\n- last reconcile: {}\n{}\n{}\n\nInspect with:\n- launchctl print {}/{}\n- memory watcher manager status",
            label,
            plist_path.display(),
            yes_no(plist_path.exists()),
            yes_no(status.loaded),
            yes_no(status.running),
            state.sessions.len(),
            state
                .updated_at
                .map(|value| value.to_rfc3339())
                .unwrap_or_else(|| "n/a".to_string()),
            runtime_lines,
            warning_lines,
            launchctl_domain_target()?,
            label
        ));
    }

    #[cfg(not(target_os = "macos"))]
    {
        let unit_path = user_systemd_unit_dir()?.join(WATCH_MANAGER_UNIT_NAME);
        let is_enabled = run_systemctl_user(["is-enabled", WATCH_MANAGER_UNIT_NAME]).is_ok();
        let is_active = run_systemctl_user(["is-active", WATCH_MANAGER_UNIT_NAME]).is_ok();
        Ok(format!(
            "Watcher manager service:\n- unit: {}\n- installed: {}\n- enabled: {}\n- active: {}\n- tracked sessions: {}\n- last reconcile: {}\n{}\n{}\n\nInspect with:\n- systemctl --user status {}\n- memory watcher manager status",
            unit_path.display(),
            yes_no(unit_path.exists()),
            yes_no(is_enabled),
            yes_no(is_active),
            state.sessions.len(),
            state
                .updated_at
                .map(|value| value.to_rfc3339())
                .unwrap_or_else(|| "n/a".to_string()),
            runtime_lines,
            warning_lines,
            WATCH_MANAGER_UNIT_NAME
        ))
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn render_watch_manager_unit(config_path: &Path) -> Result<String> {
    let memory_binary = memory_binary_path()?;
    let home = env::var("HOME").unwrap_or_else(|_| "/".to_string());
    Ok(format!(
        "[Unit]\nDescription=Memory Layer Watcher Manager\nAfter=default.target\n\n[Service]\nType=simple\nWorkingDirectory={}\nExecStart={} --config {} watcher manager run\nRestart=always\nRestartSec=2\n\n[Install]\nWantedBy=default.target\n",
        shell_escape_str(&home),
        shell_escape_path(&memory_binary),
        shell_escape_path(config_path),
    ))
}

#[cfg(target_os = "macos")]
pub(crate) fn render_watch_manager_launch_agent(config_path: &Path) -> Result<String> {
    let binary = memory_binary_path()?;
    let working_directory =
        macos_app_support_dir().ok_or_else(|| anyhow::anyhow!("HOME is not set"))?;
    let log_dir = user_memory_layer_log_dir()?;
    let stdout_path = log_dir.join("memory-watch-manager.stdout.log");
    let stderr_path = log_dir.join("memory-watch-manager.stderr.log");
    let command = launch_agent_shell_command(&[
        binary.display().to_string(),
        "--config".to_string(),
        config_path.display().to_string(),
        "watcher".to_string(),
        "manager".to_string(),
        "run".to_string(),
    ])?;
    render_launch_agent_plist(
        watch_manager_launch_agent_label(),
        &working_directory,
        &command,
        &stdout_path,
        &stderr_path,
    )
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn unit_is_active(unit_name: &str) -> bool {
    run_systemctl_user(["is-active", unit_name]).is_ok()
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn unit_is_loaded(unit_name: &str) -> bool {
    let output = ProcessCommand::new("systemctl")
        .args([
            "--user",
            "show",
            unit_name,
            "--property",
            "LoadState",
            "--value",
        ])
        .output();
    let Ok(output) = output else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let load_state = String::from_utf8_lossy(&output.stdout).trim().to_string();
    !load_state.is_empty() && load_state != "not-found"
}

pub(crate) fn should_start_agent_watcher(
    session_tracked: bool,
    unit_loaded: bool,
    unit_active: bool,
) -> bool {
    !session_tracked || !unit_loaded || !unit_active
}

pub(crate) fn stop_managed_watch_service(session_id: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let label = managed_watch_launch_agent_label(session_id);
        let path = managed_watch_launch_agent_path(session_id)?;
        let _ = bootout_launch_agent(&path, &label);
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        let unit_name = managed_watch_service_name(session_id);
        stop_unit_if_present(&unit_name)
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn stop_unit_if_present(unit_name: &str) -> Result<()> {
    if unit_is_loaded(unit_name) {
        let _ = run_systemctl_user(["stop", unit_name]);
        let _ = run_systemctl_user(["reset-failed", unit_name]);
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn render_watch_unit(repo_root: &Path, project: &str) -> Result<String> {
    let memory_binary = memory_binary_path()?;
    let env_file = user_memory_layer_env_file()?;
    let working_directory = repo_root
        .canonicalize()
        .with_context(|| format!("canonicalize {}", repo_root.display()))?;
    Ok(format!(
        "[Unit]\nDescription=Memory Layer Watcher ({project})\nAfter=default.target\n\n[Service]\nType=simple\nEnvironmentFile=-{}\nEnvironment=MEMORY_LAYER_WATCH_SERVICE_MANAGED=1\nWorkingDirectory={}\nExecStart={} --config {} watcher run --project {}\nRestart=on-failure\nRestartSec=2\n\n[Install]\nWantedBy=default.target\n",
        shell_escape_path(&env_file),
        working_directory.display(),
        shell_escape_path(&memory_binary),
        shell_escape_path(&default_global_config_path()),
        shell_escape_str(project),
    ))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn user_systemd_unit_dir() -> Result<PathBuf> {
    if let Ok(config_home) = env::var("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(config_home).join("systemd").join("user"));
    }
    let home = env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join("systemd")
        .join("user"))
}

pub(crate) fn user_memory_layer_env_file() -> Result<PathBuf> {
    platform::preferred_user_env_path().ok_or_else(|| anyhow::anyhow!("HOME is not set"))
}

pub(crate) fn memory_binary_path() -> Result<PathBuf> {
    Ok(platform::current_exe_sibling_binary("memory")
        .or_else(|| std::env::current_exe().ok())
        .unwrap_or_else(|| PathBuf::from("memory")))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn watch_unit_name(project: &str) -> String {
    platform::watch_service_unit_name(project)
}

#[cfg(target_os = "macos")]
pub(crate) fn sanitize_service_fragment(value: &str) -> String {
    platform::sanitize_service_fragment(value)
}

#[cfg(target_os = "macos")]
#[derive(Debug, Default)]
pub(crate) struct LaunchAgentStatus {
    pub(crate) loaded: bool,
    pub(crate) running: bool,
}

#[cfg(target_os = "macos")]
pub(crate) fn backend_launch_agent_label() -> &'static str {
    platform::backend_launch_agent_label()
}

#[cfg(target_os = "macos")]
pub(crate) fn watch_launch_agent_label(project: &str) -> String {
    platform::watch_launch_agent_label(project)
}

#[cfg(target_os = "macos")]
pub(crate) fn backend_launch_agent_path() -> Result<PathBuf> {
    platform::backend_launch_agent_path().ok_or_else(|| anyhow::anyhow!("HOME is not set"))
}

#[cfg(target_os = "macos")]
pub(crate) fn watch_launch_agent_path(project: &str) -> Result<PathBuf> {
    platform::watch_launch_agent_path(project).ok_or_else(|| anyhow::anyhow!("HOME is not set"))
}

#[cfg(target_os = "macos")]
pub(crate) fn watch_manager_launch_agent_label() -> &'static str {
    platform::watch_manager_launch_agent_label()
}

#[cfg(target_os = "macos")]
pub(crate) fn watch_manager_launch_agent_path() -> Result<PathBuf> {
    platform::watch_manager_launch_agent_path().ok_or_else(|| anyhow::anyhow!("HOME is not set"))
}

#[cfg(target_os = "macos")]
pub(crate) fn managed_watch_launch_agent_label(session_id: &str) -> String {
    platform::managed_watch_launch_agent_label(session_id)
}

#[cfg(target_os = "macos")]
pub(crate) fn managed_watch_launch_agent_path(session_id: &str) -> Result<PathBuf> {
    platform::managed_watch_launch_agent_path(session_id)
        .ok_or_else(|| anyhow::anyhow!("HOME is not set"))
}

#[cfg(target_os = "macos")]
pub(crate) fn macos_app_support_dir() -> Option<PathBuf> {
    platform::macos_app_support_dir()
}

#[cfg(target_os = "macos")]
pub(crate) fn user_memory_layer_log_dir() -> Result<PathBuf> {
    platform::user_memory_layer_log_dir().ok_or_else(|| anyhow::anyhow!("HOME is not set"))
}

#[cfg(target_os = "macos")]
pub(crate) fn launchctl_domain_target() -> Result<String> {
    let output = ProcessCommand::new("id")
        .arg("-u")
        .output()
        .context("run id -u")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("id -u failed: {}", stderr.trim());
    }
    let uid = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(format!("gui/{uid}"))
}

#[cfg(target_os = "macos")]
pub(crate) fn write_launch_agent(path: &Path, contents: String, label: &str) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("launch agent path has no parent"))?;
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    fs::write(path, contents).with_context(|| format!("write {}", path.display()))?;
    let _ = bootout_launch_agent(path, label);
    Ok(())
}

#[cfg(target_os = "macos")]
pub(crate) fn bootstrap_launch_agent(path: &Path, label: &str) -> Result<()> {
    run_launchctl([
        "bootstrap",
        &launchctl_domain_target()?,
        &path.display().to_string(),
    ])?;
    run_launchctl([
        "kickstart",
        "-k",
        &format!("{}/{}", launchctl_domain_target()?, label),
    ])?;
    Ok(())
}

#[cfg(target_os = "macos")]
pub(crate) fn bootout_launch_agent(path: &Path, label: &str) -> Result<()> {
    let target = format!("{}/{}", launchctl_domain_target()?, label);
    if run_launchctl(["bootout", &target]).is_ok() {
        return Ok(());
    }
    run_launchctl([
        "bootout",
        &launchctl_domain_target()?,
        &path.display().to_string(),
    ])
}

#[cfg(target_os = "macos")]
pub(crate) fn launch_agent_status(label: &str) -> Result<LaunchAgentStatus> {
    let target = format!("{}/{}", launchctl_domain_target()?, label);
    let output = ProcessCommand::new("launchctl")
        .args(["print", &target])
        .output()
        .with_context(|| format!("run launchctl print {target}"))?;
    if !output.status.success() {
        return Ok(LaunchAgentStatus::default());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(LaunchAgentStatus {
        loaded: true,
        running: stdout.contains("state = running") || stdout.contains("\"PID\" ="),
    })
}

#[cfg(target_os = "macos")]
pub(crate) fn run_launchctl<const N: usize>(args: [&str; N]) -> Result<()> {
    let output = ProcessCommand::new("launchctl")
        .args(args)
        .output()
        .with_context(|| format!("run launchctl {}", args.join(" ")))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    anyhow::bail!(
        "launchctl {} failed: {}{}{}",
        args.join(" "),
        stderr.trim(),
        if stderr.trim().is_empty() || stdout.trim().is_empty() {
            ""
        } else {
            " | "
        },
        stdout.trim()
    )
}

#[cfg(target_os = "macos")]
pub(crate) fn render_backend_launch_agent(config_path: &Path) -> Result<String> {
    let binary = memory_binary_path()?;
    let working_directory =
        macos_app_support_dir().ok_or_else(|| anyhow::anyhow!("HOME is not set"))?;
    let stdout_path = user_memory_layer_log_dir()?.join("mem-service.stdout.log");
    let stderr_path = user_memory_layer_log_dir()?.join("mem-service.stderr.log");
    let command = launch_agent_shell_command(&[
        binary.display().to_string(),
        "--config".to_string(),
        config_path.display().to_string(),
        "service".to_string(),
        "run".to_string(),
    ])?;
    render_launch_agent_plist(
        backend_launch_agent_label(),
        &working_directory,
        &command,
        &stdout_path,
        &stderr_path,
    )
}

#[cfg(target_os = "macos")]
pub(crate) fn render_watch_launch_agent(repo_root: &Path, project: &str) -> Result<String> {
    let binary = memory_binary_path()?;
    let working_directory = repo_root
        .canonicalize()
        .with_context(|| format!("canonicalize {}", repo_root.display()))?;
    let log_dir = user_memory_layer_log_dir()?;
    let sanitized = sanitize_service_fragment(project);
    let stdout_path = log_dir.join(format!("memory-watch-{sanitized}.stdout.log"));
    let stderr_path = log_dir.join(format!("memory-watch-{sanitized}.stderr.log"));
    let command = launch_agent_shell_command(&[
        binary.display().to_string(),
        "--config".to_string(),
        default_global_config_path().display().to_string(),
        "watcher".to_string(),
        "run".to_string(),
        "--project".to_string(),
        project.to_string(),
    ])?;
    let command = format!("export MEMORY_LAYER_WATCH_SERVICE_MANAGED=1; {command}");
    render_launch_agent_plist(
        &watch_launch_agent_label(project),
        &working_directory,
        &command,
        &stdout_path,
        &stderr_path,
    )
}

#[cfg(target_os = "macos")]
pub(crate) fn render_managed_watch_launch_agent(
    repo_root: &Path,
    project: &str,
    session: &LightweightAgentSession,
    started_at: &str,
    config_path: Option<&Path>,
) -> Result<String> {
    let binary = memory_binary_path()?;
    let working_directory = repo_root
        .canonicalize()
        .with_context(|| format!("canonicalize {}", repo_root.display()))?;
    let log_dir = user_memory_layer_log_dir()?;
    let sanitized = sanitize_service_fragment(&session.session_id);
    let stdout_path = log_dir.join(format!("memory-watch-codex-{sanitized}.stdout.log"));
    let stderr_path = log_dir.join(format!("memory-watch-codex-{sanitized}.stderr.log"));
    let mut args = vec![binary.display().to_string()];
    // Prefer the resolved project config so the watcher talks to the same
    // service instance the TUI and CLI use for this project.
    if let Some(path) = config_path {
        args.push("--config".to_string());
        args.push(path.display().to_string());
    }
    args.extend([
        "watcher".to_string(),
        "run".to_string(),
        "--project".to_string(),
        project.to_string(),
        "--repo-root".to_string(),
        repo_root.display().to_string(),
        "--agent-cli".to_string(),
        session.agent_cli.to_string(),
        "--agent-session-id".to_string(),
        session.session_id.clone(),
        "--agent-pid".to_string(),
        session.pid.to_string(),
        "--agent-started-at".to_string(),
        started_at.to_string(),
    ]);
    let command = launch_agent_shell_command(&args)?;
    let command = format!("export MEMORY_LAYER_WATCH_SERVICE_MANAGED=1; {command}");
    render_launch_agent_plist(
        &managed_watch_launch_agent_label(&session.session_id),
        &working_directory,
        &command,
        &stdout_path,
        &stderr_path,
    )
}

#[cfg(target_os = "macos")]
pub(crate) fn shell_export_prefix() -> Result<String> {
    let env_vars = launch_agent_environment_variables()?;
    let mut command = String::new();
    for (key, value) in env_vars {
        command.push_str("export ");
        command.push_str(&key);
        command.push('=');
        command.push_str(&shell_quote_sh(&value));
        command.push_str("; ");
    }
    Ok(command)
}

#[cfg(target_os = "macos")]
pub(crate) fn shell_program_invocation(program_arguments: &[String]) -> String {
    let mut command = String::new();
    let mut first = true;
    for arg in program_arguments {
        if !first {
            command.push(' ');
        }
        first = false;
        command.push_str(&shell_quote_sh(arg));
    }
    command
}

#[cfg(target_os = "macos")]
pub(crate) fn shell_command_for_program(
    program_arguments: &[String],
    exec_program: bool,
) -> Result<String> {
    let mut command = shell_export_prefix()?;
    if exec_program {
        command.push_str("exec");
        command.push(' ');
    }
    command.push_str(&shell_program_invocation(program_arguments));
    Ok(command)
}

#[cfg(target_os = "macos")]
pub(crate) fn launch_agent_shell_command(program_arguments: &[String]) -> Result<String> {
    shell_command_for_program(program_arguments, true)
}

#[cfg(target_os = "macos")]
pub(crate) fn launch_agent_environment_variables() -> Result<BTreeMap<String, String>> {
    let mut values = BTreeMap::new();
    values.insert(
        "HOME".to_string(),
        env::var("HOME").context("HOME is not set")?,
    );
    values.insert(
        "PATH".to_string(),
        env::var("PATH")
            .unwrap_or_else(|_| "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin".to_string()),
    );
    if let Ok(user) = env::var("USER") {
        values.insert("USER".to_string(), user.clone());
        values.insert("LOGNAME".to_string(), user);
    }
    let env_file = user_memory_layer_env_file()?;
    if !env_file.exists() {
        return Ok(values);
    }
    let content =
        fs::read_to_string(&env_file).with_context(|| format!("read {}", env_file.display()))?;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            values.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    Ok(values)
}

#[cfg(target_os = "macos")]
pub(crate) fn render_launch_agent_plist(
    label: &str,
    working_directory: &Path,
    shell_command: &str,
    stdout_path: &Path,
    stderr_path: &Path,
) -> Result<String> {
    let log_dir = stdout_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("stdout log path has no parent"))?;
    fs::create_dir_all(log_dir).with_context(|| format!("create {}", log_dir.display()))?;
    let env_vars = launch_agent_environment_variables()?;
    let program_arguments = [
        "/bin/zsh".to_string(),
        "-lc".to_string(),
        shell_command.to_string(),
    ];
    let args_xml = program_arguments
        .iter()
        .map(|arg| format!("    <string>{}</string>", xml_escape(arg)))
        .collect::<Vec<_>>()
        .join("\n");
    let env_xml = env_vars
        .iter()
        .map(|(key, value)| {
            format!(
                "    <key>{}</key>\n    <string>{}</string>",
                xml_escape(key),
                xml_escape(value)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
{args_xml}
  </array>
  <key>WorkingDirectory</key>
  <string>{working_directory}</string>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>{stdout_path}</string>
  <key>StandardErrorPath</key>
  <string>{stderr_path}</string>
  <key>EnvironmentVariables</key>
  <dict>
{env_xml}
  </dict>
</dict>
</plist>
"#,
        label = xml_escape(label),
        args_xml = args_xml,
        working_directory = xml_escape(&working_directory.display().to_string()),
        stdout_path = xml_escape(&stdout_path.display().to_string()),
        stderr_path = xml_escape(&stderr_path.display().to_string()),
        env_xml = env_xml,
    ))
}

#[cfg(target_os = "macos")]
pub(crate) fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(target_os = "macos")]
pub(crate) fn shell_quote_sh(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn run_systemctl_user<const N: usize>(args: [&str; N]) -> Result<()> {
    let output = ProcessCommand::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .with_context(|| format!("run systemctl --user {}", args.join(" ")))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    anyhow::bail!(
        "systemctl --user {} failed: {}{}{}",
        args.join(" "),
        stderr.trim(),
        if stderr.trim().is_empty() || stdout.trim().is_empty() {
            ""
        } else {
            " | "
        },
        stdout.trim()
    )
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn run_systemctl_user_for<const N: usize>(
    username: &str,
    runtime_dir: &Path,
    args: [&str; N],
) -> Result<()> {
    let output = ProcessCommand::new("runuser")
        .args(["-u", username, "--", "env"])
        .arg(format!("XDG_RUNTIME_DIR={}", runtime_dir.display()))
        .arg("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .with_context(|| format!("run systemctl --user {} for {}", args.join(" "), username))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    anyhow::bail!(
        "systemctl --user {} for {} failed: {}{}{}",
        args.join(" "),
        username,
        stderr.trim(),
        if stderr.trim().is_empty() || stdout.trim().is_empty() {
            ""
        } else {
            " | "
        },
        stdout.trim()
    )
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn shell_escape_path(value: &Path) -> String {
    shell_escape_str(&value.display().to_string())
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn shell_escape_str(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-'))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

pub(crate) fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

pub(in crate::commands) fn watcher_command_requires_config_load(command: &WatcherCommand) -> bool {
    matches!(
        command,
        WatcherCommand::Run(_)
            | WatcherCommand::Manager(WatcherManagerArgs {
                command: WatcherManagerCommand::Run
            })
    )
}
