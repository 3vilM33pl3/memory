use std::{
    env, fs,
    path::{Component, Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
    thread,
};

use anyhow::{Context, Result};
use sysinfo::{
    MINIMUM_CPU_UPDATE_INTERVAL, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind,
};

static PROCESS_SYSTEM: OnceLock<Mutex<System>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq)]
pub struct ProcessSnapshot {
    pub pid: u32,
    pub parent_pid: u32,
    pub memory_kb: u64,
    pub cpu_percent: f64,
    pub command: String,
    pub cwd: Option<PathBuf>,
    pub started_at_ms: u64,
}

pub fn process_snapshots() -> Vec<ProcessSnapshot> {
    let process_system = PROCESS_SYSTEM.get_or_init(|| {
        let mut system = System::new_all();
        thread::sleep(MINIMUM_CPU_UPDATE_INTERVAL);
        refresh_process_system(&mut system);
        Mutex::new(system)
    });
    let mut system = process_system
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    refresh_process_system(&mut system);
    system
        .processes()
        .iter()
        .map(|(pid, process)| {
            let command = if process.cmd().is_empty() {
                process.name().to_string_lossy().into_owned()
            } else {
                process
                    .cmd()
                    .iter()
                    .map(|part| quote_snapshot_argument(&part.to_string_lossy()))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            ProcessSnapshot {
                pid: pid.as_u32(),
                parent_pid: process.parent().map(|pid| pid.as_u32()).unwrap_or_default(),
                memory_kb: process.memory() / 1024,
                cpu_percent: f64::from(process.cpu_usage()),
                command,
                cwd: process.cwd().map(Path::to_path_buf),
                started_at_ms: process.start_time().saturating_mul(1000),
            }
        })
        .collect()
}

fn refresh_process_system(system: &mut System) {
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_memory()
            .with_cpu()
            .with_exe(UpdateKind::OnlyIfNotSet)
            .with_cmd(UpdateKind::OnlyIfNotSet)
            .with_cwd(UpdateKind::Always),
    );
}

fn quote_snapshot_argument(value: &str) -> String {
    if value.chars().any(char::is_whitespace) {
        format!("\"{}\"", value.replace('"', "\\\""))
    } else {
        value.to_string()
    }
}

pub fn process_is_alive(pid: u32) -> bool {
    process_snapshots().iter().any(|process| process.pid == pid)
}

pub fn user_home_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        if let Ok(profile) = env::var("USERPROFILE")
            && !profile.trim().is_empty()
        {
            return Some(PathBuf::from(profile));
        }
        if let (Ok(drive), Ok(path)) = (env::var("HOMEDRIVE"), env::var("HOMEPATH"))
            && (!drive.trim().is_empty() || !path.trim().is_empty())
        {
            return Some(PathBuf::from(format!("{drive}{path}")));
        }
    }

    env::var("HOME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
}

#[cfg(target_os = "windows")]
pub fn windows_data_dir() -> Option<PathBuf> {
    env::var("LOCALAPPDATA")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var("APPDATA")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .map(PathBuf::from)
        })
        .or_else(|| user_home_dir().map(|home| home.join("AppData").join("Local")))
        .map(|base| base.join("memory-layer"))
}

pub fn preferred_global_config_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        windows_data_dir()
            .unwrap_or_else(|| PathBuf::from(r"C:\Users\Default\AppData\Local\memory-layer"))
            .join("memory-layer.toml")
    }

    #[cfg(not(target_os = "windows"))]
    {
        if let Ok(config_home) = env::var("XDG_CONFIG_HOME") {
            return PathBuf::from(config_home)
                .join("memory-layer")
                .join("memory-layer.toml");
        }

        #[cfg(target_os = "macos")]
        if let Some(path) = macos_app_support_dir() {
            return path.join("memory-layer.toml");
        }

        if let Ok(home) = env::var("HOME") {
            PathBuf::from(home)
                .join(".config")
                .join("memory-layer")
                .join("memory-layer.toml")
        } else {
            PathBuf::from("/etc/memory-layer/memory-layer.toml")
        }
    }
}

pub fn discover_existing_global_config_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let candidate = windows_data_dir()?.join("memory-layer.toml");
        candidate.is_file().then_some(candidate)
    }

    #[cfg(not(target_os = "windows"))]
    {
        if let Ok(config_home) = env::var("XDG_CONFIG_HOME") {
            let candidate = PathBuf::from(config_home)
                .join("memory-layer")
                .join("memory-layer.toml");
            if candidate.is_file() {
                return Some(candidate);
            }
        }

        #[cfg(target_os = "macos")]
        if let Some(candidate) = macos_app_support_dir().map(|dir| dir.join("memory-layer.toml")) {
            if candidate.is_file() {
                return Some(candidate);
            }
        }

        if let Ok(home) = env::var("HOME") {
            let candidate = PathBuf::from(home)
                .join(".config")
                .join("memory-layer")
                .join("memory-layer.toml");
            if candidate.is_file() {
                return Some(candidate);
            }
        }

        let system_candidate = PathBuf::from("/etc/memory-layer/memory-layer.toml");
        if system_candidate.is_file() {
            return Some(system_candidate);
        }

        None
    }
}

pub fn preferred_user_env_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        Some(windows_data_dir()?.join("memory-layer.env"))
    }

    #[cfg(target_os = "macos")]
    {
        return Some(macos_app_support_dir()?.join("memory-layer.env"));
    }

    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        if let Ok(config_home) = env::var("XDG_CONFIG_HOME") {
            return Some(
                PathBuf::from(config_home)
                    .join("memory-layer")
                    .join("memory-layer.env"),
            );
        }
        let home = env::var("HOME").ok()?;
        Some(
            PathBuf::from(home)
                .join(".config")
                .join("memory-layer")
                .join("memory-layer.env"),
        )
    }
}

pub fn preferred_user_state_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        return Some(macos_app_support_dir()?);
    }

    #[cfg(target_os = "windows")]
    {
        windows_data_dir()
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Ok(state_home) = env::var("XDG_STATE_HOME") {
            return Some(PathBuf::from(state_home).join("memory-layer"));
        }
        let home = env::var("HOME").ok()?;
        Some(
            PathBuf::from(home)
                .join(".local")
                .join("state")
                .join("memory-layer"),
        )
    }
}

pub fn preferred_user_cache_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = env::var("HOME").ok()?;
        return Some(
            PathBuf::from(home)
                .join("Library")
                .join("Caches")
                .join("memory-layer"),
        );
    }

    #[cfg(target_os = "windows")]
    {
        Some(windows_data_dir()?.join("cache"))
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Ok(cache_home) = env::var("XDG_CACHE_HOME") {
            return Some(PathBuf::from(cache_home).join("memory-layer"));
        }
        let home = env::var("HOME").ok()?;
        Some(PathBuf::from(home).join(".cache").join("memory-layer"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectPaths {
    pub key: String,
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl ProjectPaths {
    pub fn config_path(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn dev_config_path(&self) -> PathBuf {
        self.config_dir.join("config.dev.toml")
    }

    pub fn env_path(&self) -> PathBuf {
        self.config_dir.join("memory-layer.env")
    }

    pub fn project_path(&self) -> PathBuf {
        self.config_dir.join("project.toml")
    }

    pub fn runtime_dir(&self) -> PathBuf {
        self.state_dir.join("runtime")
    }

    pub fn cache_index_dir(&self) -> PathBuf {
        self.cache_dir.join("index")
    }
}

pub fn project_paths(repo_root: &Path, slug: &str) -> Option<ProjectPaths> {
    let config_base = preferred_project_config_base_dir()?;
    let state_base = preferred_user_state_dir()?.join("projects");
    let cache_base = preferred_user_cache_dir()?.join("projects");
    let key = project_storage_key(repo_root, slug);
    Some(ProjectPaths {
        config_dir: config_base.join(&key),
        state_dir: state_base.join(&key),
        cache_dir: cache_base.join(&key),
        key,
    })
}

pub fn preferred_project_config_base_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        return Some(macos_app_support_dir()?.join("projects"));
    }

    #[cfg(target_os = "windows")]
    {
        Some(windows_data_dir()?.join("projects"))
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Ok(config_home) = env::var("XDG_CONFIG_HOME") {
            return Some(
                PathBuf::from(config_home)
                    .join("memory-layer")
                    .join("projects"),
            );
        }
        let home = env::var("HOME").ok()?;
        Some(
            PathBuf::from(home)
                .join(".config")
                .join("memory-layer")
                .join("projects"),
        )
    }
}

pub fn project_storage_key(repo_root: &Path, slug: &str) -> String {
    let identity_path = git_common_dir(repo_root)
        .or_else(|| canonicalize_lossy(repo_root))
        .unwrap_or_else(|| repo_root.to_path_buf());
    let hash = stable_path_hash(&identity_path);
    format!("{}-{:016x}", sanitize_project_slug(slug), hash)
}

pub fn discover_project_root(start: &Path) -> Option<PathBuf> {
    for directory in start.ancestors() {
        if directory.join(".mem").join("project.toml").is_file()
            || directory
                .join(".agents")
                .join("memory-layer.toml")
                .is_file()
            || directory.join(".git").exists()
        {
            return Some(directory.to_path_buf());
        }
    }
    None
}

pub fn git_common_dir(repo_root: &Path) -> Option<PathBuf> {
    let git_path = repo_root.join(".git");
    if git_path.is_dir() {
        return canonicalize_lossy(&git_path);
    }
    let content = fs::read_to_string(&git_path).ok()?;
    let gitdir = content.trim().strip_prefix("gitdir:")?.trim();
    let gitdir_path = PathBuf::from(gitdir);
    let absolute = if gitdir_path.is_absolute() {
        gitdir_path
    } else {
        repo_root.join(gitdir_path)
    };
    let canonical = canonicalize_lossy(&absolute).unwrap_or(absolute);
    if path_has_component(&canonical, "worktrees") {
        let mut cursor = canonical.as_path();
        while let Some(parent) = cursor.parent() {
            if cursor
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name == "worktrees")
            {
                return canonicalize_lossy(parent).or_else(|| Some(parent.to_path_buf()));
            }
            cursor = parent;
        }
    }
    Some(canonical)
}

fn canonicalize_lossy(path: &Path) -> Option<PathBuf> {
    path.canonicalize().ok()
}

fn stable_path_hash(path: &Path) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in path.display().to_string().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn sanitize_project_slug(value: &str) -> String {
    let sanitized = sanitize_service_fragment(value.trim()).to_ascii_lowercase();
    if sanitized.is_empty() {
        "project".to_string()
    } else {
        sanitized
    }
}

fn path_has_component(path: &Path, needle: &str) -> bool {
    path.components().any(
        |component| matches!(component, Component::Normal(value) if value.to_str() == Some(needle)),
    )
}

pub fn default_shared_capnp_unix_socket() -> String {
    #[cfg(target_os = "windows")]
    {
        windows_data_dir()
            .unwrap_or_else(|| PathBuf::from(r"C:\Users\Default\AppData\Local\memory-layer"))
            .join("run")
            .join("memory-layer.capnp.sock")
            .display()
            .to_string()
    }

    #[cfg(not(target_os = "windows"))]
    {
        #[cfg(target_os = "macos")]
        if let Some(path) = macos_app_support_dir() {
            return path
                .join("run")
                .join("memory-layer.capnp.sock")
                .display()
                .to_string();
        }

        "/tmp/memory-layer.capnp.sock".to_string()
    }
}

pub fn sanitize_service_fragment(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => ch,
            _ => '-',
        })
        .collect::<String>()
}

pub fn current_username() -> String {
    env::var("MEMORY_LAYER_WRITER_IDENTITY_USER")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| env::var("USER").ok())
        .or_else(|| env::var("USERNAME").ok())
        .or_else(|| command_stdout_trimmed("whoami"))
        .map(|value| sanitize_service_fragment(value.trim()).to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown-user".to_string())
}

pub fn current_hostname() -> String {
    let hostname = env::var("MEMORY_LAYER_WRITER_IDENTITY_HOST")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            env::var(if cfg!(target_os = "windows") {
                "COMPUTERNAME"
            } else {
                "HOSTNAME"
            })
            .ok()
        });
    #[cfg(unix)]
    let hostname = hostname.or_else(|| {
        std::fs::read_to_string("/etc/hostname")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    });
    hostname
        .or_else(|| command_stdout_trimmed("hostname"))
        .map(|value| sanitize_service_fragment(value.trim()).to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown-host".to_string())
}

pub fn derive_default_writer_id(tool: &str) -> String {
    format!(
        "{}-{}-{}",
        sanitize_service_fragment(tool).to_ascii_lowercase(),
        current_username(),
        current_hostname()
    )
}

pub fn dev_mode_status_line(start_dir: Option<&Path>) -> String {
    format!("DEV MODE  commit={}", detect_dev_commit_label(start_dir))
}

pub fn detect_dev_commit_label(start_dir: Option<&Path>) -> String {
    let start_dir = start_dir
        .map(Path::to_path_buf)
        .or_else(|| env::current_dir().ok());
    let Some(start_dir) = start_dir else {
        return "unknown".to_string();
    };
    let short_hash = git_stdout_trimmed(&["rev-parse", "--short=12", "HEAD"], &start_dir);
    let dirty = git_stdout_trimmed(&["status", "--porcelain"], &start_dir)
        .is_some_and(|output| !output.trim().is_empty());
    format_dev_commit_label(short_hash.as_deref(), dirty)
}

pub fn format_dev_commit_label(short_hash: Option<&str>, dirty: bool) -> String {
    let Some(short_hash) = short_hash.map(str::trim).filter(|value| !value.is_empty()) else {
        return "unknown".to_string();
    };
    if dirty {
        format!("{short_hash}+dirty")
    } else {
        short_hash.to_string()
    }
}

fn command_stdout_trimmed(program: &str) -> Option<String> {
    let output = Command::new(program).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn git_stdout_trimmed(args: &[&str], cwd: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

pub fn watch_service_unit_name(project: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        windows_watch_task_name(project)
    }

    #[cfg(not(target_os = "windows"))]
    {
        format!(
            "memory-watch-{}.service",
            sanitize_service_fragment(project)
        )
    }
}

pub fn managed_watch_service_name(session_id: &str) -> String {
    #[cfg(target_os = "macos")]
    {
        managed_watch_launch_agent_label(session_id)
    }

    #[cfg(target_os = "windows")]
    {
        windows_managed_watch_task_name("prod", session_id)
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        format!(
            "memory-watch-codex-{}.service",
            sanitize_service_fragment(session_id)
        )
    }
}

pub fn current_exe_sibling_binary(name: &str) -> Option<PathBuf> {
    let current_exe = env::current_exe().ok()?;
    let bin_dir = current_exe.parent()?;
    #[cfg(target_os = "windows")]
    let name = if Path::new(name).extension().is_none() {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    #[cfg(not(target_os = "windows"))]
    let name = name.to_string();
    let sibling = bin_dir.join(name);
    sibling.is_file().then_some(sibling)
}

pub fn current_exe_share_subdir(name: &str) -> Option<PathBuf> {
    let current_exe = env::current_exe().ok()?;
    packaged_share_subdir_for_exe(&current_exe, name)
}

fn packaged_share_subdir_for_exe(current_exe: &Path, name: &str) -> Option<PathBuf> {
    let bin_dir = current_exe.parent()?;
    let adjacent = bin_dir.join("share").join("memory-layer").join(name);
    if adjacent.is_dir() {
        return Some(adjacent);
    }
    let prefix = bin_dir.parent()?;
    let candidate = prefix.join("share").join("memory-layer").join(name);
    candidate.is_dir().then_some(candidate)
}

pub fn packaged_system_service_available() -> bool {
    #[cfg(target_os = "macos")]
    {
        current_exe_sibling_binary("memory").is_some() || env::current_exe().ok().is_some()
    }

    #[cfg(target_os = "windows")]
    {
        env::current_exe().is_ok()
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Path::new("/lib/systemd/system/memory-layer.service").is_file()
            || Path::new("/etc/systemd/system/memory-layer.service").is_file()
    }
}

#[cfg(target_os = "windows")]
pub fn windows_backend_task_name() -> &'static str {
    "MemoryLayer-Backend"
}

#[cfg(target_os = "windows")]
pub fn windows_watch_manager_task_name() -> &'static str {
    "MemoryLayer-WatcherManager"
}

#[cfg(target_os = "windows")]
pub fn windows_watch_task_name(project: &str) -> String {
    format!("MemoryLayer-Watch-{}", sanitize_service_fragment(project))
}

#[cfg(target_os = "windows")]
pub fn windows_managed_watch_task_name(profile: &str, session_id: &str) -> String {
    format!(
        "MemoryLayer-ManagedWatch-{}-{}",
        sanitize_service_fragment(profile),
        sanitize_service_fragment(session_id)
    )
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone)]
pub struct WindowsTaskSpec {
    pub name: String,
    pub description: String,
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub working_directory: PathBuf,
    pub start_at_logon: bool,
    pub restart_on_failure: bool,
}

#[cfg(target_os = "windows")]
pub fn windows_task_exists(name: &str) -> bool {
    Command::new("schtasks.exe")
        .args(["/Query", "/TN", name])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(target_os = "windows")]
pub fn windows_task_is_running(name: &str) -> bool {
    let Ok(output) = Command::new("schtasks.exe")
        .args(["/Query", "/TN", name, "/FO", "CSV", "/NH"])
        .output()
    else {
        return false;
    };
    output.status.success()
        && parse_schtasks_csv_status(&String::from_utf8_lossy(&output.stdout))
            .is_some_and(|status| status.eq_ignore_ascii_case("running"))
}

#[cfg(target_os = "windows")]
fn parse_schtasks_csv_status(output: &str) -> Option<String> {
    let line = output.lines().find(|line| !line.trim().is_empty())?;
    let fields = line
        .trim()
        .trim_matches('"')
        .split("\",\"")
        .collect::<Vec<_>>();
    fields.get(2).or_else(|| fields.get(1)).map(|value| {
        value
            .trim()
            .trim_matches('"')
            .trim_end_matches('.')
            .to_string()
    })
}

#[cfg(target_os = "windows")]
pub fn windows_memory_task_names() -> Vec<String> {
    let Ok(output) = Command::new("schtasks.exe")
        .args(["/Query", "/FO", "CSV", "/NH"])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let mut names = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split("\",\"").next())
        .map(|value| value.trim().trim_matches('"').trim_start_matches('\\'))
        .filter(|value| value.starts_with("MemoryLayer-"))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

#[cfg(target_os = "windows")]
pub fn windows_task_running(command_fragment: &str) -> bool {
    let needle = command_fragment.to_ascii_lowercase();
    process_snapshots().iter().any(|process| {
        process.command.to_ascii_lowercase().contains(&needle) && process.pid != std::process::id()
    })
}

#[cfg(target_os = "windows")]
pub fn register_windows_task(spec: &WindowsTaskSpec) -> Result<()> {
    let xml_path = windows_task_xml_path(&spec.name)?;
    let task_dir = xml_path
        .parent()
        .context("scheduled task metadata path has no parent")?;
    fs::create_dir_all(task_dir).with_context(|| format!("create {}", task_dir.display()))?;
    let xml = render_windows_task_xml(spec)?;
    fs::write(&xml_path, encode_windows_task_xml(&xml))
        .with_context(|| format!("write {}", xml_path.display()))?;
    let output = Command::new("schtasks.exe")
        .args(["/Create", "/TN", &spec.name, "/XML"])
        .arg(&xml_path)
        .arg("/F")
        .output()
        .with_context(|| format!("register scheduled task {}", spec.name))?;
    if !output.status.success() {
        anyhow::bail!(
            "schtasks /Create failed for {}: {}{}{}",
            spec.name,
            String::from_utf8_lossy(&output.stderr).trim(),
            if output.stderr.is_empty() || output.stdout.is_empty() {
                ""
            } else {
                " | "
            },
            String::from_utf8_lossy(&output.stdout).trim()
        );
    }
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn run_windows_task(name: &str) -> Result<()> {
    run_schtasks(["/Run", "/TN", name])
}

#[cfg(target_os = "windows")]
pub fn stop_windows_task(name: &str) -> Result<()> {
    if !windows_task_is_running(name) {
        return Ok(());
    }
    run_schtasks(["/End", "/TN", name])?;
    for _ in 0..100 {
        if !windows_task_is_running(name) {
            return Ok(());
        }
        thread::sleep(std::time::Duration::from_millis(50));
    }
    anyhow::bail!("scheduled task {name} did not stop within 5 seconds")
}

#[cfg(target_os = "windows")]
pub fn delete_windows_task(name: &str) -> Result<()> {
    if windows_task_exists(name) {
        let _ = stop_windows_task(name);
        run_schtasks(["/Delete", "/TN", name, "/F"])?;
    }
    let xml_path = windows_task_xml_path(name)?;
    if xml_path.is_file() {
        fs::remove_file(&xml_path).with_context(|| format!("remove {}", xml_path.display()))?;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn windows_task_xml_path(name: &str) -> Result<PathBuf> {
    Ok(windows_data_dir()
        .context("LOCALAPPDATA and APPDATA are not set")?
        .join("tasks")
        .join(format!("{}.xml", sanitize_service_fragment(name))))
}

#[cfg(target_os = "windows")]
fn run_schtasks<const N: usize>(args: [&str; N]) -> Result<()> {
    let output = Command::new("schtasks.exe")
        .args(args)
        .output()
        .with_context(|| format!("run schtasks {}", args.join(" ")))?;
    if output.status.success() {
        return Ok(());
    }
    anyhow::bail!(
        "schtasks {} failed: {}{}{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim(),
        if output.stderr.is_empty() || output.stdout.is_empty() {
            ""
        } else {
            " | "
        },
        String::from_utf8_lossy(&output.stdout).trim()
    )
}

#[cfg(target_os = "windows")]
fn render_windows_task_xml(spec: &WindowsTaskSpec) -> Result<String> {
    let user = command_stdout_trimmed("whoami").context("resolve current Windows user")?;
    let args = spec
        .arguments
        .iter()
        .map(|value| quote_windows_argument(value))
        .collect::<Vec<_>>()
        .join(" ");
    let trigger = if spec.start_at_logon {
        format!(
            "<Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{}</UserId></LogonTrigger></Triggers>",
            xml_escape(&user)
        )
    } else {
        "<Triggers />".to_string()
    };
    let restart = if spec.restart_on_failure {
        "<RestartOnFailure><Interval>PT1M</Interval><Count>999</Count></RestartOnFailure>"
    } else {
        ""
    };
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.4" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>{description}</Description></RegistrationInfo>
  {trigger}
  <Principals><Principal id="Author"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>
  <Settings><MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy><DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries><StopIfGoingOnBatteries>false</StopIfGoingOnBatteries><AllowHardTerminate>true</AllowHardTerminate><StartWhenAvailable>true</StartWhenAvailable>{restart}<ExecutionTimeLimit>PT0S</ExecutionTimeLimit><Enabled>true</Enabled></Settings>
  <Actions Context="Author"><Exec><Command>{executable}</Command><Arguments>{arguments}</Arguments><WorkingDirectory>{working_directory}</WorkingDirectory></Exec></Actions>
</Task>
"#,
        description = xml_escape(&spec.description),
        trigger = trigger,
        user = xml_escape(&user),
        restart = restart,
        executable = xml_escape(&spec.executable.display().to_string()),
        arguments = xml_escape(&args),
        working_directory = xml_escape(&spec.working_directory.display().to_string()),
    ))
}

#[cfg(target_os = "windows")]
fn encode_windows_task_xml(xml: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(xml.len().saturating_mul(2) + 2);
    bytes.extend_from_slice(&[0xff, 0xfe]);
    for code_unit in xml.encode_utf16() {
        bytes.extend_from_slice(&code_unit.to_le_bytes());
    }
    bytes
}

#[cfg(target_os = "windows")]
fn quote_windows_argument(value: &str) -> String {
    if !value.is_empty() && !value.chars().any(|ch| ch.is_whitespace() || ch == '"') {
        return value.to_string();
    }
    let mut quoted = String::from("\"");
    let mut backslashes = 0usize;
    for ch in value.chars() {
        match ch {
            '\\' => backslashes += 1,
            '"' => {
                quoted.push_str(&"\\".repeat(backslashes.saturating_mul(2) + 1));
                quoted.push('"');
                backslashes = 0;
            }
            _ => {
                quoted.push_str(&"\\".repeat(backslashes));
                backslashes = 0;
                quoted.push(ch);
            }
        }
    }
    quoted.push_str(&"\\".repeat(backslashes.saturating_mul(2)));
    quoted.push('"');
    quoted
}

#[cfg(target_os = "windows")]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub fn backend_service_available() -> bool {
    packaged_system_service_available()
}

pub fn restart_local_watcher_service_name(service_name: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let uid_output = Command::new("id")
            .arg("-u")
            .output()
            .context("run id -u for launchctl target")?;
        if !uid_output.status.success() {
            let stderr = String::from_utf8_lossy(&uid_output.stderr);
            anyhow::bail!("id -u failed: {}", stderr.trim());
        }
        let uid = String::from_utf8_lossy(&uid_output.stdout)
            .trim()
            .to_string();
        let target = format!("gui/{uid}/{service_name}");
        let output = Command::new("launchctl")
            .args(["kickstart", "-k", &target])
            .output()
            .context("run launchctl kickstart")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("launchctl kickstart failed: {}", stderr.trim());
        }
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        stop_windows_task(service_name)?;
        run_windows_task(service_name)
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let output = Command::new("systemctl")
            .args(["--user", "restart", service_name])
            .output()
            .with_context(|| format!("run systemctl --user restart {service_name}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "systemctl --user restart {service_name} failed: {}",
                stderr.trim()
            );
        }
        Ok(())
    }
}

pub fn restart_local_watcher_service(project: &str) -> Result<()> {
    restart_local_watcher_service_name(&watch_service_unit_name(project))
}

#[cfg(target_os = "macos")]
pub fn macos_app_support_dir() -> Option<PathBuf> {
    let home = env::var("HOME").ok()?;
    Some(
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("memory-layer"),
    )
}

#[cfg(target_os = "macos")]
pub fn user_launch_agents_dir() -> Option<PathBuf> {
    let home = env::var("HOME").ok()?;
    Some(PathBuf::from(home).join("Library").join("LaunchAgents"))
}

#[cfg(target_os = "macos")]
pub fn backend_launch_agent_label() -> &'static str {
    "com.memory-layer.mem-service"
}

#[cfg(target_os = "macos")]
pub fn watch_launch_agent_label(project: &str) -> String {
    format!(
        "com.memory-layer.memory-watch.{}",
        sanitize_service_fragment(project)
    )
}

#[cfg(target_os = "macos")]
pub fn watch_manager_launch_agent_label() -> &'static str {
    "com.memory-layer.memory-watch-manager"
}

#[cfg(target_os = "macos")]
pub fn managed_watch_launch_agent_label(session_id: &str) -> String {
    format!(
        "com.memory-layer.memory-watch.codex.{}",
        sanitize_service_fragment(session_id)
    )
}

#[cfg(target_os = "macos")]
pub fn user_memory_layer_log_dir() -> Option<PathBuf> {
    Some(macos_app_support_dir()?.join("log"))
}

#[cfg(target_os = "macos")]
pub fn backend_pid_file_path() -> Option<PathBuf> {
    Some(macos_app_support_dir()?.join("run").join("mem-service.pid"))
}

#[cfg(target_os = "macos")]
pub fn backend_launch_agent_path() -> Option<PathBuf> {
    Some(user_launch_agents_dir()?.join(format!("{}.plist", backend_launch_agent_label())))
}

#[cfg(target_os = "macos")]
pub fn watch_launch_agent_path(project: &str) -> Option<PathBuf> {
    Some(user_launch_agents_dir()?.join(format!("{}.plist", watch_launch_agent_label(project))))
}

#[cfg(target_os = "macos")]
pub fn watch_manager_launch_agent_path() -> Option<PathBuf> {
    Some(user_launch_agents_dir()?.join(format!("{}.plist", watch_manager_launch_agent_label())))
}

#[cfg(target_os = "macos")]
pub fn managed_watch_launch_agent_path(session_id: &str) -> Option<PathBuf> {
    Some(user_launch_agents_dir()?.join(format!(
        "{}.plist",
        managed_watch_launch_agent_label(session_id)
    )))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::{
        derive_default_writer_id, dev_mode_status_line, format_dev_commit_label,
        managed_watch_service_name, watch_service_unit_name,
    };

    #[cfg(target_os = "windows")]
    use super::{
        WindowsTaskSpec, preferred_global_config_path, preferred_user_cache_dir,
        preferred_user_env_path, preferred_user_state_dir, render_windows_task_xml, user_home_dir,
        windows_backend_task_name, windows_watch_manager_task_name,
    };

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn restore_env_var(key: &str, value: Option<String>) {
        unsafe {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }

    #[test]
    fn derive_default_writer_id_uses_overrides_and_sanitizes_values() {
        let _guard = ENV_LOCK.lock().unwrap();
        let old_user = std::env::var("MEMORY_LAYER_WRITER_IDENTITY_USER").ok();
        let old_host = std::env::var("MEMORY_LAYER_WRITER_IDENTITY_HOST").ok();

        unsafe {
            std::env::set_var("MEMORY_LAYER_WRITER_IDENTITY_USER", "Olivier Smith");
            std::env::set_var("MEMORY_LAYER_WRITER_IDENTITY_HOST", "dev-box.local");
        }

        let writer_id = derive_default_writer_id("memory");

        restore_env_var("MEMORY_LAYER_WRITER_IDENTITY_USER", old_user);
        restore_env_var("MEMORY_LAYER_WRITER_IDENTITY_HOST", old_host);

        assert_eq!(writer_id, "memory-olivier-smith-dev-box-local");
    }

    #[test]
    fn dev_commit_label_formats_clean_dirty_and_unknown_states() {
        assert_eq!(
            format_dev_commit_label(Some("288690845510"), false),
            "288690845510"
        );
        assert_eq!(
            format_dev_commit_label(Some("288690845510"), true),
            "288690845510+dirty"
        );
        assert_eq!(format_dev_commit_label(None, true), "unknown");
        assert_eq!(format_dev_commit_label(Some("   "), false), "unknown");
        assert!(dev_mode_status_line(None).starts_with("DEV MODE  commit="));
    }

    #[test]
    fn watcher_service_names_distinguish_legacy_and_managed_units() {
        #[cfg(target_os = "macos")]
        {
            assert_eq!(
                watch_service_unit_name("customer portal"),
                "memory-watch-customer-portal.service"
            );
            assert_eq!(
                managed_watch_service_name("session 123"),
                "com.memory-layer.memory-watch.codex.session-123"
            );
        }

        #[cfg(all(unix, not(target_os = "macos")))]
        {
            assert_eq!(
                watch_service_unit_name("customer portal"),
                "memory-watch-customer-portal.service"
            );
            assert_eq!(
                managed_watch_service_name("session 123"),
                "memory-watch-codex-session-123.service"
            );
        }

        #[cfg(target_os = "windows")]
        {
            assert_eq!(
                watch_service_unit_name("customer portal"),
                "MemoryLayer-Watch-customer-portal"
            );
            assert_eq!(
                managed_watch_service_name("session 123"),
                "MemoryLayer-ManagedWatch-prod-session-123"
            );
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_paths_stay_under_local_app_data() {
        let _guard = ENV_LOCK.lock().unwrap();
        let old_local = std::env::var("LOCALAPPDATA").ok();
        let old_profile = std::env::var("USERPROFILE").ok();
        unsafe {
            std::env::set_var("LOCALAPPDATA", r"C:\Users\tester\AppData\Local");
            std::env::set_var("USERPROFILE", r"C:\Users\tester");
        }

        let root = std::path::PathBuf::from(r"C:\Users\tester\AppData\Local\memory-layer");
        assert_eq!(
            user_home_dir().unwrap(),
            std::path::PathBuf::from(r"C:\Users\tester")
        );
        assert_eq!(
            preferred_global_config_path(),
            root.join("memory-layer.toml")
        );
        assert_eq!(
            preferred_user_env_path().unwrap(),
            root.join("memory-layer.env")
        );
        assert_eq!(preferred_user_state_dir().unwrap(), root);
        assert_eq!(preferred_user_cache_dir().unwrap(), root.join("cache"));

        restore_env_var("LOCALAPPDATA", old_local);
        restore_env_var("USERPROFILE", old_profile);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_task_xml_is_per_user_and_restartable() {
        let xml = render_windows_task_xml(&WindowsTaskSpec {
            name: windows_backend_task_name().to_string(),
            executable: std::path::PathBuf::from(r"C:\Program Files\Memory Layer\bin\memory.exe"),
            arguments: vec![
                "--config".to_string(),
                r"C:\Users\Test User\AppData\Local\memory-layer\memory-layer.toml".to_string(),
                "service".to_string(),
                "run".to_string(),
            ],
            description: "Run the Memory Layer backend".to_string(),
            working_directory: std::path::PathBuf::from(r"C:\Users\tester"),
            start_at_logon: true,
            restart_on_failure: true,
        })
        .unwrap();

        assert!(xml.contains("<LogonType>InteractiveToken</LogonType>"));
        assert!(xml.contains("<RunLevel>LeastPrivilege</RunLevel>"));
        assert!(xml.contains("<LogonTrigger>"));
        assert!(xml.contains("<RestartOnFailure>"));
        assert!(xml.starts_with(r#"<?xml version="1.0" encoding="UTF-16"?>"#));
        assert_eq!(&super::encode_windows_task_xml(&xml)[..2], &[0xff, 0xfe]);
        assert!(xml.contains(
            "&quot;C:\\Users\\Test User\\AppData\\Local\\memory-layer\\memory-layer.toml&quot;"
        ));
        assert_eq!(
            windows_watch_manager_task_name(),
            "MemoryLayer-WatcherManager"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn scheduled_task_csv_status_parser_handles_verbose_and_compact_rows() {
        assert_eq!(
            super::parse_schtasks_csv_status(r#""BUNNY","\MemoryLayer-Backend","Running","N/A""#)
                .as_deref(),
            Some("Running")
        );
        assert_eq!(
            super::parse_schtasks_csv_status(r#""\MemoryLayer-Backend","Ready""#).as_deref(),
            Some("Ready")
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn packaged_assets_are_found_from_windows_bin_share_layout() {
        let root = std::env::temp_dir().join(format!("memory-layer-assets-{}", std::process::id()));
        let exe = root.join("bin").join("memory.exe");
        let web = root.join("share").join("memory-layer").join("web");
        std::fs::create_dir_all(&web).unwrap();
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"test").unwrap();

        assert_eq!(super::packaged_share_subdir_for_exe(&exe, "web"), Some(web));

        let _ = std::fs::remove_dir_all(root);
    }
}
