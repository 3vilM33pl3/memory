use std::{
    env, fs,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    time::Duration,
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use mem_api::{AppConfig, Profile, discover_global_config_path};
use mem_platform as platform;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPoolOptions;

#[cfg(target_os = "macos")]
use crate::commands::watch_support::write_launch_agent;
use crate::commands::{
    output::service_url,
    runtime::{default_global_config_path, packaged_service_available, run_systemctl_system},
    watch_support::{memory_binary_path, run_systemctl_user, run_systemctl_user_for, yes_no},
};

pub(crate) async fn enable_backend_service(config_path: &Path) -> Result<String> {
    let config = AppConfig::load_from_path(Some(config_path.to_path_buf()))
        .context("load config for backend service enable")?;
    let startup_output = start_backend_service_once(config_path)?;
    match wait_for_backend_health(config_path).await {
        Ok(health) => Ok(format!(
            "{startup_output}\n\n{}",
            format_backend_health_summary(&health)
        )),
        Err(start_error) => {
            let database_error = check_database_connectivity(&config).await.err();
            if !config.cluster.enabled
                && let Some(database_error) = database_error
            {
                if io::stdin().is_terminal()
                    && io::stdout().is_terminal()
                    && prompt_yes_no(&format!(
                        "Backend could not reach PostgreSQL ({database_error}). Enable relay discovery in {} and retry?",
                        config_path.display()
                    ))?
                {
                    set_cluster_enabled_in_shared_config(config_path, true)?;
                    let _ = disable_backend_service();
                    let retry_output = start_backend_service_once(config_path)?;
                    let health = wait_for_backend_health(config_path).await?;
                    return Ok(format!(
                        "Enabled relay discovery in {}.\n{}\n\n{}",
                        config_path.display(),
                        retry_output,
                        format_backend_health_summary(&health)
                    ));
                }
                anyhow::bail!(
                    "Backend did not become healthy after startup.\nLikely cause: {database_error}\nRecovery: enable relay discovery by setting [cluster].enabled = true in {} and rerun `memory service enable`.",
                    config_path.display()
                );
            }
            Err(start_error)
        }
    }
}

pub(crate) fn start_backend_service_once(config_path: &Path) -> Result<String> {
    #[cfg(not(target_os = "macos"))]
    let _ = config_path;

    #[cfg(target_os = "macos")]
    {
        let plist_path = backend_launch_agent_path()?;
        let label = backend_launch_agent_label();
        let stdout_path = user_memory_layer_log_dir()?.join("mem-service.stdout.log");
        let stderr_path = user_memory_layer_log_dir()?.join("mem-service.stderr.log");
        write_launch_agent(
            &plist_path,
            render_backend_launch_agent(config_path)?,
            label,
        )?;
        bootstrap_launch_agent(&plist_path, label)?;
        Ok(format!(
            "Installed and started backend LaunchAgent {}.\nPlist: {}\nConfig: {}\nLogs:\n- {}\n- {}\n\nManage it with:\n- memory service status\n- memory service disable\n- launchctl kickstart -k {}/{}",
            label,
            plist_path.display(),
            config_path.display(),
            stdout_path.display(),
            stderr_path.display(),
            launchctl_domain_target()?,
            label,
        ))
    }

    #[cfg(not(target_os = "macos"))]
    {
        run_systemctl_system(["daemon-reload"])?;
        run_systemctl_system(["enable", "--now", "memory-layer.service"])?;
        Ok("Enabled memory-layer.service".to_string())
    }
}

pub(crate) fn preview_enable_backend_service(config_path: &Path) -> String {
    #[cfg(target_os = "macos")]
    {
        match backend_launch_agent_path() {
            Ok(plist_path) => format!(
                "Dry run: would install and start backend LaunchAgent {}.\nPlist: {}\nConfig: {}",
                backend_launch_agent_label(),
                plist_path.display(),
                config_path.display()
            ),
            Err(_) => format!(
                "Dry run: would install and start the backend LaunchAgent with config {}",
                config_path.display()
            ),
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        format!(
            "Dry run: would run `systemctl enable --now memory-layer.service` using config {}",
            config_path.display()
        )
    }
}

pub(crate) async fn enable_relay_discovery_and_restart_backend() -> Result<String> {
    let config_path = discover_global_config_path().unwrap_or_else(default_global_config_path);
    set_cluster_enabled_in_shared_config(&config_path, true)?;
    let _ = disable_backend_service();
    enable_backend_service(&config_path).await
}

pub(crate) fn disable_backend_service() -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        let plist_path = backend_launch_agent_path()?;
        let label = backend_launch_agent_label();
        let _ = bootout_launch_agent(&plist_path, label);
        if plist_path.exists() {
            fs::remove_file(&plist_path)
                .with_context(|| format!("remove {}", plist_path.display()))?;
        }
        Ok(format!(
            "Disabled backend LaunchAgent {}.\nRemoved plist: {}",
            label,
            plist_path.display()
        ))
    }

    #[cfg(not(target_os = "macos"))]
    {
        run_systemctl_system(["disable", "--now", "memory-layer.service"])?;
        Ok("Disabled memory-layer.service".to_string())
    }
}

pub(crate) fn preview_disable_backend_service(config_path: &Path) -> String {
    #[cfg(target_os = "macos")]
    {
        match backend_launch_agent_path() {
            Ok(plist_path) => format!(
                "Dry run: would disable backend LaunchAgent {} and remove {}\nConfig: {}",
                backend_launch_agent_label(),
                plist_path.display(),
                config_path.display()
            ),
            Err(_) => format!(
                "Dry run: would disable the backend LaunchAgent configured by {}",
                config_path.display()
            ),
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        format!(
            "Dry run: would run `systemctl disable --now memory-layer.service` using config {}",
            config_path.display()
        )
    }
}

pub(crate) fn backend_service_status(config_path: &Path) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        let plist_path = backend_launch_agent_path()?;
        let label = backend_launch_agent_label();
        let status = launch_agent_status(label)?;
        Ok(format!(
            "Backend service:\n- label: {}\n- plist: {}\n- config: {}\n- installed: {}\n- running: {}\n\nInspect with:\n- launchctl print {}/{}\n- tail -f {}",
            label,
            plist_path.display(),
            config_path.display(),
            yes_no(plist_path.exists() || status.loaded),
            yes_no(status.running),
            launchctl_domain_target()?,
            label,
            user_memory_layer_log_dir()?
                .join("mem-service.stderr.log")
                .display(),
        ))
    }

    #[cfg(not(target_os = "macos"))]
    {
        let is_installed = packaged_service_available();
        let is_enabled = run_systemctl_system(["is-enabled", "memory-layer.service"]).is_ok();
        let is_active = run_systemctl_system(["is-active", "memory-layer.service"]).is_ok();
        Ok(format!(
            "Backend service:\n- unit: memory-layer.service\n- config: {}\n- installed: {}\n- enabled: {}\n- active: {}\n\nInspect with:\n- systemctl status memory-layer.service",
            config_path.display(),
            yes_no(is_installed),
            yes_no(is_enabled),
            yes_no(is_active),
        ))
    }
}

const TUI_RESTART_MARKER_FILE: &str = "tui-restart-required.json";
#[cfg(not(target_os = "macos"))]
const LINUX_GLOBAL_TUI_RESTART_MARKER: &str = "/var/lib/memory-layer/tui-restart-required.json";
#[cfg(target_os = "macos")]
const MACOS_GLOBAL_TUI_RESTART_MARKER: &str =
    "/usr/local/var/memory-layer/tui-restart-required.json";

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ServiceRestartReport {
    pub(crate) dry_run: bool,
    pub(crate) marked_tui_restart: bool,
    pub(crate) marker_paths: Vec<String>,
    pub(crate) operations: Vec<ServiceRestartOperation>,
}

impl ServiceRestartReport {
    pub(crate) fn summary(&self) -> String {
        let mut lines = vec![format!(
            "Memory Layer service restart{}:",
            if self.dry_run { " dry run" } else { "" }
        )];
        for operation in &self.operations {
            lines.push(format!(
                "- {} [{}]: {}{}",
                operation.name,
                operation.manager,
                operation.action,
                operation
                    .message
                    .as_deref()
                    .filter(|value| !value.is_empty())
                    .map(|value| format!(" ({value})"))
                    .unwrap_or_default()
            ));
        }
        if self.marked_tui_restart {
            lines.push(format!(
                "TUI restart marker written: {}",
                self.marker_paths.join(", ")
            ));
        }
        lines.join("\n")
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ServiceRestartOperation {
    pub(crate) name: String,
    pub(crate) manager: String,
    pub(crate) active: bool,
    pub(crate) action: String,
    pub(crate) success: bool,
    pub(crate) message: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct TuiRestartMarker {
    pub(crate) version: String,
    pub(crate) marked_at: DateTime<Utc>,
    pub(crate) reason: String,
    pub(crate) binary_path: String,
    pub(crate) restarted_services: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TuiRestartNotice {
    pub(crate) marker_path: PathBuf,
    pub(crate) version: String,
    pub(crate) reason: String,
}

pub(crate) fn restart_all_memory_services(
    dry_run: bool,
    mark_tui_restart: bool,
) -> Result<ServiceRestartReport> {
    let mut operations = Vec::new();
    restart_platform_services(dry_run, &mut operations)?;
    let restarted_services = operations
        .iter()
        .filter(|operation| {
            operation.active
                && (operation.success || dry_run)
                && (operation.action == "restart" || operation.action == "would-restart")
        })
        .map(|operation| operation.name.clone())
        .collect::<Vec<_>>();
    let marker_paths = if mark_tui_restart && !dry_run {
        write_tui_restart_marker("install-or-upgrade", restarted_services)?
    } else {
        Vec::new()
    };
    Ok(ServiceRestartReport {
        dry_run,
        marked_tui_restart: mark_tui_restart && !dry_run && !marker_paths.is_empty(),
        marker_paths: marker_paths
            .into_iter()
            .map(|path| path.display().to_string())
            .collect(),
        operations,
    })
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn restart_platform_services(
    dry_run: bool,
    operations: &mut Vec<ServiceRestartOperation>,
) -> Result<()> {
    for unit in ["memory-layer.service", "memory-watch.service"] {
        restart_systemd_system_unit_if_active(unit, dry_run, operations);
    }
    for scope in active_memory_user_unit_scopes() {
        for unit in &scope.units {
            restart_systemd_user_unit_if_active(&scope, unit, dry_run, operations);
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub(crate) fn restart_platform_services(
    dry_run: bool,
    operations: &mut Vec<ServiceRestartOperation>,
) -> Result<()> {
    for label in active_launch_agent_labels()? {
        restart_launch_agent_if_loaded(&label, dry_run, operations);
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn restart_systemd_system_unit_if_active(
    unit: &str,
    dry_run: bool,
    operations: &mut Vec<ServiceRestartOperation>,
) {
    let active = run_systemctl_system(["is-active", "--quiet", unit]).is_ok();
    if !active {
        operations.push(ServiceRestartOperation {
            name: unit.to_string(),
            manager: "systemd-system".to_string(),
            active,
            action: "skip-inactive".to_string(),
            success: true,
            message: None,
        });
        return;
    }
    if dry_run {
        operations.push(ServiceRestartOperation {
            name: unit.to_string(),
            manager: "systemd-system".to_string(),
            active,
            action: "would-restart".to_string(),
            success: true,
            message: None,
        });
        return;
    }
    let result = run_systemctl_system(["restart", unit]);
    operations.push(ServiceRestartOperation {
        name: unit.to_string(),
        manager: "systemd-system".to_string(),
        active,
        action: "restart".to_string(),
        success: result.is_ok(),
        message: result.err().map(|error| error.to_string()),
    });
}

#[cfg(not(target_os = "macos"))]
#[derive(Debug, Clone)]
pub(crate) struct SystemdUserScope {
    pub(crate) manager_label: String,
    pub(crate) username: Option<String>,
    pub(crate) runtime_dir: Option<PathBuf>,
    pub(crate) units: Vec<String>,
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn active_memory_user_unit_scopes() -> Vec<SystemdUserScope> {
    if running_as_root() {
        let scopes = active_logged_in_user_memory_unit_scopes();
        if !scopes.is_empty() {
            return scopes;
        }
    }
    active_current_user_memory_units()
        .into_iter()
        .next()
        .map(|units| SystemdUserScope {
            manager_label: "systemd-user".to_string(),
            username: None,
            runtime_dir: None,
            units,
        })
        .into_iter()
        .collect()
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn running_as_root() -> bool {
    ProcessCommand::new("id")
        .arg("-u")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim() == "0")
        .unwrap_or(false)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn active_logged_in_user_memory_unit_scopes() -> Vec<SystemdUserScope> {
    let Ok(entries) = fs::read_dir("/run/user") else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let runtime_dir = entry.path();
            let uid = runtime_dir.file_name()?.to_str()?.to_string();
            let username = username_for_uid(&uid)?;
            let units = active_user_memory_units_for(&username, Some(&runtime_dir));
            (!units.is_empty()).then_some(SystemdUserScope {
                manager_label: format!("systemd-user:{username}"),
                username: Some(username),
                runtime_dir: Some(runtime_dir),
                units,
            })
        })
        .collect()
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn username_for_uid(uid: &str) -> Option<String> {
    let output = ProcessCommand::new("getent")
        .args(["passwd", uid])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .split(':')
        .next()
        .map(str::to_string)
        .filter(|value| !value.is_empty())
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn active_current_user_memory_units() -> Option<Vec<String>> {
    let units = active_user_memory_units_for("", None);
    (!units.is_empty()).then_some(units)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn active_user_memory_units_for(
    username: &str,
    runtime_dir: Option<&Path>,
) -> Vec<String> {
    let mut command = if let Some(runtime_dir) = runtime_dir {
        let mut command = ProcessCommand::new("runuser");
        command
            .args(["-u", username, "--", "env"])
            .arg(format!("XDG_RUNTIME_DIR={}", runtime_dir.display()))
            .args([
                "systemctl",
                "--user",
                "list-units",
                "--type=service",
                "--state=active",
                "--no-legend",
                "memory-watch*.service",
            ]);
        command
    } else {
        let mut command = ProcessCommand::new("systemctl");
        command.args([
            "--user",
            "list-units",
            "--type=service",
            "--state=active",
            "--no-legend",
            "memory-watch*.service",
        ]);
        command
    };
    let output = command.output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    parse_systemd_unit_names(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn parse_systemd_unit_names(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|unit| unit.starts_with("memory-watch") && unit.ends_with(".service"))
        .map(ToString::to_string)
        .collect()
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn restart_systemd_user_unit_if_active(
    scope: &SystemdUserScope,
    unit: &str,
    dry_run: bool,
    operations: &mut Vec<ServiceRestartOperation>,
) {
    if dry_run {
        operations.push(ServiceRestartOperation {
            name: unit.to_string(),
            manager: scope.manager_label.clone(),
            active: true,
            action: "would-restart".to_string(),
            success: true,
            message: None,
        });
        return;
    }
    let result = if let (Some(username), Some(runtime_dir)) = (&scope.username, &scope.runtime_dir)
    {
        run_systemctl_user_for(username, runtime_dir, ["restart", unit])
    } else {
        run_systemctl_user(["restart", unit])
    };
    operations.push(ServiceRestartOperation {
        name: unit.to_string(),
        manager: scope.manager_label.clone(),
        active: true,
        action: "restart".to_string(),
        success: result.is_ok(),
        message: result.err().map(|error| error.to_string()),
    });
}

#[cfg(target_os = "macos")]
pub(crate) fn active_launch_agent_labels() -> Result<Vec<String>> {
    let mut labels = vec![
        backend_launch_agent_label().to_string(),
        watch_manager_launch_agent_label().to_string(),
    ];
    if let Some(dir) = platform::user_launch_agents_dir() {
        if dir.is_dir() {
            for entry in fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
                let path = entry?.path();
                let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
                    continue;
                };
                if file_name.starts_with("com.memory-layer.memory-watch")
                    && file_name.ends_with(".plist")
                {
                    labels.push(file_name.trim_end_matches(".plist").to_string());
                }
            }
        }
    }
    labels.sort();
    labels.dedup();
    Ok(labels)
}

#[cfg(target_os = "macos")]
pub(crate) fn restart_launch_agent_if_loaded(
    label: &str,
    dry_run: bool,
    operations: &mut Vec<ServiceRestartOperation>,
) {
    let status = launch_agent_status(label).unwrap_or_default();
    if !status.loaded {
        operations.push(ServiceRestartOperation {
            name: label.to_string(),
            manager: "launchctl".to_string(),
            active: false,
            action: "skip-unloaded".to_string(),
            success: true,
            message: None,
        });
        return;
    }
    if dry_run {
        operations.push(ServiceRestartOperation {
            name: label.to_string(),
            manager: "launchctl".to_string(),
            active: true,
            action: "would-restart".to_string(),
            success: true,
            message: None,
        });
        return;
    }
    let target = format!(
        "{}/{}",
        launchctl_domain_target().unwrap_or_else(|_| "gui/unknown".to_string()),
        label
    );
    let result = run_launchctl(["kickstart", "-k", &target]);
    operations.push(ServiceRestartOperation {
        name: label.to_string(),
        manager: "launchctl".to_string(),
        active: true,
        action: "restart".to_string(),
        success: result.is_ok(),
        message: result.err().map(|error| error.to_string()),
    });
}

pub(crate) fn tui_restart_marker_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(dir) = platform::preferred_user_state_dir() {
        paths.push(dir.join(TUI_RESTART_MARKER_FILE));
    }
    #[cfg(not(target_os = "macos"))]
    paths.push(PathBuf::from(LINUX_GLOBAL_TUI_RESTART_MARKER));
    #[cfg(target_os = "macos")]
    paths.push(PathBuf::from(MACOS_GLOBAL_TUI_RESTART_MARKER));
    paths.sort();
    paths.dedup();
    paths
}

pub(crate) fn write_tui_restart_marker(
    reason: &str,
    restarted_services: Vec<String>,
) -> Result<Vec<PathBuf>> {
    let marker = TuiRestartMarker {
        version: Profile::detect().display_version(env!("CARGO_PKG_VERSION")),
        marked_at: Utc::now(),
        reason: reason.to_string(),
        binary_path: memory_binary_path()
            .unwrap_or_else(|_| PathBuf::from("memory"))
            .display()
            .to_string(),
        restarted_services,
    };
    let contents = serde_json::to_vec_pretty(&marker)?;
    let mut written = Vec::new();
    let mut last_error: Option<anyhow::Error> = None;
    for path in tui_restart_marker_paths() {
        if let Some(parent) = path.parent()
            && let Err(error) = fs::create_dir_all(parent)
        {
            last_error = Some(error.into());
            continue;
        }
        match fs::write(&path, &contents) {
            Ok(()) => written.push(path),
            Err(error) => last_error = Some(error.into()),
        }
    }
    if written.is_empty()
        && let Some(error) = last_error
    {
        return Err(error).context("write TUI restart marker");
    }
    Ok(written)
}

pub(crate) fn load_tui_restart_notice(
    startup_at: DateTime<Utc>,
    running_version: &str,
) -> Option<TuiRestartNotice> {
    newest_tui_restart_notice(startup_at, running_version, tui_restart_marker_paths())
}

pub(crate) fn newest_tui_restart_notice(
    startup_at: DateTime<Utc>,
    running_version: &str,
    marker_paths: Vec<PathBuf>,
) -> Option<TuiRestartNotice> {
    marker_paths
        .into_iter()
        .filter_map(|path| {
            let contents = fs::read_to_string(&path).ok()?;
            let marker: TuiRestartMarker = serde_json::from_str(&contents).ok()?;
            if !restart_marker_requires_restart(
                &marker.version,
                running_version,
                marker.marked_at,
                startup_at,
            ) {
                return None;
            }
            Some(TuiRestartNotice {
                marker_path: path,
                version: marker.version,
                reason: marker.reason,
            })
        })
        .max_by_key(|notice| {
            fs::read_to_string(&notice.marker_path)
                .ok()
                .and_then(|contents| serde_json::from_str::<TuiRestartMarker>(&contents).ok())
                .map(|marker| marker.marked_at)
        })
}

pub(crate) fn restart_marker_requires_restart(
    marker_version: &str,
    running_version: &str,
    marked_at: DateTime<Utc>,
    startup_at: DateTime<Utc>,
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

pub(crate) fn version_profile_suffix(version: &str) -> &'static str {
    if version.trim().ends_with("-dev") {
        "dev"
    } else {
        "prod"
    }
}

pub(crate) async fn wait_for_backend_health(config_path: &Path) -> Result<serde_json::Value> {
    let config = AppConfig::load_from_path(Some(config_path.to_path_buf()))
        .context("reload config after backend startup")?;
    let client = Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .context("build backend startup http client")?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut last_error = None;
    while tokio::time::Instant::now() < deadline {
        match client.get(service_url(&config, "/healthz")).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    return response
                        .json()
                        .await
                        .context("parse backend health response");
                }
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                last_error = Some(anyhow::anyhow!("health endpoint returned {status} {body}"));
            }
            Err(error) => last_error = Some(error.into()),
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("backend health endpoint did not respond")))
}

pub(crate) async fn check_database_connectivity(config: &AppConfig) -> Result<()> {
    PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(3))
        .connect(&config.database.url)
        .await
        .map(drop)
        .context("connect postgres")
}

pub(crate) fn format_backend_health_summary(health: &serde_json::Value) -> String {
    let role = health
        .get("role")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    let status = health
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    let database = health
        .get("database")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    let mut lines = vec![
        "Backend health:".to_string(),
        format!("- role: {role}"),
        format!("- status: {status}"),
        format!("- database: {database}"),
    ];
    if let Some(upstream) = health.get("upstream") {
        lines.push(format!("- upstream: {upstream}"));
    }
    lines.join("\n")
}

pub(crate) fn prompt_yes_no(prompt: &str) -> Result<bool> {
    print!("{prompt} [y/N]: ");
    io::stdout().flush().context("flush prompt")?;
    let mut input = String::new();
    io::stdin().read_line(&mut input).context("read prompt")?;
    Ok(matches!(
        input.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

pub(crate) fn set_cluster_enabled_in_shared_config(path: &Path, enabled: bool) -> Result<()> {
    let mut content = if path.exists() {
        fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?
    } else {
        String::new()
    };
    let mut lines = content.lines().map(ToOwned::to_owned).collect::<Vec<_>>();
    let mut cluster_header = None;
    let mut enabled_line = None;
    let mut in_cluster = false;

    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_cluster = trimmed == "[cluster]";
            if in_cluster {
                cluster_header = Some(index);
            }
            continue;
        }
        if in_cluster && trimmed.starts_with("enabled = ") {
            enabled_line = Some(index);
            break;
        }
    }

    let enabled_value = format!("enabled = {enabled}");
    if let Some(index) = enabled_line {
        lines[index] = enabled_value;
    } else if let Some(index) = cluster_header {
        lines.insert(index + 1, enabled_value);
    } else {
        if !lines.is_empty() && !lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push("[cluster]".to_string());
        lines.push(enabled_value);
        lines.push("# advertise_addr = \"192.168.1.50:4040\"".to_string());
        lines.push("# discovery_multicast_addr = \"239.255.42.99:4042\"".to_string());
        lines.push("# announce_interval = \"5s\"".to_string());
        lines.push("# peer_ttl = \"15s\"".to_string());
        lines.push("# priority = 100".to_string());
    }

    content = lines.join("\n");
    if !content.ends_with('\n') {
        content.push('\n');
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(path, content).with_context(|| format!("write {}", path.display()))
}
