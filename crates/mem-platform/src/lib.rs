use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result};

pub fn preferred_global_config_path() -> PathBuf {
    #[cfg(target_os = "macos")]
    if let Some(path) = macos_app_support_dir() {
        return path.join("memory-layer.toml");
    }

    if let Ok(config_home) = env::var("XDG_CONFIG_HOME") {
        PathBuf::from(config_home)
            .join("memory-layer")
            .join("memory-layer.toml")
    } else if let Ok(home) = env::var("HOME") {
        PathBuf::from(home)
            .join(".config")
            .join("memory-layer")
            .join("memory-layer.toml")
    } else {
        PathBuf::from("/etc/memory-layer/memory-layer.toml")
    }
}

pub fn discover_existing_global_config_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    if let Some(candidate) = macos_app_support_dir().map(|dir| dir.join("memory-layer.toml")) {
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    if let Ok(config_home) = env::var("XDG_CONFIG_HOME") {
        let candidate = PathBuf::from(config_home)
            .join("memory-layer")
            .join("memory-layer.toml");
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

pub fn preferred_user_env_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        return Some(macos_app_support_dir()?.join("memory-layer.env"));
    }

    #[cfg(not(target_os = "macos"))]
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
        if let Ok(local_app_data) = env::var("LOCALAPPDATA") {
            return Some(PathBuf::from(local_app_data).join("memory-layer"));
        }
        if let Ok(app_data) = env::var("APPDATA") {
            return Some(PathBuf::from(app_data).join("memory-layer"));
        }
        return None;
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

pub fn default_shared_capnp_unix_socket() -> String {
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
    env::var("MEMORY_LAYER_WRITER_IDENTITY_HOST")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| env::var("HOSTNAME").ok())
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
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

fn command_stdout_trimmed(program: &str) -> Option<String> {
    let output = Command::new(program).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

pub fn watch_service_unit_name(project: &str) -> String {
    format!(
        "memory-watch-{}.service",
        sanitize_service_fragment(project)
    )
}

pub fn current_exe_sibling_binary(name: &str) -> Option<PathBuf> {
    let current_exe = env::current_exe().ok()?;
    let bin_dir = current_exe.parent()?;
    let sibling = bin_dir.join(name);
    sibling.is_file().then_some(sibling)
}

pub fn packaged_system_service_available() -> bool {
    #[cfg(target_os = "macos")]
    {
        current_exe_sibling_binary("memory").is_some() || env::current_exe().ok().is_some()
    }

    #[cfg(not(target_os = "macos"))]
    {
        Path::new("/lib/systemd/system/memory-layer.service").is_file()
            || Path::new("/etc/systemd/system/memory-layer.service").is_file()
    }
}

pub fn backend_service_available() -> bool {
    packaged_system_service_available()
}

pub fn restart_local_watcher_service(project: &str) -> Result<()> {
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
        let target = format!("gui/{uid}/{}", watch_launch_agent_label(project));
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
        let _ = project;
        anyhow::bail!("watcher watchdog restart is not implemented on Windows yet")
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let unit_name = watch_service_unit_name(project);
        let output = Command::new("systemctl")
            .args(["--user", "restart", &unit_name])
            .output()
            .with_context(|| format!("run systemctl --user restart {unit_name}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "systemctl --user restart {unit_name} failed: {}",
                stderr.trim()
            );
        }
        Ok(())
    }
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

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::derive_default_writer_id;

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
}
