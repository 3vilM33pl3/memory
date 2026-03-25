use std::{
    env,
    path::{Path, PathBuf},
};

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
        current_exe_sibling_binary("mem-service").is_some()
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
