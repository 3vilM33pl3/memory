use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    time::{Duration, Instant},
};

use super::app::*;
use anyhow::Result;
use crossterm::{
    event::{KeyCode, KeyEvent},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use mem_api::{NamedCount, Profile, ProjectOverviewResponse};
use mem_platform::preferred_user_state_dir;
use ratatui::{Terminal, backend::CrosstermBackend};

pub(super) fn should_quit(key: KeyEvent, app: &App) -> bool {
    matches!(app.chrome.input_mode, InputMode::Normal) && matches!(key.code, KeyCode::Char('q'))
}

pub(super) fn should_attempt_stream_reconnect(
    stream_connected: bool,
    stream_connecting: bool,
    last_attempt: Instant,
) -> bool {
    !stream_connected && !stream_connecting && last_attempt.elapsed() >= Duration::from_secs(1)
}

pub(super) fn empty_overview(project: String) -> ProjectOverviewResponse {
    ProjectOverviewResponse {
        project,
        service_status: "unknown".to_string(),
        database_status: "unknown".to_string(),
        memory_entries_total: 0,
        active_memories: 0,
        archived_memories: 0,
        raw_captures_total: 0,
        uncurated_raw_captures: 0,
        tasks_total: 0,
        sessions_total: 0,
        curation_runs_total: 0,
        recent_memories_7d: 0,
        recent_captures_7d: 0,
        high_confidence_memories: 0,
        medium_confidence_memories: 0,
        low_confidence_memories: 0,
        embedding_chunks_total: 0,
        fresh_embedding_chunks: 0,
        stale_embedding_chunks: 0,
        missing_embedding_chunks: 0,
        embedding_spaces_total: 0,
        active_embedding_provider: None,
        active_embedding_model: None,
        last_memory_at: None,
        last_capture_at: None,
        last_curation_at: None,
        oldest_uncurated_capture_age_hours: None,
        memory_type_breakdown: Vec::new(),
        source_kind_breakdown: Vec::new(),
        top_tags: Vec::<NamedCount>::new(),
        top_files: Vec::<NamedCount>::new(),
        pending_replacement_proposals: 0,
        automation: None,
        watchers: None,
    }
}

pub(super) fn load_manager_footer_status(profile: Profile) -> ManagerFooterStatus {
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
    let state = derive_manager_state(
        unit_installed,
        unit_enabled,
        unit_active,
        foreground_active,
        state_file.is_some() || manager_unit_path(profile).is_some(),
    );
    let mode = if unit_active {
        Some(ManagerMode::Service)
    } else if foreground_active {
        Some(ManagerMode::Foreground)
    } else {
        None
    };
    ManagerFooterStatus {
        state,
        tracked_sessions,
        warning_count,
        mode,
        runtime_mode,
        last_reconcile_reason,
        event_count,
        fallback_scan_count,
    }
}

pub(super) fn derive_manager_state(
    unit_installed: bool,
    unit_enabled: bool,
    unit_active: bool,
    foreground_active: bool,
    can_probe: bool,
) -> ManagerState {
    if unit_active || foreground_active {
        ManagerState::Active
    } else if unit_installed || unit_enabled {
        ManagerState::Installed
    } else if can_probe {
        ManagerState::Off
    } else {
        ManagerState::Error
    }
}

pub(super) fn foreground_manager_process_running(profile: Profile) -> bool {
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
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let Some(pid) = parts.next() else {
            continue;
        };
        if pid == current_pid {
            continue;
        }
        let command = parts.collect::<Vec<_>>().join(" ");
        if command_is_manager_for_profile(&command, profile) {
            return true;
        }
    }
    false
}

pub(super) fn command_is_manager_for_profile(command: &str, profile: Profile) -> bool {
    if !(command.contains(" watcher manager run")
        || command.ends_with("watcher manager run")
        || command.contains("watcher manager run "))
    {
        return false;
    }
    match profile {
        Profile::Prod => !command_looks_dev_stack(command),
        Profile::Dev => command_looks_dev_stack(command),
    }
}

pub(super) fn command_looks_dev_stack(command: &str) -> bool {
    command.contains("target/debug/memory")
        || command.contains("target/release/memory")
        || command.contains("MEMORY_LAYER_PROFILE=dev")
        || command.contains("MEMORY_LAYER_PROFILE=\"dev\"")
        || command.contains("MEMORY_LAYER_PROFILE='dev'")
        || command.contains("config.dev.toml")
        || command.contains("/.mem/runtime/dev/")
}

#[cfg(not(target_os = "macos"))]
pub(super) fn linux_manager_unit_path() -> Option<PathBuf> {
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

pub(super) fn manager_unit_path(profile: Profile) -> Option<PathBuf> {
    if profile == Profile::Dev {
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

pub(super) fn load_manager_state_file(profile: Profile) -> Option<ManagerStateFile> {
    let filename = match profile {
        Profile::Dev => "watcher-manager-state-dev.json",
        Profile::Prod => "watcher-manager-state.json",
    };
    let path = preferred_user_state_dir()?.join(filename);
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

#[cfg(not(target_os = "macos"))]
pub(super) fn systemctl_user_check(action: &str, unit: &str) -> bool {
    ProcessCommand::new("systemctl")
        .args(["--user", action, unit])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub(super) fn manager_service_enabled(profile: Profile) -> bool {
    if profile == Profile::Dev {
        return false;
    }

    #[cfg(target_os = "macos")]
    {
        let Some(path) = mem_platform::watch_manager_launch_agent_path() else {
            return false;
        };
        path.exists()
    }

    #[cfg(not(target_os = "macos"))]
    {
        systemctl_user_check("is-enabled", "memory-watch-manager.service")
    }
}

pub(super) fn manager_service_running(profile: Profile) -> bool {
    if profile == Profile::Dev {
        return false;
    }

    #[cfg(target_os = "macos")]
    {
        let Some(label) = Some(mem_platform::watch_manager_launch_agent_label()) else {
            return false;
        };
        launchctl_print_succeeds(label)
    }

    #[cfg(not(target_os = "macos"))]
    {
        systemctl_user_check("is-active", "memory-watch-manager.service")
    }
}

#[cfg(target_os = "macos")]
pub(super) fn launchctl_print_succeeds(label: &str) -> bool {
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

pub(super) fn detect_tool_versions(profile: Profile) -> ToolVersions {
    let version = profile.display_version(env!("CARGO_PKG_VERSION"));
    ToolVersions {
        mem_cli: version.clone(),
        mem_service: version.clone(),
        watch_manager: version.clone(),
        memory_watch: version,
    }
}

pub(super) fn detect_dev_commit_label(repo_root: &Path) -> String {
    let short_hash = git_output(repo_root, &["rev-parse", "--short=12", "HEAD"]);
    let dirty = git_worktree_dirty(repo_root);
    format_dev_commit_label(short_hash.as_deref(), dirty)
}

pub(super) fn format_dev_commit_label(short_hash: Option<&str>, dirty: bool) -> String {
    let Some(short_hash) = short_hash.map(str::trim).filter(|value| !value.is_empty()) else {
        return "unknown".to_string();
    };
    if dirty {
        format!("{short_hash}+dirty")
    } else {
        short_hash.to_string()
    }
}

fn git_worktree_dirty(repo_root: &Path) -> bool {
    git_output(repo_root, &["status", "--porcelain"])
        .is_some_and(|output| !output.trim().is_empty())
}

fn git_output(repo_root: &Path, args: &[&str]) -> Option<String> {
    let output = ProcessCommand::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(super) fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

pub(super) fn restore_terminal(mut terminal: Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}
