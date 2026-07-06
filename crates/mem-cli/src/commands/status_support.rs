#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::{
    fs,
    io::{self},
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    time::Duration,
};

use anyhow::{Context, Result};
use mem_api::{
    AppConfig, Profile, discover_global_config_path, discover_repo_env_path,
    effective_llm_base_url, is_ollama_provider, llm_requires_api_key, resolve_llm_api_key,
};
use mem_watch::load_state;
use reqwest::Client;
use serde::Serialize;
use sqlx::{Row, postgres::PgPoolOptions};

use crate::commands::{
    api::{ApiClient, format_api_error},
    init_support::{ensure_mem_gitignore, initialize_repo},
    memory_ops::detect_changed_files,
    output::{service_url, write_headers},
    runtime::{
        DEV_API_TOKEN, backend_start_hint, default_global_config_path,
        ensure_shared_service_api_token, shared_env_path_for_config,
    },
    service_support::backend_service_status,
    skill_support::{
        SkillBundleStatus, SkillUpgradeAction, discover_skill_template_dir,
        download_github_skill_template, ensure_claude_md_memory_section,
        format_github_skill_version_summary, format_skill_inventory_summary,
        github_skill_version_report, missing_memory_skill_dirs, project_skill_inventory,
        render_agent_project_config, render_project_metadata, render_repo_config,
        sync_memory_skill_bundle, upgrade_project_skills,
    },
    watch_support::{watch_manager_service_status, watch_service_status, yes_no},
};
use crate::writer_identity::resolve_writer_identity;

#[cfg(target_os = "macos")]
use crate::commands::watch_support::{
    launch_agent_status, watch_manager_launch_agent_label, watch_manager_launch_agent_path,
};

#[cfg(not(target_os = "macos"))]
use crate::commands::watch_support::{
    WATCH_MANAGER_UNIT_NAME, run_systemctl_user, user_systemd_unit_dir,
};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DoctorReport {
    pub(crate) project: String,
    pub(crate) repo_root: String,
    pub(crate) config_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) global_config_path: Option<String>,
    pub(crate) fix_mode: bool,
    pub(crate) checks: Vec<DoctorCheckResult>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CliStatusReport {
    pub(crate) project: String,
    pub(crate) repo_root: String,
    pub(crate) config_path: String,
    pub(crate) summary: CliStatusSummary,
    pub(crate) service: StatusTextProbe,
    pub(crate) health: StatusJsonProbe,
    pub(crate) stats: StatusJsonProbe,
    pub(crate) runtime: StatusJsonProbe,
    pub(crate) watcher_manager: StatusTextProbe,
    pub(crate) project_watcher: StatusTextProbe,
    pub(crate) mcp: mem_mcp::MpcStatusReport,
    pub(crate) doctor: DoctorReport,
    pub(crate) output_contract: CliOutputContract,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CliStatusSummary {
    pub(crate) overall: DoctorStatus,
    pub(crate) doctor_failures: usize,
    pub(crate) doctor_warnings: usize,
    pub(crate) service_reachable: bool,
    pub(crate) stats_reachable: bool,
    pub(crate) mcp_service_reachable: bool,
    pub(crate) mcp_project_ok: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct StatusTextProbe {
    pub(crate) ok: bool,
    pub(crate) summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) details: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct StatusJsonProbe {
    pub(crate) ok: bool,
    pub(crate) status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) payload: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CliOutputContract {
    pub(crate) stdout: &'static str,
    pub(crate) stderr: &'static str,
    pub(crate) json_errors: &'static str,
    pub(crate) exit_codes: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DoctorCheckResult {
    pub(crate) id: String,
    pub(crate) status: DoctorStatus,
    pub(crate) summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) details: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) suggested_fix: Option<String>,
    #[serde(default)]
    pub(crate) fix_applied: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DoctorStatus {
    Ok,
    Warn,
    Fail,
    Skipped,
}

impl DoctorReport {
    fn push(&mut self, result: DoctorCheckResult) {
        self.checks.push(result);
    }
}

pub(crate) async fn build_cli_status_report(
    config_path: Option<PathBuf>,
    client: &Client,
    config: AppConfig,
    repo_root: &Path,
    project: String,
) -> Result<CliStatusReport> {
    let config_path = config_path.unwrap_or_else(default_global_config_path);
    let service = text_probe("service", backend_service_status(&config_path));
    let health = fetch_status_json_probe(client, &config, "/healthz", false).await;
    let stats = fetch_status_json_probe(client, &config, "/v1/stats", true).await;
    let runtime = fetch_status_json_probe(
        client,
        &config,
        &format!("/v1/runtime/status?project={project}"),
        true,
    )
    .await;
    let watcher_manager = text_probe(
        "watcher manager",
        watch_manager_service_status(Profile::detect()),
    );
    let project_watcher = text_probe("project watcher", watch_service_status(repo_root, &project));
    let mcp = mem_mcp::status_report(config.clone(), Some(project.clone())).await;
    let doctor = run_doctor(Some(config_path.clone()), repo_root, &project, false).await?;
    let doctor_failures = doctor
        .checks
        .iter()
        .filter(|check| check.status == DoctorStatus::Fail)
        .count();
    let doctor_warnings = doctor
        .checks
        .iter()
        .filter(|check| check.status == DoctorStatus::Warn)
        .count();
    let overall = if doctor_failures > 0 || !health.ok || !mcp.service_reachable {
        DoctorStatus::Fail
    } else if doctor_warnings > 0 || !stats.ok || !mcp.project_overview_ok {
        DoctorStatus::Warn
    } else {
        DoctorStatus::Ok
    };

    Ok(CliStatusReport {
        project,
        repo_root: repo_root.display().to_string(),
        config_path: config_path.display().to_string(),
        summary: CliStatusSummary {
            overall,
            doctor_failures,
            doctor_warnings,
            service_reachable: health.ok,
            stats_reachable: stats.ok,
            mcp_service_reachable: mcp.service_reachable,
            mcp_project_ok: mcp.project_overview_ok,
        },
        service,
        health,
        stats,
        runtime,
        watcher_manager,
        project_watcher,
        mcp,
        doctor,
        output_contract: CliOutputContract {
            stdout: "machine-readable JSON or final human-readable command output only",
            stderr: "warnings, progress, and diagnostics that are not part of parsed output",
            json_errors: "HTTP failures use the shared diagnostic error shape when the service provides one",
            exit_codes: "non-zero on command failure; stable category-specific exit codes are not implemented yet",
        },
    })
}

pub(crate) async fn fetch_status_json_probe(
    client: &Client,
    config: &AppConfig,
    path: &str,
    auth: bool,
) -> StatusJsonProbe {
    let mut request = client.get(service_url(config, path));
    if auth {
        match write_headers(config) {
            Ok(headers) => {
                request = request.headers(headers);
            }
            Err(error) => {
                return StatusJsonProbe {
                    ok: false,
                    status: None,
                    payload: None,
                    error: Some(error.to_string()),
                };
            }
        }
    }
    match request.send().await {
        Ok(response) => {
            let status = response.status();
            match response.text().await {
                Ok(body) if status.is_success() => match serde_json::from_str(&body) {
                    Ok(payload) => StatusJsonProbe {
                        ok: true,
                        status: Some(status.as_u16()),
                        payload: Some(payload),
                        error: None,
                    },
                    Err(error) => StatusJsonProbe {
                        ok: false,
                        status: Some(status.as_u16()),
                        payload: None,
                        error: Some(format!("response was not valid JSON: {error}")),
                    },
                },
                Ok(body) => StatusJsonProbe {
                    ok: false,
                    status: Some(status.as_u16()),
                    payload: None,
                    error: Some(format_api_error(status, &body)),
                },
                Err(error) => StatusJsonProbe {
                    ok: false,
                    status: Some(status.as_u16()),
                    payload: None,
                    error: Some(error.to_string()),
                },
            }
        }
        Err(error) => StatusJsonProbe {
            ok: false,
            status: None,
            payload: None,
            error: Some(error.to_string()),
        },
    }
}

pub(crate) fn text_probe(label: &str, result: Result<String>) -> StatusTextProbe {
    match result {
        Ok(output) => StatusTextProbe {
            ok: true,
            summary: first_non_empty_line(&output).unwrap_or_else(|| format!("{label} ok")),
            details: Some(output),
            error: None,
        },
        Err(error) => StatusTextProbe {
            ok: false,
            summary: format!("{label} unavailable"),
            details: None,
            error: Some(error.to_string()),
        },
    }
}

pub(crate) fn first_non_empty_line(value: &str) -> Option<String> {
    value
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
}

pub(crate) fn print_cli_status_report(report: &CliStatusReport) {
    println!(
        "Memory Layer status: {}",
        doctor_status_label(report.summary.overall)
    );
    println!("Project: {}", report.project);
    println!("Repo root: {}", report.repo_root);
    println!("Config: {}", report.config_path);
    println!("Service install: {}", report.service.summary);
    println!(
        "HTTP health: {}",
        status_probe_summary(
            report.health.ok,
            report.health.status,
            report.health.error.as_deref()
        )
    );
    println!(
        "Stats API: {}",
        status_probe_summary(
            report.stats.ok,
            report.stats.status,
            report.stats.error.as_deref()
        )
    );
    if let Some(provenance) = report
        .runtime
        .payload
        .as_ref()
        .and_then(|payload| payload.get("provenance"))
    {
        let status = provenance
            .get("status")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        let last_finished = provenance
            .get("last_finished_at")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("not run");
        println!("Provenance reverify: {status} (last finished: {last_finished})");
    }
    println!("Watcher manager: {}", report.watcher_manager.summary);
    println!("Project watcher: {}", report.project_watcher.summary);
    println!(
        "MCP: service={} project={} http={} tools={}",
        yes_no(report.mcp.service_reachable),
        yes_no(report.mcp.project_overview_ok),
        if report.mcp.http_enabled {
            report.mcp.http_path.as_str()
        } else {
            "disabled"
        },
        report.mcp.tools.len(),
    );
    println!(
        "Doctor: {} failure(s), {} warning(s), {} check(s)",
        report.summary.doctor_failures,
        report.summary.doctor_warnings,
        report.doctor.checks.len(),
    );
    println!(
        "Output contract: parsed output stays on stdout; warnings/progress go to stderr; category-specific exit codes are deferred."
    );
    if report.summary.overall != DoctorStatus::Ok {
        println!("Next: memory doctor --project {}", report.project);
    }
}

pub(crate) fn status_probe_summary(ok: bool, status: Option<u16>, error: Option<&str>) -> String {
    if ok {
        return status
            .map(|status| format!("ok HTTP {status}"))
            .unwrap_or_else(|| "ok".to_string());
    }
    let mut summary = status
        .map(|status| format!("failed HTTP {status}"))
        .unwrap_or_else(|| "failed".to_string());
    if let Some(error) = error {
        summary.push_str(": ");
        summary.push_str(error);
    }
    summary
}

pub(crate) fn doctor_status_label(status: DoctorStatus) -> &'static str {
    match status {
        DoctorStatus::Ok => "ok",
        DoctorStatus::Warn => "warn",
        DoctorStatus::Fail => "fail",
        DoctorStatus::Skipped => "skipped",
    }
}

pub(crate) fn doctor_check(
    id: &str,
    status: DoctorStatus,
    summary: impl Into<String>,
    details: Option<String>,
    suggested_fix: Option<String>,
    fix_applied: bool,
) -> DoctorCheckResult {
    DoctorCheckResult {
        id: id.to_string(),
        status,
        summary: summary.into(),
        details,
        suggested_fix,
        fix_applied,
    }
}

/// Check whether the `memory` binary is resolvable on `PATH`. This is the one
/// "command not found" failure `doctor` could not previously diagnose (it runs
/// as the binary, so PATH resolution is otherwise invisible). Reported as a
/// warning, not a failure, because doctor is clearly already executing.
fn cli_path_check() -> DoctorCheckResult {
    let current_exe = std::env::current_exe().ok();
    let exe_dir = current_exe
        .as_ref()
        .and_then(|path| path.parent())
        .map(Path::to_path_buf);
    let binary_name = "memory";

    let resolved = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(binary_name))
            .find(|candidate| candidate.is_file())
    });

    match resolved {
        Some(path) => doctor_check(
            "cli.on_path",
            DoctorStatus::Ok,
            "The `memory` command is on PATH.",
            Some(path.display().to_string()),
            None,
            false,
        ),
        None => doctor_check(
            "cli.on_path",
            DoctorStatus::Warn,
            "The `memory` command is not on PATH.",
            current_exe.as_ref().map(|path| path.display().to_string()),
            Some(match exe_dir {
                Some(dir) => format!(
                    "Add this directory to your PATH so `memory` resolves everywhere: {}",
                    dir.display()
                ),
                None => {
                    "Add the directory containing the `memory` binary to your PATH.".to_string()
                }
            }),
            false,
        ),
    }
}

pub(crate) fn repo_uses_go_skill_runtime(repo_root: &Path) -> bool {
    repo_root
        .join(".agents/skills/memory-layer/scripts/go.mod")
        .is_file()
}

pub(crate) fn go_runtime_available() -> bool {
    ProcessCommand::new("go")
        .arg("version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub(crate) async fn run_doctor(
    cli_config: Option<PathBuf>,
    repo_root: &Path,
    project: &str,
    fix: bool,
) -> Result<DoctorReport> {
    let project_paths = mem_platform::project_paths(repo_root, project)
        .ok_or_else(|| anyhow::anyhow!("could not resolve user project config paths"))?;
    let config_path = cli_config
        .clone()
        .unwrap_or_else(|| project_paths.config_path());
    let global_config_path = discover_global_config_path();
    let mut report = DoctorReport {
        project: project.to_string(),
        repo_root: repo_root.display().to_string(),
        config_path: config_path.display().to_string(),
        global_config_path: global_config_path
            .as_ref()
            .map(|path| path.display().to_string()),
        fix_mode: fix,
        checks: Vec::new(),
    };

    report.push(cli_path_check());

    let mem_dir = repo_root.join(".mem");
    let project_path = mem_dir.join("project.toml");
    let legacy_config_path = mem_dir.join("config.toml");
    let root_gitignore_path = repo_root.join(".gitignore");
    let local_service_overrides = read_local_service_overrides(repo_root);

    let mut init_fix_applied = false;
    if !mem_dir.exists() && fix {
        initialize_repo(repo_root, project, false, false)?;
        init_fix_applied = true;
    }

    report.push(doctor_check(
        "repo.bootstrap_dir",
        if mem_dir.exists() || init_fix_applied {
            DoctorStatus::Ok
        } else {
            DoctorStatus::Fail
        },
        if mem_dir.exists() || init_fix_applied {
            "Repo-local .mem directory is present."
        } else {
            "Repo-local .mem directory is missing."
        },
        Some(mem_dir.display().to_string()),
        if mem_dir.exists() || init_fix_applied {
            None
        } else {
            Some("memory init".to_string())
        },
        init_fix_applied,
    ));

    let migration_fix_applied = if !config_path.exists() && legacy_config_path.exists() && fix {
        repair_repo_bootstrap(repo_root, project)?;
        true
    } else {
        false
    };
    report.push(doctor_check(
        "project.legacy_config_migration",
        if legacy_config_path.exists() && config_path.exists() {
            DoctorStatus::Ok
        } else if legacy_config_path.exists() {
            DoctorStatus::Warn
        } else {
            DoctorStatus::Ok
        },
        if legacy_config_path.exists() && config_path.exists() {
            "Legacy .mem config has a user-local project config counterpart."
        } else if legacy_config_path.exists() {
            "Legacy .mem config is present but has not been migrated to the user-local project config directory."
        } else {
            "No legacy .mem config migration is needed."
        },
        Some(format!(
            "legacy={}, current={}",
            legacy_config_path.display(),
            config_path.display()
        )),
        if legacy_config_path.exists() && !config_path.exists() {
            Some("memory doctor --fix".to_string())
        } else {
            None
        },
        migration_fix_applied,
    ));

    let config_fix_applied = if !config_path.exists() && fix {
        repair_repo_bootstrap(repo_root, project)?;
        true
    } else {
        false
    };
    report.push(doctor_check(
        "project.config_file",
        if config_path.exists() || config_fix_applied {
            DoctorStatus::Ok
        } else if legacy_config_path.exists() {
            DoctorStatus::Warn
        } else {
            DoctorStatus::Fail
        },
        if config_path.exists() || config_fix_applied {
            "User-local project config file is present."
        } else if legacy_config_path.exists() {
            "User-local project config file is missing; legacy .mem/config.toml fallback is active."
        } else {
            "User-local project config file is missing."
        },
        Some(config_path.display().to_string()),
        if config_path.exists() || config_fix_applied {
            None
        } else if legacy_config_path.exists() {
            Some("memory doctor --fix".to_string())
        } else {
            Some("memory init".to_string())
        },
        config_fix_applied,
    ));

    let project_fix_applied = if !project_path.exists() && fix {
        repair_repo_bootstrap(repo_root, project)?;
        true
    } else {
        false
    };
    report.push(doctor_check(
        "repo.project_file",
        if project_path.exists() || project_fix_applied {
            DoctorStatus::Ok
        } else {
            DoctorStatus::Fail
        },
        if project_path.exists() || project_fix_applied {
            "Project metadata file is present."
        } else {
            "Project metadata file is missing."
        },
        Some(project_path.display().to_string()),
        if project_path.exists() || project_fix_applied {
            None
        } else {
            Some("memory init".to_string())
        },
        project_fix_applied,
    ));

    report.push(doctor_check(
        "global.config_file",
        if global_config_path.is_some() {
            DoctorStatus::Ok
        } else {
            DoctorStatus::Warn
        },
        if global_config_path.is_some() {
            "Global shared config is present."
        } else {
            "Global shared config is missing."
        },
        Some(
            global_config_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(default_global_config_path_label),
        ),
        if global_config_path.is_some() {
            None
        } else {
            Some("Create the global config and set shared defaults there.".to_string())
        },
        false,
    ));

    report.push(doctor_check(
        "repo.gitignore",
        if root_gitignore_contains_mem(repo_root)? {
            DoctorStatus::Warn
        } else {
            DoctorStatus::Ok
        },
        if root_gitignore_contains_mem(repo_root)? {
            "Root .gitignore ignores .mem, which may hide the repo-local project marker."
        } else {
            "Root .gitignore does not hide the repo-local .mem marker directory."
        },
        Some(root_gitignore_path.display().to_string()),
        if root_gitignore_contains_mem(repo_root)? {
            Some(
                "Remove `/.mem` from the root .gitignore if you want to commit .mem/project.toml."
                    .to_string(),
            )
        } else {
            None
        },
        false,
    ));

    let skill_upgrade_fix = if fix {
        Some(upgrade_project_skills(repo_root, false, false)?)
    } else {
        None
    };
    let skill_inventory = skill_upgrade_fix
        .as_ref()
        .map(|report| report.inventory.clone())
        .unwrap_or_else(|| project_skill_inventory(repo_root, false));
    let skill_fix_applied = skill_upgrade_fix.as_ref().is_some_and(|upgrade| {
        upgrade
            .inventory
            .skills
            .iter()
            .any(|skill| !matches!(skill.action, SkillUpgradeAction::Skip))
    });
    report.push(doctor_check(
        "workflow.project_skills",
        match skill_inventory.status {
            SkillBundleStatus::Ok => DoctorStatus::Ok,
            SkillBundleStatus::Warn => DoctorStatus::Warn,
            SkillBundleStatus::Error => DoctorStatus::Fail,
        },
        match skill_inventory.status {
            SkillBundleStatus::Ok => {
                "Repo-local Memory skill bundle matches the installed template version."
            }
            SkillBundleStatus::Warn => "Repo-local Memory skill bundle needs attention.",
            SkillBundleStatus::Error => "Repo-local Memory skill bundle could not be evaluated.",
        },
        Some(format_skill_inventory_summary(&skill_inventory)),
        if skill_inventory.status == SkillBundleStatus::Ok {
            None
        } else {
            Some(
                "Run `memory doctor --fix` to download current skills from GitHub and repair repo-local copies, or preview with `memory upgrade --dry-run`."
                    .to_string(),
            )
        },
        skill_fix_applied,
    ));

    match github_skill_version_report(repo_root) {
        Ok(github_inventory) => {
            report.push(doctor_check(
                "workflow.project_skills_github",
                match github_inventory.status {
                    SkillBundleStatus::Ok => DoctorStatus::Ok,
                    SkillBundleStatus::Warn => DoctorStatus::Warn,
                    SkillBundleStatus::Error => DoctorStatus::Fail,
                },
                match github_inventory.status {
                    SkillBundleStatus::Ok => {
                        "Repo-local Memory skills match the GitHub skill bundle."
                    }
                    SkillBundleStatus::Warn => {
                        "Repo-local Memory skills differ from the GitHub skill bundle."
                    }
                    SkillBundleStatus::Error => {
                        "GitHub Memory skill bundle could not be evaluated."
                    }
                },
                Some(format_github_skill_version_summary(&github_inventory)),
                if github_inventory.status == SkillBundleStatus::Ok {
                    None
                } else {
                    Some(
                        "Run `memory doctor --fix` to download current skills from GitHub and repair repo-local copies."
                            .to_string(),
                    )
                },
                false,
            ));
        }
        Err(error) => report.push(doctor_check(
            "workflow.project_skills_github",
            DoctorStatus::Skipped,
            "Skipped GitHub skill freshness check.",
            Some(error.to_string()),
            Some(
                "Connect to GitHub and rerun `memory doctor`, or run `memory doctor --fix` when online."
                    .to_string(),
            ),
            false,
        )),
    }

    let config = match AppConfig::load_from_path(cli_config.clone()) {
        Ok(config) => {
            report.push(doctor_check(
                "config.load",
                DoctorStatus::Ok,
                "Merged config loads successfully.",
                None,
                None,
                false,
            ));
            Some(config)
        }
        Err(error) => {
            report.push(doctor_check(
                "config.load",
                DoctorStatus::Fail,
                "Merged config failed to load.",
                Some(error.to_string()),
                Some(format!(
                    "Check {} and {}",
                    global_config_path
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(default_global_config_path_label),
                    config_path.display()
                )),
                false,
            ));
            None
        }
    };

    if let Some(config) = config {
        report.push(doctor_check(
            "config.database_url",
            if is_placeholder_database_url(&config.database.url) {
                DoctorStatus::Warn
            } else {
                DoctorStatus::Ok
            },
            if is_placeholder_database_url(&config.database.url) {
                "Database URL still uses the placeholder value."
            } else {
                "Database URL is configured."
            },
            Some(mask_database_url(&config.database.url)),
            if is_placeholder_database_url(&config.database.url) {
                Some(format!(
                    "Set [database].url in {}",
                    global_config_path
                        .as_ref()
                        .unwrap_or(&config_path)
                        .display()
                ))
            } else {
                None
            },
            false,
        ));

        let mut database_connect_error = None;
        if is_placeholder_database_url(&config.database.url) {
            report.push(doctor_check(
                "database.pgvector_extension",
                DoctorStatus::Skipped,
                "Skipped pgvector checks because the database URL is still a placeholder.",
                None,
                None,
                false,
            ));
        } else {
            match PgPoolOptions::new()
                .max_connections(1)
                .acquire_timeout(Duration::from_secs(3))
                .connect(&config.database.url)
                .await
            {
                Ok(pool) => {
                    report.push(doctor_check(
                        "database.connect",
                        DoctorStatus::Ok,
                        "Database connection succeeded.",
                        Some(mask_database_url(&config.database.url)),
                        None,
                        false,
                    ));

                    match sqlx::query(
                        "SELECT extversion FROM pg_extension WHERE extname = 'vector' LIMIT 1",
                    )
                    .fetch_optional(&pool)
                    .await
                    {
                        Ok(Some(row)) => report.push(doctor_check(
                            "database.pgvector_extension",
                            DoctorStatus::Ok,
                            "pgvector extension is enabled in the target database.",
                            Some(format!(
                                "vector extension version {}",
                                row.try_get::<String, _>("extversion")
                                    .unwrap_or_else(|_| "unknown".to_string())
                            )),
                            None,
                            false,
                        )),
                        Ok(None) if fix => {
                            match sqlx::query("CREATE EXTENSION IF NOT EXISTS vector")
                                .execute(&pool)
                                .await
                            {
                                Ok(_) => report.push(doctor_check(
                                    "database.pgvector_extension",
                                    DoctorStatus::Ok,
                                    "Enabled the pgvector extension in the target database.",
                                    None,
                                    None,
                                    true,
                                )),
                                Err(error) => report.push(doctor_check(
                                    "database.pgvector_extension",
                                    DoctorStatus::Fail,
                                    "pgvector extension is missing and could not be created automatically.",
                                    Some(error.to_string()),
                                    Some(
                                        "Install the pgvector package for your PostgreSQL version (e.g. postgresql-16-pgvector), then run CREATE EXTENSION vector; in the target database."
                                            .to_string(),
                                    ),
                                    false,
                                )),
                            }
                        }
                        Ok(None) => report.push(doctor_check(
                            "database.pgvector_extension",
                            DoctorStatus::Fail,
                            "pgvector extension is not enabled in the target database.",
                            None,
                            Some(
                                "Run `memory doctor --fix` to attempt CREATE EXTENSION vector, or install pgvector for your PostgreSQL version and run CREATE EXTENSION vector; in the target database."
                                    .to_string(),
                            ),
                            false,
                        )),
                        Err(error) => report.push(doctor_check(
                            "database.pgvector_extension",
                            DoctorStatus::Fail,
                            "Could not verify pgvector extension state.",
                            Some(error.to_string()),
                            Some(
                                "Install pgvector for your PostgreSQL version and run CREATE EXTENSION vector; in the target database."
                                    .to_string(),
                            ),
                            false,
                        )),
                    }
                }
                Err(error) => {
                    database_connect_error = Some(error.to_string());
                    report.push(doctor_check(
                        "database.connect",
                        DoctorStatus::Fail,
                        "Could not connect to the configured database directly.",
                        Some(error.to_string()),
                        Some(if config.cluster.enabled {
                            "Fix the database URL or credentials first, or start another database-connected Memory Layer backend on the local network for relay discovery.".to_string()
                        } else {
                            format!(
                                "Fix the database URL or credentials first, or enable relay discovery by setting [cluster].enabled = true in {}.",
                                global_config_path
                                    .as_ref()
                                    .unwrap_or(&config_path)
                                    .display()
                            )
                        }),
                        false,
                    ));
                    report.push(doctor_check(
                        "database.pgvector_extension",
                        DoctorStatus::Skipped,
                        "Skipped pgvector extension check because the database connection failed.",
                        None,
                        None,
                        false,
                    ));
                }
            }
        }

        report.push(doctor_check(
            "config.api_token",
            if config.service.api_token.trim().is_empty() {
                DoctorStatus::Fail
            } else if config.service.api_token == DEV_API_TOKEN {
                DoctorStatus::Warn
            } else {
                DoctorStatus::Ok
            },
            if config.service.api_token.trim().is_empty() {
                "API token is empty."
            } else if config.service.api_token == DEV_API_TOKEN {
                "API token is set to the development default."
            } else {
                "API token is configured."
            },
            None,
            if config.service.api_token.trim().is_empty()
                || config.service.api_token == DEV_API_TOKEN
            {
                Some(
                    "Run `memory wizard --global` or `memory service ensure-api-token --rotate-placeholder` to provision a machine-local token."
                        .to_string(),
                )
            } else {
                None
            },
            false,
        ));

        report.push(doctor_check(
            "config.writer_id",
            DoctorStatus::Ok,
            if config.writer.id.trim().is_empty() {
                "Writer id will be auto-derived for write-capable workflows."
            } else {
                "Writer id is configured."
            },
            Some(resolve_writer_identity(&config, None)?.id),
            if config.writer.id.trim().is_empty() {
                Some(format!(
                    "Optional: set [writer].id in {} or export MEMORY_LAYER_WRITER_ID if you want a custom stable writer label.",
                    config_path.display()
                ))
            } else {
                None
            },
            false,
        ));

        report.push(doctor_check(
            "config.llm_model",
            if config.llm.model.trim().is_empty() {
                DoctorStatus::Fail
            } else {
                DoctorStatus::Ok
            },
            if config.llm.model.trim().is_empty() {
                "LLM model is not configured."
            } else {
                "LLM model is configured."
            },
            Some(format!(
                "provider={} base_url={}",
                config.llm.provider,
                effective_llm_base_url(&config.llm)
            )),
            if config.llm.model.trim().is_empty() {
                Some(format!(
                    "Set [llm].model in {}",
                    global_config_path
                        .as_ref()
                        .unwrap_or(&config_path)
                        .display()
                ))
            } else {
                None
            },
            false,
        ));

        let repo_env_path = discover_repo_env_path();
        let llm_api_key_value = resolve_llm_api_key(&config.llm).unwrap_or_default();
        let llm_api_key_required = llm_requires_api_key(&config.llm);
        report.push(doctor_check(
            "config.llm_api_key",
            if !llm_api_key_required {
                DoctorStatus::Skipped
            } else if llm_api_key_value.trim().is_empty() {
                DoctorStatus::Fail
            } else {
                DoctorStatus::Ok
            },
            if !llm_api_key_required {
                "LLM API key is optional for this provider."
            } else if llm_api_key_value.trim().is_empty() {
                "LLM API key environment variable is missing."
            } else {
                "LLM API key environment variable is present."
            },
            Some(config.llm.api_key_env.clone()),
            if llm_api_key_value.trim().is_empty() {
                Some({
                    let mut locations = Vec::new();
                    if let Some(path) = repo_env_path.as_ref() {
                        locations.push(path.display().to_string());
                    }
                    locations.push(
                        global_config_path
                            .as_ref()
                            .map(|path| shared_env_path_for_config(path).display().to_string())
                            .unwrap_or_else(|| {
                                shared_env_path_for_config(&config_path)
                                    .display()
                                    .to_string()
                            }),
                    );
                    format!(
                        "Set {} in {} or export it in your shell",
                        config.llm.api_key_env,
                        locations.join(" or ")
                    )
                })
            } else {
                None
            },
            false,
        ));

        if is_ollama_provider(&config.llm.provider) {
            let models_url = format!("{}/models", effective_llm_base_url(&config.llm));
            let ollama_check = match Client::new().get(&models_url).send().await {
                Ok(response) if response.status().is_success() => {
                    match response.json::<serde_json::Value>().await {
                        Ok(body) => {
                            let model = config.llm.model.trim();
                            let found = body
                                .get("data")
                                .and_then(|value| value.as_array())
                                .is_some_and(|models| {
                                    models.iter().any(|entry| {
                                        entry
                                            .get("id")
                                            .and_then(|value| value.as_str())
                                            .is_some_and(|id| id == model)
                                    })
                                });
                            doctor_check(
                                "config.ollama",
                                if found {
                                    DoctorStatus::Ok
                                } else {
                                    DoctorStatus::Warn
                                },
                                if found {
                                    "Ollama is reachable and the configured model is available."
                                } else {
                                    "Ollama is reachable but the configured model was not listed."
                                },
                                Some(models_url),
                                (!found).then(|| format!("Run `ollama pull {model}`")),
                                false,
                            )
                        }
                        Err(error) => doctor_check(
                            "config.ollama",
                            DoctorStatus::Warn,
                            "Ollama responded but the model list could not be parsed.",
                            Some(models_url),
                            Some(error.to_string()),
                            false,
                        ),
                    }
                }
                Ok(response) => doctor_check(
                    "config.ollama",
                    DoctorStatus::Fail,
                    "Ollama model endpoint returned an error.",
                    Some(models_url),
                    Some(format!("HTTP {}", response.status())),
                    false,
                ),
                Err(error) => doctor_check(
                    "config.ollama",
                    DoctorStatus::Fail,
                    "Ollama is not reachable at the configured base URL.",
                    Some(models_url),
                    Some(format!("Start Ollama with `ollama serve`: {error}")),
                    false,
                ),
            };
            report.push(ollama_check);
        }

        report.push(doctor_check(
            "config.service_endpoints",
            DoctorStatus::Ok,
            if local_service_overrides.is_some() {
                "Repo-local service endpoints are configured."
            } else {
                "Using shared/global service endpoints."
            },
            Some(format!(
                "http={} capnp_tcp={} capnp_unix={}",
                config.service.bind_addr,
                config.service.capnp_tcp_addr,
                config.service.capnp_unix_socket
            )),
            None,
            false,
        ));
        report.push(doctor_check(
            "config.relay_discovery",
            if config.cluster.enabled {
                DoctorStatus::Ok
            } else if database_connect_error.is_some() {
                DoctorStatus::Warn
            } else {
                DoctorStatus::Ok
            },
            if config.cluster.enabled {
                "Relay discovery is enabled for backend failover."
            } else {
                "Relay discovery is disabled."
            },
            Some(format!(
                "enabled={} multicast={} priority={}",
                config.cluster.enabled,
                config.cluster.discovery_multicast_addr,
                config.cluster.priority
            )),
            if config.cluster.enabled {
                None
            } else {
                Some(format!(
                    "Set [cluster].enabled = true in {} to allow this backend to discover and proxy to another Memory Layer backend when PostgreSQL is unavailable.",
                    global_config_path
                        .as_ref()
                        .unwrap_or(&config_path)
                        .display()
                ))
            },
            false,
        ));

        let runtime_dir = automation_runtime_dir(&config, repo_root);
        let runtime_fix_applied = if !runtime_dir.exists() && fix {
            fs::create_dir_all(&runtime_dir)
                .with_context(|| format!("create {}", runtime_dir.display()))?;
            true
        } else {
            false
        };
        report.push(doctor_check(
            "automation.runtime_dir",
            if runtime_dir.exists() {
                DoctorStatus::Ok
            } else {
                DoctorStatus::Warn
            },
            if runtime_dir.exists() {
                "Automation runtime directory is present."
            } else {
                "Automation runtime directory is missing."
            },
            Some(runtime_dir.display().to_string()),
            if runtime_dir.exists() {
                None
            } else {
                Some("memory doctor --fix".to_string())
            },
            runtime_fix_applied,
        ));

        let resolved_repo_root = config
            .automation
            .repo_root
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| repo_root.to_path_buf());
        report.push(doctor_check(
            "automation.repo_root",
            if resolved_repo_root == repo_root {
                DoctorStatus::Ok
            } else {
                DoctorStatus::Warn
            },
            if resolved_repo_root == repo_root {
                "Automation repo_root matches the current repository."
            } else {
                "Automation repo_root differs from the current repository."
            },
            Some(resolved_repo_root.display().to_string()),
            if resolved_repo_root == repo_root {
                None
            } else {
                Some(format!(
                    "Edit {} and set [automation].repo_root",
                    config_path.display()
                ))
            },
            false,
        ));

        #[cfg(target_os = "macos")]
        {
            let manager_plist_path = watch_manager_launch_agent_path()?;
            let manager_status = launch_agent_status(watch_manager_launch_agent_label())?;
            let manager_installed = manager_plist_path.exists();
            report.push(doctor_check(
                "watcher.manager_service",
                if manager_status.running {
                    DoctorStatus::Ok
                } else {
                    DoctorStatus::Warn
                },
                if manager_status.running {
                    "Agent-linked watcher manager service is active."
                } else if manager_installed || manager_status.loaded {
                    "Agent-linked watcher manager service is installed but not active."
                } else {
                    "Agent-linked watcher manager service is not installed."
                },
                Some(format!(
                    "installed={} loaded={} active={} plist={}",
                    yes_no(manager_installed),
                    yes_no(manager_status.loaded),
                    yes_no(manager_status.running),
                    manager_plist_path.display()
                )),
                if manager_status.running {
                    None
                } else {
                    Some("memory watcher manager enable".to_string())
                },
                false,
            ));
        }

        #[cfg(not(target_os = "macos"))]
        {
            let manager_unit_path = user_systemd_unit_dir()?.join(WATCH_MANAGER_UNIT_NAME);
            let manager_installed = manager_unit_path.exists();
            let manager_enabled =
                run_systemctl_user(["is-enabled", WATCH_MANAGER_UNIT_NAME]).is_ok();
            let manager_active = run_systemctl_user(["is-active", WATCH_MANAGER_UNIT_NAME]).is_ok();
            report.push(doctor_check(
                "watcher.manager_service",
                if manager_active {
                    DoctorStatus::Ok
                } else {
                    DoctorStatus::Warn
                },
                if manager_active {
                    "Agent-linked watcher manager service is active."
                } else if manager_installed {
                    "Agent-linked watcher manager service is installed but not active."
                } else {
                    "Agent-linked watcher manager service is not installed."
                },
                Some(format!(
                    "installed={} enabled={} active={} unit={}",
                    yes_no(manager_installed),
                    yes_no(manager_enabled),
                    yes_no(manager_active),
                    manager_unit_path.display()
                )),
                if manager_active {
                    None
                } else {
                    Some("memory watcher manager enable".to_string())
                },
                false,
            ));
        }

        let client = Client::builder()
            .timeout(config.service.request_timeout)
            .build()
            .context("build doctor http client")?;
        let api = ApiClient::new(client, config.clone());

        match api.health().await {
            Ok(value) => {
                let role = value.get("role").and_then(|field| field.as_str());
                let upstream = value.get("upstream").cloned();
                report.push(doctor_check(
                    "backend.health",
                    DoctorStatus::Ok,
                    "Backend health endpoint is reachable.",
                    Some(value.to_string()),
                    None,
                    false,
                ));
                report.push(doctor_check(
                    "backend.role",
                    if role == Some("relay")
                        && upstream
                            .as_ref()
                            .and_then(|payload| payload.as_object())
                            .is_none()
                    {
                        DoctorStatus::Warn
                    } else {
                        DoctorStatus::Ok
                    },
                    match role {
                        Some("primary") => "Backend is running in primary mode.",
                        Some("relay") => "Backend is running in relay mode.",
                        _ => "Backend did not report a cluster role.",
                    },
                    match role {
                        Some("relay") => upstream.as_ref().map(|payload| payload.to_string()),
                        Some(other) => Some(other.to_string()),
                        None => None,
                    },
                    if role == Some("relay")
                        && upstream
                            .as_ref()
                            .and_then(|payload| payload.as_object())
                            .is_none()
                    {
                        Some(
                            "Start a database-connected Memory service on the local network or fix the local database connection."
                                .to_string(),
                        )
                    } else {
                        None
                    },
                    false,
                ));
                match api.project_overview(project).await {
                    Ok(overview) => {
                        report.push(doctor_check(
                            "backend.project_overview",
                            DoctorStatus::Ok,
                            "Project overview endpoint is reachable.",
                            Some(format!(
                                "{} memories / {} raw captures",
                                overview.memory_entries_total, overview.raw_captures_total
                            )),
                            None,
                            false,
                        ));
                        if overview
                            .automation
                            .as_ref()
                            .is_some_and(|automation| automation.enabled)
                        {
                            let active_watchers = overview
                                .watchers
                                .as_ref()
                                .map(|watchers| watchers.active_count);
                            report.push(doctor_check(
                                "backend.watchers",
                                if active_watchers.unwrap_or(0) > 0 {
                                    DoctorStatus::Ok
                                } else {
                                    DoctorStatus::Warn
                                },
                                if active_watchers.unwrap_or(0) > 0 {
                                    "At least one active watcher is visible to the backend."
                                } else {
                                    "Automation is enabled but no active watcher is visible."
                                },
                                active_watchers.map(|count| format!("{count} active watcher(s)")),
                                if active_watchers.unwrap_or(0) > 0 {
                                    None
                                } else {
                                    Some(if cfg!(target_os = "macos") {
                                        format!("memory watcher enable --project {}", project)
                                    } else {
                                        "memory watcher manager enable".to_string()
                                    })
                                },
                                false,
                            ));
                        }

                        if repo_root.join(".git").exists() {
                            match api.project_commits(project, 1, 0).await {
                                Ok(commits) => report.push(doctor_check(
                                    "history.commit_sync",
                                    if commits.total > 0 {
                                        DoctorStatus::Ok
                                    } else {
                                        DoctorStatus::Warn
                                    },
                                    if commits.total > 0 {
                                        "Commit history has been imported for this project."
                                    } else {
                                        "No commit history has been imported for this project."
                                    },
                                    Some(format!("{} stored commit(s)", commits.total)),
                                    if commits.total > 0 {
                                        None
                                    } else {
                                        Some(format!("memory commits sync --project {}", project))
                                    },
                                    false,
                                )),
                                Err(error) => report.push(doctor_check(
                                    "history.commit_sync",
                                    DoctorStatus::Warn,
                                    "Could not load project commit history.",
                                    Some(error.to_string()),
                                    Some(format!("memory commits sync --project {}", project)),
                                    false,
                                )),
                            }
                        }
                    }
                    Err(error) => report.push(doctor_check(
                        "backend.project_overview",
                        DoctorStatus::Warn,
                        "Project overview endpoint did not return data.",
                        Some(error.to_string()),
                        Some(format!("memory init --project {}", project)),
                        false,
                    )),
                }

                let (http_status, http_details) = tcp_endpoint_status(&config.service.bind_addr);
                report.push(doctor_check(
                    "backend.http_endpoint",
                    if matches!(http_status, DoctorStatus::Fail) {
                        DoctorStatus::Fail
                    } else {
                        DoctorStatus::Ok
                    },
                    "Configured HTTP endpoint is reachable.",
                    Some(http_details),
                    None,
                    false,
                ));

                let (tcp_status, tcp_details) = tcp_endpoint_status(&config.service.capnp_tcp_addr);
                report.push(doctor_check(
                    "backend.capnp_tcp_endpoint",
                    if matches!(tcp_status, DoctorStatus::Fail) {
                        DoctorStatus::Fail
                    } else {
                        DoctorStatus::Ok
                    },
                    "Configured Cap'n Proto TCP endpoint has a listener.",
                    Some(tcp_details),
                    None,
                    false,
                ));

                let (unix_status, unix_details) =
                    unix_socket_status(&config.service.capnp_unix_socket);
                report.push(doctor_check(
                    "backend.capnp_unix_socket",
                    if matches!(unix_status, DoctorStatus::Fail) {
                        DoctorStatus::Fail
                    } else {
                        DoctorStatus::Ok
                    },
                    "Configured Cap'n Proto Unix socket path is active.",
                    Some(unix_details),
                    None,
                    false,
                ));
            }
            Err(error) => {
                report.push(doctor_check(
                    "backend.health",
                    DoctorStatus::Fail,
                    "Backend health endpoint is not reachable.",
                    Some(error.to_string()),
                    Some(if database_connect_error.is_some() && !config.cluster.enabled {
                        format!(
                            "{} or enable relay discovery in {} and rerun `memory service enable`",
                            backend_start_hint(&config_path),
                            global_config_path
                                .as_ref()
                                .unwrap_or(&config_path)
                                .display()
                        )
                    } else {
                        backend_start_hint(&config_path)
                    }),
                    false,
                ));
                report.push(doctor_check(
                    "backend.project_overview",
                    DoctorStatus::Skipped,
                    "Skipped project overview because the backend is unavailable.",
                    None,
                    None,
                    false,
                ));
                report.push(doctor_check(
                    "history.commit_sync",
                    DoctorStatus::Skipped,
                    "Skipped commit history check because the backend is unavailable.",
                    None,
                    None,
                    false,
                ));

                let (http_status, http_details) = tcp_endpoint_status(&config.service.bind_addr);
                report.push(doctor_check(
                    "backend.http_endpoint",
                    http_status,
                    "Configured HTTP endpoint is not serving Memory Layer health.",
                    Some(http_details),
                    Some(format!(
                        "Start the intended backend for {} or change [service].bind_addr",
                        project
                    )),
                    false,
                ));

                let (tcp_status, tcp_details) = tcp_endpoint_status(&config.service.capnp_tcp_addr);
                report.push(doctor_check(
                    "backend.capnp_tcp_endpoint",
                    tcp_status,
                    "Configured Cap'n Proto TCP endpoint is not confirmed healthy.",
                    Some(tcp_details),
                    Some(format!(
                        "Start the intended backend for {} or change [service].capnp_tcp_addr",
                        project
                    )),
                    false,
                ));

                let (unix_status, unix_details) =
                    unix_socket_status(&config.service.capnp_unix_socket);
                report.push(doctor_check(
                    "backend.capnp_unix_socket",
                    unix_status,
                    "Configured Cap'n Proto Unix socket is not confirmed healthy.",
                    Some(unix_details),
                    Some(format!(
                        "Start the intended backend for {} or change [service].capnp_unix_socket",
                        project
                    )),
                    false,
                ));
            }
        }

        match load_state(project, &resolved_repo_root, &config.automation).await {
            Ok(state) => report.push(doctor_check(
                "automation.state",
                if config.automation.enabled {
                    DoctorStatus::Ok
                } else {
                    DoctorStatus::Skipped
                },
                if config.automation.enabled {
                    "Automation state can be loaded."
                } else {
                    "Skipped automation state because automation is disabled."
                },
                Some(format!(
                    "enabled={} dirty_files={}",
                    state.enabled,
                    state.current_session.changed_files.len()
                )),
                None,
                false,
            )),
            Err(error) => report.push(doctor_check(
                "automation.state",
                if config.automation.enabled {
                    DoctorStatus::Warn
                } else {
                    DoctorStatus::Skipped
                },
                if config.automation.enabled {
                    "Automation state could not be loaded."
                } else {
                    "Skipped automation state because automation is disabled."
                },
                Some(error.to_string()),
                Some("memory doctor --fix".to_string()),
                false,
            )),
        }

        let remember_prereqs = detect_changed_files().is_ok();
        report.push(doctor_check(
            "workflow.remember_ready",
            if remember_prereqs {
                DoctorStatus::Ok
            } else {
                DoctorStatus::Warn
            },
            if remember_prereqs {
                "Remember workflow prerequisites look usable."
            } else {
                "Remember workflow could not inspect repo state."
            },
            None,
            if remember_prereqs {
                None
            } else {
                Some("Ensure git is available and run inside the repo".to_string())
            },
            false,
        ));

        if repo_uses_go_skill_runtime(repo_root) {
            let go_available = go_runtime_available();
            report.push(doctor_check(
                "workflow.skill_runtime_go",
                if go_available {
                    DoctorStatus::Ok
                } else {
                    DoctorStatus::Warn
                },
                if go_available {
                    "Go runtime is available for the repo-local memory skill helper."
                } else {
                    "Repo-local memory skills require `go run`, but Go is not available."
                },
                None,
                if go_available {
                    None
                } else {
                    Some(
                        "Install Go and ensure `go` is on PATH before using the repo-local memory skills."
                            .to_string(),
                    )
                },
                false,
            ));
        } else {
            report.push(doctor_check(
                "workflow.skill_runtime_go",
                DoctorStatus::Skipped,
                "Skipped Go runtime check because the repo-local memory skill helper is not installed.",
                None,
                None,
                false,
            ));
        }
    } else {
        for (id, summary) in [
            (
                "config.database_url",
                "Skipped database URL validation because config could not load.",
            ),
            (
                "config.api_token",
                "Skipped API token validation because config could not load.",
            ),
            (
                "automation.runtime_dir",
                "Skipped automation runtime checks because config could not load.",
            ),
            (
                "config.llm_model",
                "Skipped LLM model validation because config could not load.",
            ),
            (
                "config.llm_api_key",
                "Skipped LLM API key validation because config could not load.",
            ),
            (
                "automation.repo_root",
                "Skipped automation repo_root check because config could not load.",
            ),
            (
                "backend.health",
                "Skipped backend health check because config could not load.",
            ),
            (
                "backend.project_overview",
                "Skipped project overview check because config could not load.",
            ),
            (
                "automation.state",
                "Skipped automation state check because config could not load.",
            ),
            (
                "workflow.remember_ready",
                "Skipped remember readiness check because config could not load.",
            ),
            (
                "workflow.skill_runtime_go",
                "Skipped skill helper Go runtime check because config could not load.",
            ),
        ] {
            report.push(doctor_check(
                id,
                DoctorStatus::Skipped,
                summary,
                None,
                None,
                false,
            ));
        }
    }

    Ok(report)
}

pub(crate) fn repair_repo_bootstrap(repo_root: &Path, project: &str) -> Result<()> {
    let mem_dir = repo_root.join(".mem");
    let project_paths = mem_platform::project_paths(repo_root, project)
        .ok_or_else(|| anyhow::anyhow!("could not resolve user project config paths"))?;
    let runtime_dir = project_paths.runtime_dir();
    let config_path = project_paths.config_path();
    let env_path = project_paths.env_path();
    let home_project_path = project_paths.project_path();
    let project_path = mem_dir.join("project.toml");
    let legacy_config_path = mem_dir.join("config.toml");
    let legacy_env_path = mem_dir.join("memory-layer.env");
    let local_gitignore_path = mem_dir.join(".gitignore");
    let agent_config_path = repo_root.join(".agents").join("memory-layer.toml");
    let skill_root = repo_root.join(".agents").join("skills");

    fs::create_dir_all(&project_paths.config_dir)
        .with_context(|| format!("create {}", project_paths.config_dir.display()))?;
    fs::create_dir_all(&runtime_dir)
        .with_context(|| format!("create {}", runtime_dir.display()))?;
    if !config_path.exists() {
        if legacy_config_path.exists() {
            fs::copy(&legacy_config_path, &config_path).with_context(|| {
                format!(
                    "copy {} to {}",
                    legacy_config_path.display(),
                    config_path.display()
                )
            })?;
        } else {
            fs::write(&config_path, render_repo_config(repo_root, &project_paths))
                .with_context(|| format!("write {}", config_path.display()))?;
        }
    }
    if !env_path.exists() && legacy_env_path.exists() {
        migrate_legacy_env_or_create_token(&legacy_env_path, &env_path)?;
    }
    if !home_project_path.exists() {
        fs::write(
            &home_project_path,
            render_project_metadata(project, repo_root),
        )
        .with_context(|| format!("write {}", home_project_path.display()))?;
    }
    fs::create_dir_all(&mem_dir).context("create .mem")?;
    if !project_path.exists() {
        fs::write(&project_path, render_project_metadata(project, repo_root))
            .context("write .mem/project.toml")?;
    }
    ensure_mem_gitignore(&local_gitignore_path, false)?;
    if !agent_config_path.exists() {
        if let Some(parent) = agent_config_path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        fs::write(
            &agent_config_path,
            render_agent_project_config(project, repo_root),
        )
        .context("write .agents/memory-layer.toml")?;
    }
    if missing_memory_skill_dirs(&skill_root).next().is_some() {
        let skill_template_dir = download_github_skill_template()
            .ok()
            .or_else(discover_skill_template_dir)
            .ok_or_else(|| {
                anyhow::anyhow!("could not locate packaged memory-layer skill template")
            })?;
        sync_memory_skill_bundle(&skill_template_dir, &skill_root, false)?;
    }
    ensure_claude_md_memory_section(repo_root, project)?;
    Ok(())
}

pub(crate) fn migrate_legacy_env_or_create_token(
    legacy_env_path: &Path,
    env_path: &Path,
) -> Result<()> {
    match fs::copy(legacy_env_path, env_path) {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            ensure_shared_service_api_token(env_path, None, true).with_context(|| {
                format!("create replacement service token in {}", env_path.display())
            })?;
            Ok(())
        }
        Err(error) => Err(error).with_context(|| {
            format!(
                "copy {} to {}",
                legacy_env_path.display(),
                env_path.display()
            )
        }),
    }
}

pub(crate) fn root_gitignore_contains_mem(repo_root: &Path) -> Result<bool> {
    let path = repo_root.join(".gitignore");
    if !path.exists() {
        return Ok(false);
    }
    let content = fs::read_to_string(path)?;
    Ok(content.lines().any(|line| line.trim() == "/.mem"))
}

pub(crate) fn is_placeholder_database_url(value: &str) -> bool {
    value.contains("<password>") || value.trim().is_empty()
}

pub(crate) fn mask_database_url(value: &str) -> String {
    if let Some((prefix, rest)) = value.split_once("://")
        && let Some((creds, suffix)) = rest.split_once('@')
        && creds.contains(':')
    {
        return format!("{prefix}://<redacted>@{suffix}");
    }
    value.to_string()
}

pub(crate) fn automation_runtime_dir(config: &AppConfig, repo_root: &Path) -> PathBuf {
    if let Some(path) = &config.automation.state_file_path {
        PathBuf::from(path)
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| default_automation_runtime_dir(repo_root))
    } else if let Some(path) = &config.automation.audit_log_path {
        PathBuf::from(path)
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| default_automation_runtime_dir(repo_root))
    } else {
        default_automation_runtime_dir(repo_root)
    }
}

pub(crate) fn default_automation_runtime_dir(repo_root: &Path) -> PathBuf {
    mem_api::project_paths_for_repo(repo_root)
        .map(|paths| paths.runtime_dir())
        .unwrap_or_else(|| repo_root.join(".mem").join("runtime"))
}

#[derive(Clone, Debug, Default)]
pub(crate) struct LocalServiceOverrides {
    pub(crate) bind_addr: String,
    pub(crate) capnp_tcp_addr: String,
    pub(crate) capnp_unix_socket: String,
}

impl LocalServiceOverrides {
    fn is_enabled(&self) -> bool {
        !self.bind_addr.trim().is_empty()
            || !self.capnp_tcp_addr.trim().is_empty()
            || !self.capnp_unix_socket.trim().is_empty()
    }
}

pub(crate) fn default_local_service_overrides(repo_root: &Path) -> LocalServiceOverrides {
    let socket_path = mem_api::project_paths_for_repo(repo_root)
        .map(|paths| paths.runtime_dir().join("memory-layer.capnp.sock"))
        .unwrap_or_else(|| {
            repo_root
                .join(".mem")
                .join("runtime")
                .join("memory-layer.capnp.sock")
        });
    LocalServiceOverrides {
        bind_addr: "127.0.0.1:4140".to_string(),
        capnp_tcp_addr: "127.0.0.1:4141".to_string(),
        capnp_unix_socket: socket_path.display().to_string(),
    }
}

pub(crate) fn read_local_service_overrides(repo_root: &Path) -> Option<LocalServiceOverrides> {
    let config_path = mem_api::project_paths_for_repo(repo_root)
        .map(|paths| paths.config_path())
        .filter(|path| path.is_file())
        .unwrap_or_else(|| repo_root.join(".mem").join("config.toml"));
    let content = fs::read_to_string(config_path).ok()?;
    let mut in_service = false;
    let mut overrides = LocalServiceOverrides::default();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_service = trimmed == "[service]";
            continue;
        }
        if !in_service {
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("bind_addr = ") {
            overrides.bind_addr = value.trim_matches('"').to_string();
        } else if let Some(value) = trimmed.strip_prefix("capnp_tcp_addr = ") {
            overrides.capnp_tcp_addr = value.trim_matches('"').to_string();
        } else if let Some(value) = trimmed.strip_prefix("capnp_unix_socket = ") {
            overrides.capnp_unix_socket = value.trim_matches('"').to_string();
        }
    }

    overrides.is_enabled().then_some(overrides)
}

pub(crate) fn tcp_endpoint_status(addr: &str) -> (DoctorStatus, String) {
    match addr.parse::<SocketAddr>() {
        Ok(socket_addr) => {
            match TcpStream::connect_timeout(&socket_addr, Duration::from_millis(250)) {
                Ok(_) => (DoctorStatus::Warn, format!("listener detected on {addr}")),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    (DoctorStatus::Ok, format!("no listener detected on {addr}"))
                }
                Err(error) => (DoctorStatus::Warn, error.to_string()),
            }
        }
        Err(error) => (
            DoctorStatus::Fail,
            format!("invalid socket address: {error}"),
        ),
    }
}

pub(crate) fn unix_socket_status(path: &str) -> (DoctorStatus, String) {
    #[cfg(unix)]
    {
        let socket_path = Path::new(path);
        if !socket_path.exists() {
            return (DoctorStatus::Ok, "socket path is free".to_string());
        }

        match UnixStream::connect(socket_path) {
            Ok(_) => (
                DoctorStatus::Warn,
                format!("listener detected on {}", socket_path.display()),
            ),
            Err(error) => (
                DoctorStatus::Warn,
                format!("path exists but is not accepting connections: {error}"),
            ),
        }
    }

    #[cfg(not(unix))]
    {
        let _ = path;
        (
            DoctorStatus::Skipped,
            "unix socket checks are not available on this platform".to_string(),
        )
    }
}

pub(crate) fn print_doctor_report(report: &DoctorReport) {
    println!(
        "Doctor report for project {} at {}\n",
        report.project, report.repo_root
    );
    if let Some(global_config_path) = &report.global_config_path {
        println!("Merged global config: {global_config_path}");
    } else {
        println!(
            "Merged global config: <not found> (expected at {})",
            default_global_config_path_label()
        );
    }
    println!("Project config: {}\n", report.config_path);
    for check in &report.checks {
        let icon = match check.status {
            DoctorStatus::Ok => "OK",
            DoctorStatus::Warn => "WARN",
            DoctorStatus::Fail => "FAIL",
            DoctorStatus::Skipped => "SKIP",
        };
        println!("[{icon}] {} - {}", check.id, check.summary);
        if let Some(details) = &check.details {
            println!("  details: {details}");
        }
        if let Some(fix) = &check.suggested_fix {
            println!("  fix: {fix}");
        }
        if check.fix_applied {
            println!("  applied: true");
        }
    }

    let ok = report
        .checks
        .iter()
        .filter(|check| check.status == DoctorStatus::Ok)
        .count();
    let warn = report
        .checks
        .iter()
        .filter(|check| check.status == DoctorStatus::Warn)
        .count();
    let fail = report
        .checks
        .iter()
        .filter(|check| check.status == DoctorStatus::Fail)
        .count();
    let skipped = report
        .checks
        .iter()
        .filter(|check| check.status == DoctorStatus::Skipped)
        .count();
    println!("\nSummary: {ok} ok, {warn} warn, {fail} fail, {skipped} skipped");
}

pub(crate) fn default_global_config_path_label() -> String {
    default_global_config_path().display().to_string()
}
