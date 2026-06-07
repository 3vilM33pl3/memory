#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    env, fs,
    io::{self, Cursor},
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    time::Duration,
};

use anyhow::{Context, Result};
use chrono::Utc;
use mem_platform as platform;
use serde::{Deserialize, Serialize};
use zip::ZipArchive;

use crate::commands::{
    runtime::default_global_config_path, status_support::default_global_config_path_label,
};

pub(crate) const MEMORY_SKILL_NAMES: &[&str] = &[
    "memory-layer",
    "memory-project-init",
    "memory-github-init",
    "memory-review-proposals",
    "memory-query-resume",
    "memory-plan-execution",
    "memory-direct-task-start",
    "memory-remember",
];

const GITHUB_SKILL_TEMPLATE_ARCHIVE_URL: &str =
    "https://github.com/3vilM33pl3/memory/archive/refs/heads/main.zip";
const GITHUB_SKILL_TEMPLATE_RAW_BASE: &str =
    "https://raw.githubusercontent.com/3vilM33pl3/memory/main/.agents/skills";
const GITHUB_SKILL_TEMPLATE_TIMEOUT: Duration = Duration::from_secs(10);
const GITHUB_ARCHIVE_ENV: &str = "MEMORY_LAYER_GITHUB_SKILL_TEMPLATE_ARCHIVE_URL";
const GITHUB_RAW_BASE_ENV: &str = "MEMORY_LAYER_GITHUB_SKILL_TEMPLATE_RAW_BASE";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SkillVersionStatus {
    UpToDate,
    Missing,
    Outdated,
    NewerThanTemplate,
    Unversioned,
    InvalidVersion,
    TemplateMissing,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SkillBundleStatus {
    Ok,
    Warn,
    Error,
}

impl SkillBundleStatus {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

impl SkillVersionStatus {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::UpToDate => "up-to-date",
            Self::Missing => "missing",
            Self::Outdated => "outdated",
            Self::NewerThanTemplate => "newer-than-template",
            Self::Unversioned => "unversioned",
            Self::InvalidVersion => "invalid-version",
            Self::TemplateMissing => "template-missing",
        }
    }

    fn needs_upgrade(self) -> bool {
        matches!(
            self,
            Self::Missing | Self::Outdated | Self::Unversioned | Self::InvalidVersion
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SkillUpgradeAction {
    Install,
    Replace,
    ReplaceForced,
    Skip,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SkillVersionInfo {
    pub(crate) name: String,
    pub(crate) project_path: String,
    pub(crate) template_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) project_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) template_version: Option<String>,
    pub(crate) status: SkillVersionStatus,
    pub(crate) action: SkillUpgradeAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SkillInventoryReport {
    pub(crate) project_root: String,
    pub(crate) project_skill_root: String,
    pub(crate) template_root: Option<String>,
    pub(crate) bundle_version: String,
    pub(crate) status: SkillBundleStatus,
    pub(crate) summary: String,
    pub(crate) skills: Vec<SkillVersionInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SkillUpgradeReport {
    pub(crate) dry_run: bool,
    pub(crate) force: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) backup_root: Option<String>,
    pub(crate) inventory: SkillInventoryReport,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct GitHubSkillVersionReport {
    pub(crate) raw_base_url: String,
    pub(crate) status: SkillBundleStatus,
    pub(crate) summary: String,
    pub(crate) skills: Vec<GitHubSkillVersionInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct GitHubSkillVersionInfo {
    pub(crate) name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) project_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) github_version: Option<String>,
    pub(crate) status: SkillVersionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) detail: Option<String>,
}

pub(crate) fn missing_memory_skill_dirs<'a>(
    skill_root: &'a Path,
) -> impl Iterator<Item = PathBuf> + 'a {
    MEMORY_SKILL_NAMES
        .iter()
        .map(|name| skill_root.join(name))
        .filter(|path| !path.is_dir())
}

pub(crate) fn project_skill_inventory(repo_root: &Path, force: bool) -> SkillInventoryReport {
    project_skill_inventory_with_template(repo_root, discover_skill_template_dir(), force)
}

pub(crate) fn project_skill_inventory_with_template(
    repo_root: &Path,
    template_root: Option<PathBuf>,
    force: bool,
) -> SkillInventoryReport {
    let skill_root = repo_root.join(".agents").join("skills");
    let skills: Vec<_> = MEMORY_SKILL_NAMES
        .iter()
        .map(|name| {
            let project_path = skill_root.join(name);
            let template_path = template_root.as_ref().map(|root| root.join(name));
            skill_version_info(name, &project_path, template_path.as_deref(), force)
        })
        .collect();
    let (status, summary) = skill_bundle_status(&skills);

    SkillInventoryReport {
        project_root: repo_root.display().to_string(),
        project_skill_root: skill_root.display().to_string(),
        template_root: template_root.map(|path| path.display().to_string()),
        bundle_version: env!("CARGO_PKG_VERSION").to_string(),
        status,
        summary,
        skills,
    }
}

pub(crate) fn skill_bundle_status(skills: &[SkillVersionInfo]) -> (SkillBundleStatus, String) {
    let error_count = skills
        .iter()
        .filter(|skill| skill.status == SkillVersionStatus::TemplateMissing)
        .count();
    let warn_count = skills
        .iter()
        .filter(|skill| {
            !matches!(
                skill.status,
                SkillVersionStatus::UpToDate | SkillVersionStatus::TemplateMissing
            )
        })
        .count();
    if error_count > 0 {
        (
            SkillBundleStatus::Error,
            format!("{error_count} skill template(s) missing"),
        )
    } else if warn_count > 0 {
        (
            SkillBundleStatus::Warn,
            format!("{warn_count} project skill(s) need upgrade"),
        )
    } else {
        (
            SkillBundleStatus::Ok,
            "all project skills match the installed template".to_string(),
        )
    }
}

pub(crate) fn skill_version_info(
    name: &str,
    project_path: &Path,
    template_path: Option<&Path>,
    force: bool,
) -> SkillVersionInfo {
    let project_version = read_skill_version(project_path).ok().flatten();
    let template_version = template_path.and_then(|path| read_skill_version(path).ok().flatten());
    let template_exists = template_path.is_some_and(Path::is_dir);
    let project_exists = project_path.is_dir();
    let mut detail = None;

    let status = if !template_exists {
        SkillVersionStatus::TemplateMissing
    } else if !project_exists {
        SkillVersionStatus::Missing
    } else if project_version.is_none() || template_version.is_none() {
        SkillVersionStatus::Unversioned
    } else {
        let project_raw = project_version.as_deref().unwrap_or_default();
        let template_raw = template_version.as_deref().unwrap_or_default();
        match (
            semver::Version::parse(project_raw),
            semver::Version::parse(template_raw),
        ) {
            (Ok(project), Ok(template)) if project == template => SkillVersionStatus::UpToDate,
            (Ok(project), Ok(template)) if project < template => SkillVersionStatus::Outdated,
            (Ok(_), Ok(_)) => SkillVersionStatus::NewerThanTemplate,
            (project_result, template_result) => {
                let mut parts = Vec::new();
                if let Err(error) = project_result {
                    parts.push(format!("project version `{project_raw}`: {error}"));
                }
                if let Err(error) = template_result {
                    parts.push(format!("template version `{template_raw}`: {error}"));
                }
                detail = Some(parts.join("; "));
                SkillVersionStatus::InvalidVersion
            }
        }
    };

    let action = skill_upgrade_action(status, project_exists, force);

    SkillVersionInfo {
        name: name.to_string(),
        project_path: project_path.display().to_string(),
        template_path: template_path.map(|path| path.display().to_string()),
        project_version,
        template_version,
        status,
        action,
        detail,
    }
}

pub(crate) fn skill_upgrade_action(
    status: SkillVersionStatus,
    project_exists: bool,
    force: bool,
) -> SkillUpgradeAction {
    if matches!(status, SkillVersionStatus::TemplateMissing) {
        return SkillUpgradeAction::Skip;
    }
    if force && project_exists {
        return SkillUpgradeAction::ReplaceForced;
    }
    if force {
        return SkillUpgradeAction::Install;
    }
    if matches!(status, SkillVersionStatus::Missing) {
        return SkillUpgradeAction::Install;
    }
    if status.needs_upgrade() {
        return SkillUpgradeAction::Replace;
    }
    SkillUpgradeAction::Skip
}

pub(crate) fn read_skill_version(skill_dir: &Path) -> Result<Option<String>> {
    let skill_md = skill_dir.join("SKILL.md");
    if let Some(version) = read_skill_md_frontmatter_version(&skill_md)? {
        return Ok(Some(version));
    }
    read_simple_yaml_version(&skill_dir.join("agents").join("openai.yaml"))
}

pub(crate) fn read_skill_md_frontmatter_version(path: &Path) -> Result<Option<String>> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    let mut lines = content.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Ok(None);
    }
    let mut frontmatter = Vec::new();
    for line in lines {
        if line.trim() == "---" {
            return Ok(simple_yaml_value(&frontmatter, "version"));
        }
        frontmatter.push(line);
    }
    Ok(None)
}

pub(crate) fn read_simple_yaml_version(path: &Path) -> Result<Option<String>> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    let lines = content.lines().collect::<Vec<_>>();
    Ok(simple_yaml_value(&lines, "version"))
}

pub(crate) fn simple_yaml_value(lines: &[&str], key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    lines.iter().find_map(|line| {
        let trimmed = line.trim();
        trimmed
            .strip_prefix(&prefix)
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

pub(crate) fn discover_skill_template_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join(".agents")
            .join("skills"),
    );
    if let Some(path) = platform::current_exe_share_subdir("skill-template") {
        candidates.push(path);
    }
    if let Ok(data_home) = env::var("XDG_DATA_HOME") {
        candidates.push(
            PathBuf::from(data_home)
                .join("memory-layer")
                .join("skill-template"),
        );
    }
    if let Some(state_dir) = platform::preferred_user_state_dir() {
        candidates.push(state_dir.join("skill-template"));
    }
    if let Ok(home) = env::var("HOME") {
        candidates.push(
            PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("memory-layer")
                .join("skill-template"),
        );
    }
    candidates.push(PathBuf::from("/usr/share/memory-layer/skill-template"));

    candidates.into_iter().find(|path| path.is_dir())
}

pub(crate) fn sync_memory_skill_bundle(
    src_root: &Path,
    dest_root: &Path,
    force: bool,
) -> Result<()> {
    fs::create_dir_all(dest_root).with_context(|| format!("create {}", dest_root.display()))?;
    for skill_name in MEMORY_SKILL_NAMES {
        let src = src_root.join(skill_name);
        if !src.is_dir() {
            anyhow::bail!("skill template is missing {}", src.display());
        }
        let dest = dest_root.join(skill_name);
        if dest.exists() {
            if force {
                fs::remove_dir_all(&dest).with_context(|| format!("remove {}", dest.display()))?;
            } else {
                continue;
            }
        }
        copy_directory_tree(&src, &dest)?;
    }
    Ok(())
}

pub(crate) fn upgrade_project_skills(
    repo_root: &Path,
    force: bool,
    dry_run: bool,
) -> Result<SkillUpgradeReport> {
    let template_root = if dry_run {
        discover_skill_template_dir()
    } else {
        download_github_skill_template()
            .ok()
            .or_else(discover_skill_template_dir)
    };
    upgrade_project_skills_with_template(repo_root, template_root, force, dry_run)
}

pub(crate) fn upgrade_project_skills_with_template(
    repo_root: &Path,
    template_root: Option<PathBuf>,
    force: bool,
    dry_run: bool,
) -> Result<SkillUpgradeReport> {
    let inventory = project_skill_inventory_with_template(repo_root, template_root, force);
    let backup_root = if dry_run
        || inventory.skills.iter().all(|skill| {
            matches!(
                skill.action,
                SkillUpgradeAction::Skip | SkillUpgradeAction::Install
            )
        }) {
        None
    } else {
        let runtime_dir = mem_api::project_paths_for_repo(repo_root)
            .map(|paths| paths.runtime_dir())
            .unwrap_or_else(|| repo_root.join(".mem").join("runtime"));
        Some(
            runtime_dir
                .join("skill-backups")
                .join(Utc::now().format("%Y%m%dT%H%M%SZ").to_string()),
        )
    };

    if !dry_run {
        if let Some(root) = &backup_root {
            fs::create_dir_all(root).with_context(|| format!("create {}", root.display()))?;
        }
        for skill in &inventory.skills {
            match skill.action {
                SkillUpgradeAction::Install
                | SkillUpgradeAction::Replace
                | SkillUpgradeAction::ReplaceForced => {
                    let template_path = skill
                        .template_path
                        .as_ref()
                        .map(PathBuf::from)
                        .ok_or_else(|| {
                            anyhow::anyhow!("skill template is missing for {}", skill.name)
                        })?;
                    let project_path = PathBuf::from(&skill.project_path);
                    if project_path.exists() {
                        let backup_root = backup_root.as_ref().ok_or_else(|| {
                            anyhow::anyhow!("backup root missing while replacing {}", skill.name)
                        })?;
                        let backup_path = backup_root.join(&skill.name);
                        copy_directory_tree(&project_path, &backup_path).with_context(|| {
                            format!(
                                "backup {} -> {}",
                                project_path.display(),
                                backup_path.display()
                            )
                        })?;
                        fs::remove_dir_all(&project_path)
                            .with_context(|| format!("remove {}", project_path.display()))?;
                    }
                    copy_directory_tree(&template_path, &project_path).with_context(|| {
                        format!(
                            "copy {} -> {}",
                            template_path.display(),
                            project_path.display()
                        )
                    })?;
                }
                SkillUpgradeAction::Skip => {}
            }
        }
    }

    Ok(SkillUpgradeReport {
        dry_run,
        force,
        backup_root: backup_root.map(|path| path.display().to_string()),
        inventory,
    })
}

pub(crate) fn print_skill_upgrade_report(report: &SkillUpgradeReport) {
    println!(
        "{} repo-local Memory skills at {}",
        if report.dry_run {
            "Would inspect"
        } else {
            "Inspected"
        },
        report.inventory.project_skill_root
    );
    if let Some(template_root) = &report.inventory.template_root {
        println!("Template: {template_root}");
    } else {
        println!("Template: <not found>");
    }
    if let Some(backup_root) = &report.backup_root {
        println!("Backup: {backup_root}");
    }
    println!(
        "Bundle: v{} {} ({})",
        report.inventory.bundle_version,
        report.inventory.status.label(),
        report.inventory.summary
    );
    println!();
    for skill in &report.inventory.skills {
        let project_version = skill.project_version.as_deref().unwrap_or("n/a");
        let template_version = skill.template_version.as_deref().unwrap_or("n/a");
        println!(
            "- {}: {} (project {}, template {}, action {:?})",
            skill.name,
            skill.status.label(),
            project_version,
            template_version,
            skill.action
        );
        if let Some(detail) = &skill.detail {
            println!("  detail: {detail}");
        }
    }
    if report.dry_run {
        println!(
            "\nDry run only. Run `memory upgrade` to apply the listed install/replace actions."
        );
    }
}

pub(crate) fn format_skill_inventory_summary(inventory: &SkillInventoryReport) -> String {
    let template = inventory
        .template_root
        .as_deref()
        .unwrap_or("<template not found>");
    let skills = inventory
        .skills
        .iter()
        .map(|skill| {
            format!(
                "{}={} local:{} template:{}",
                skill.name,
                skill.status.label(),
                skill.project_version.as_deref().unwrap_or("n/a"),
                skill.template_version.as_deref().unwrap_or("n/a")
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    format!(
        "bundle=v{} status={} summary={}; template={template}; {skills}",
        inventory.bundle_version,
        inventory.status.label(),
        inventory.summary
    )
}

pub(crate) fn github_skill_version_report(repo_root: &Path) -> Result<GitHubSkillVersionReport> {
    let raw_base = github_raw_base_url();
    let skill_root = repo_root.join(".agents").join("skills");
    let mut skills = Vec::new();
    for name in MEMORY_SKILL_NAMES {
        let project_version = read_skill_version(&skill_root.join(name)).ok().flatten();
        let github_version = fetch_github_skill_version(&raw_base, name)
            .with_context(|| format!("fetch GitHub version for {name}"))?;
        let (status, detail) =
            github_skill_version_status(project_version.as_deref(), github_version.as_deref());
        skills.push(GitHubSkillVersionInfo {
            name: (*name).to_string(),
            project_version,
            github_version,
            status,
            detail,
        });
    }
    let (status, summary) = github_skill_bundle_status(&skills);
    Ok(GitHubSkillVersionReport {
        raw_base_url: raw_base,
        status,
        summary,
        skills,
    })
}

pub(crate) fn format_github_skill_version_summary(report: &GitHubSkillVersionReport) -> String {
    let skills = report
        .skills
        .iter()
        .map(|skill| {
            format!(
                "{}={} local:{} github:{}",
                skill.name,
                skill.status.label(),
                skill.project_version.as_deref().unwrap_or("n/a"),
                skill.github_version.as_deref().unwrap_or("n/a")
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    format!(
        "status={} summary={}; source={}; {skills}",
        report.status.label(),
        report.summary,
        report.raw_base_url
    )
}

pub(crate) fn download_github_skill_template() -> Result<PathBuf> {
    let archive_url = github_archive_url();
    let bytes = read_url_or_file(&archive_url)
        .with_context(|| format!("download Memory Layer skill template from {archive_url}"))?;
    extract_github_skill_template_archive(&bytes)
}

fn github_archive_url() -> String {
    env::var(GITHUB_ARCHIVE_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| GITHUB_SKILL_TEMPLATE_ARCHIVE_URL.to_string())
}

fn github_raw_base_url() -> String {
    env::var(GITHUB_RAW_BASE_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| GITHUB_SKILL_TEMPLATE_RAW_BASE.to_string())
        .trim_end_matches('/')
        .to_string()
}

fn fetch_github_skill_version(raw_base: &str, skill_name: &str) -> Result<Option<String>> {
    let content = if let Some(path) = raw_base.strip_prefix("file://") {
        let path = PathBuf::from(path).join(skill_name).join("SKILL.md");
        fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?
    } else {
        let url = format!("{raw_base}/{skill_name}/SKILL.md");
        String::from_utf8(read_url_or_file(&url)?).context("decode GitHub skill metadata")?
    };
    read_skill_md_frontmatter_version_from_str(&content)
}

fn github_skill_version_status(
    project_version: Option<&str>,
    github_version: Option<&str>,
) -> (SkillVersionStatus, Option<String>) {
    match (project_version, github_version) {
        (_, None) => (
            SkillVersionStatus::TemplateMissing,
            Some("GitHub skill did not expose a version".to_string()),
        ),
        (None, Some(_)) => (SkillVersionStatus::Missing, None),
        (Some(project_raw), Some(github_raw)) => match (
            semver::Version::parse(project_raw),
            semver::Version::parse(github_raw),
        ) {
            (Ok(project), Ok(github)) if project == github => (SkillVersionStatus::UpToDate, None),
            (Ok(project), Ok(github)) if project < github => (SkillVersionStatus::Outdated, None),
            (Ok(_), Ok(_)) => (SkillVersionStatus::NewerThanTemplate, None),
            (project_result, github_result) => {
                let mut parts = Vec::new();
                if let Err(error) = project_result {
                    parts.push(format!("project version `{project_raw}`: {error}"));
                }
                if let Err(error) = github_result {
                    parts.push(format!("GitHub version `{github_raw}`: {error}"));
                }
                (SkillVersionStatus::InvalidVersion, Some(parts.join("; ")))
            }
        },
    }
}

fn github_skill_bundle_status(skills: &[GitHubSkillVersionInfo]) -> (SkillBundleStatus, String) {
    let error_count = skills
        .iter()
        .filter(|skill| skill.status == SkillVersionStatus::TemplateMissing)
        .count();
    let warn_count = skills
        .iter()
        .filter(|skill| {
            !matches!(
                skill.status,
                SkillVersionStatus::UpToDate | SkillVersionStatus::TemplateMissing
            )
        })
        .count();
    if error_count > 0 {
        (
            SkillBundleStatus::Error,
            format!("{error_count} GitHub skill(s) missing version metadata"),
        )
    } else if warn_count > 0 {
        (
            SkillBundleStatus::Warn,
            format!("{warn_count} project skill(s) differ from GitHub"),
        )
    } else {
        (
            SkillBundleStatus::Ok,
            "all project skills match GitHub".to_string(),
        )
    }
}

fn read_skill_md_frontmatter_version_from_str(content: &str) -> Result<Option<String>> {
    let mut lines = content.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Ok(None);
    }
    let mut frontmatter = Vec::new();
    for line in lines {
        if line.trim() == "---" {
            return Ok(simple_yaml_value(&frontmatter, "version"));
        }
        frontmatter.push(line);
    }
    Ok(None)
}

fn read_url_or_file(url: &str) -> Result<Vec<u8>> {
    if let Some(path) = url.strip_prefix("file://") {
        return fs::read(path).with_context(|| format!("read {path}"));
    }
    let url = url.to_string();
    std::thread::spawn(move || read_http_url(&url))
        .join()
        .map_err(|_| anyhow::anyhow!("GitHub skill template download worker panicked"))?
}

fn read_http_url(url: &str) -> Result<Vec<u8>> {
    let response = reqwest::blocking::Client::builder()
        .timeout(GITHUB_SKILL_TEMPLATE_TIMEOUT)
        .user_agent("memory-layer")
        .build()
        .context("build GitHub skill template client")?
        .get(url)
        .send()
        .with_context(|| format!("GET {url}"))?;
    let status = response.status();
    if !status.is_success() {
        anyhow::bail!("GET {url} returned {status}");
    }
    Ok(response.bytes()?.to_vec())
}

fn extract_github_skill_template_archive(bytes: &[u8]) -> Result<PathBuf> {
    let state_dir =
        platform::preferred_user_state_dir().ok_or_else(|| anyhow::anyhow!("HOME is not set"))?;
    let cache_root = state_dir.join("skill-template-github");
    let target = cache_root.join("main");
    let tmp = cache_root.join(format!(
        ".download-{}-{}",
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    if tmp.exists() {
        fs::remove_dir_all(&tmp).with_context(|| format!("remove {}", tmp.display()))?;
    }
    fs::create_dir_all(&tmp).with_context(|| format!("create {}", tmp.display()))?;

    let mut archive =
        ZipArchive::new(Cursor::new(bytes)).context("open GitHub skill template archive")?;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index)?;
        let Some(enclosed) = file.enclosed_name() else {
            continue;
        };
        let Some(relative) = skill_archive_relative_path(&enclosed) else {
            continue;
        };
        let out = tmp.join(relative);
        if file.is_dir() {
            fs::create_dir_all(&out).with_context(|| format!("create {}", out.display()))?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        let mut output =
            fs::File::create(&out).with_context(|| format!("create {}", out.display()))?;
        io::copy(&mut file, &mut output).with_context(|| format!("extract {}", out.display()))?;
    }

    for name in MEMORY_SKILL_NAMES {
        let skill = tmp.join(name);
        if !skill.join("SKILL.md").is_file() {
            anyhow::bail!(
                "GitHub skill template archive is missing {}",
                skill.display()
            );
        }
    }

    if target.exists() {
        fs::remove_dir_all(&target).with_context(|| format!("remove {}", target.display()))?;
    }
    fs::rename(&tmp, &target)
        .with_context(|| format!("rename {} -> {}", tmp.display(), target.display()))?;
    Ok(target)
}

fn skill_archive_relative_path(path: &Path) -> Option<PathBuf> {
    let components = path
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>();
    let skills_index = components
        .windows(2)
        .position(|window| window == [".agents", "skills"])?;
    let relative = &components[(skills_index + 2)..];
    let skill_name = relative.first()?;
    if !MEMORY_SKILL_NAMES.contains(skill_name) {
        return None;
    }
    let mut path = PathBuf::new();
    for part in relative {
        path.push(part);
    }
    Some(path)
}

pub(crate) fn copy_directory_tree(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest).with_context(|| format!("create {}", dest.display()))?;
    for entry in fs::read_dir(src).with_context(|| format!("read {}", src.display()))? {
        let entry = entry.with_context(|| format!("read entry in {}", src.display()))?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        let file_type = entry
            .file_type()
            .with_context(|| format!("read type for {}", src_path.display()))?;
        if file_type.is_dir() {
            copy_directory_tree(&src_path, &dest_path)?;
        } else if file_type.is_file() {
            fs::copy(&src_path, &dest_path).with_context(|| {
                format!("copy {} -> {}", src_path.display(), dest_path.display())
            })?;
            let mode = if src_path.extension().and_then(|ext| ext.to_str()) == Some("sh") {
                0o755
            } else {
                0o644
            };
            set_copied_file_permissions(&dest_path, mode)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) fn set_private_file_permissions(path: &Path) -> Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("chmod {}", path.display()))
}

#[cfg(not(unix))]
pub(crate) fn set_private_file_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
pub(crate) fn set_copied_file_permissions(path: &Path, mode: u32) -> Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .with_context(|| format!("chmod {}", path.display()))
}

#[cfg(not(unix))]
pub(crate) fn set_copied_file_permissions(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
}

pub(crate) fn render_repo_config(
    repo_root: &Path,
    project_paths: &mem_platform::ProjectPaths,
) -> String {
    let repo_root = repo_root.display();
    let runtime_dir = project_paths.runtime_dir();
    let socket_path = runtime_dir.join("memory-layer.capnp.sock");
    let audit_log_path = runtime_dir.join("automation.log");
    let state_file_path = runtime_dir.join("automation-state.json");
    format!(
        r#"# User-local project overrides for this project.
# Put shared defaults and secrets in the global config:
#   {}
# Shared LLM settings for `memory scan` should also live there under [llm].

# Uncomment [service] to run a repo-local dev backend alongside the shared one.
# Example dev endpoints:
# [service]
# bind_addr = "127.0.0.1:4140"
# capnp_unix_socket = "{}"
# capnp_tcp_addr = "127.0.0.1:4141"

[automation]
enabled = false
mode = "suggest"
repo_root = "{repo_root}"
file_events = true
poll_interval = "60s"
idle_threshold = "5m"
min_changed_files = 2
require_passing_test = false
ignored_paths = [".git/", "target/", ".mem/"]
audit_log_path = "{}"
state_file_path = "{}"
"#,
        default_global_config_path_label(),
        socket_path.display(),
        audit_log_path.display(),
        state_file_path.display()
    )
}

pub(crate) fn render_project_metadata(project: &str, repo_root: &Path) -> String {
    format!(
        r#"slug = "{project}"
repo_root = "{}"
"#,
        repo_root.display()
    )
}

pub(crate) fn render_agent_project_config(project: &str, repo_root: &Path) -> String {
    format!(
        r#"# Project-owned memory behavior.
# Less technical users should customize Memory Layer here.

[project]
slug = "{project}"
repo_root = "{}"

[capture]
include_paths = ["README.md", "docs/", "src/", "crates/", "scripts/", "packaging/"]
ignore_paths = [".git/", "target/", ".mem/", "node_modules/"]

[analysis]
analyzers = ["rust", "typescript", "python"]

[retrieval]
graph_enabled = false

[curation]
replacement_policy = "balanced"
"#,
        repo_root.display()
    )
}

const CLAUDE_MD_MEMORY_MARKER: &str = "## Memory Layer workflows";

pub(crate) fn render_claude_md_memory_section(project: &str) -> String {
    format!(
        r#"## Memory Layer workflows

This project uses Memory Layer to persist durable project knowledge. The `memory` CLI
must be on PATH (or use `cargo run --bin memory --` from the repo root).

### Shared invariants
1. Query memory before answering project-specific questions.
2. Use `resume` instead of a generic query for interruption-recovery prompts.
3. Save the approved plan before implementation begins when a planning phase turns into execution.
4. Verify plan-backed work is complete before claiming the task is finished.
5. Remember meaningful work after it is actually done.
6. Remember distilled code and codebase explanations after answering explanation requests.
7. Prefer insufficient evidence over unsupported conclusions.
8. Never invent provenance.

### Query and resume
Use when: the user asks a project-specific question or returns after an interruption.

```bash
memory query --project {project} --question "<question>"
memory resume --project {project}
```

### Plan execution
Use when: a planning session ends and the user approves execution.

Save checkpoint and plan at execution start:
```bash
memory checkpoint start-execution --project {project} --plan-file /tmp/approved-plan.md
```

Verify all plan items are complete before claiming finished:
```bash
memory checkpoint finish-execution --project {project}
```

### Remember completed work (mandatory post-task rule)
**After any meaningful repository work, run the remember workflow before sending the
final response** unless one of these is true:
- no durable knowledge was produced
- the work was purely trivial
- the user explicitly asked not to store memory

```bash
memory remember --project {project} \
  --title "<task title>" \
  --summary "<what changed>" \
  --note "<durable fact 1>" \
  --note "<durable fact 2>" \
  --file-changed "<path>"
```

This should default to storing durable project knowledge, not waiting for the user to ask.

### Store code explanations
Use when: you answered a request to explain code, a file, a module, an architecture path, or the whole codebase.

After answering, store a distilled reusable memory when the explanation is durable and grounded in inspected code or existing memory. Do not store the full chat answer, speculative claims, duplicates, or trivial explanations. Do not use `--file-changed` unless files actually changed.

```bash
memory remember --project {project} --type project \
  --title "Explained <file/module/codebase>" \
  --prompt "<user explanation request>" \
  --summary "<short explanation summary>" \
  --note "<stable explanation fact with file/module/symbol provenance>"
```

### Store user context
Use when: you learn about the user's role, preferences, or expertise.

```bash
memory remember --project {project} --type user --note "<what you learned>"
```

### Store feedback
Use when: the user corrects your approach or confirms a non-obvious choice.

```bash
memory remember --project {project} --type feedback \
  --note "<rule or validated approach>" \
  --note "<why: reason or context>"
```

### Store project context
Use when: you learn about goals, deadlines, or ongoing initiatives.

```bash
memory remember --project {project} --type project \
  --note "<fact or decision>" \
  --note "<why: motivation or constraint>"
```

### Store external reference
Use when: you learn about resources tracked in external systems.

```bash
memory remember --project {project} --type reference \
  --note "<what the resource is and where to find it>"
```
"#
    )
}

pub(crate) fn ensure_claude_md_memory_section(repo_root: &Path, project: &str) -> Result<()> {
    let path = repo_root.join("CLAUDE.md");
    let content = if path.exists() {
        fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?
    } else {
        String::new()
    };
    if content.contains(CLAUDE_MD_MEMORY_MARKER) {
        return Ok(());
    }
    let section = render_claude_md_memory_section(project);
    let updated = if content.is_empty() {
        format!("# Project Instructions\n\n{section}")
    } else {
        format!("{}\n\n{}", content.trim_end(), section)
    };
    fs::write(&path, updated).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub(crate) fn render_init_summary(
    repo_root: &Path,
    project: &str,
    config_path: &Path,
    project_path: &Path,
    agent_config_path: &Path,
    skills_root: &Path,
    print_only: bool,
) -> String {
    let action = if print_only {
        "Would prepare"
    } else {
        "Prepared"
    };
    let watcher_step = if cfg!(target_os = "macos") {
        "7. Optional: enable the Codex-linked watcher manager:\n   memory watcher manager enable\n   Legacy per-repo watcher service: memory watcher enable --project ".to_string()
            + project
    } else {
        "7. Optional: enable the Linux Codex-linked watcher manager:\n   memory watcher manager enable\n   Legacy per-repo watcher service: memory watcher enable --project ".to_string() + project
    };
    format!(
        "{action} memory bootstrap for project `{project}` at {}.\n\nFiles:\n- {} (user-local project config)\n- {} (repo-local project marker)\n- {} (agent-visible project behavior)\n- {} (bundled memory skills)\n\nNext steps:\n1. Set shared values like `database.url`, `service.api_token`, and `[llm]` config in {}\n2. Use {} for project runtime overrides\n3. Use {} to customize agent-visible project memory behavior\n4. Start the shared backend if it is not already running:\n   memory service run --config {}\n5. Optional: configure project-local [service] overrides if you want a parallel dev backend for this project\n6. Optional: run a project scan:\n   memory scan --project {}\n{}\n8. Open the TUI:\n   memory tui --project {}\n9. Use the repo-local memory skill bundle from {} (umbrella skill at {}/memory-layer)",
        repo_root.display(),
        config_path.display(),
        project_path.display(),
        agent_config_path.display(),
        skills_root.display(),
        default_global_config_path_label(),
        config_path.display(),
        agent_config_path.display(),
        default_global_config_path().display(),
        project,
        watcher_step,
        project,
        skills_root.display(),
        skills_root.display()
    )
}

pub(crate) fn resolve_repo_root(cwd: &Path) -> Result<PathBuf> {
    let output = ProcessCommand::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output();

    if let Ok(output) = output
        && output.status.success()
    {
        let stdout = String::from_utf8(output.stdout).context("decode git rev-parse output")?;
        let root = stdout.trim();
        if !root.is_empty() {
            return Ok(PathBuf::from(root));
        }
    }

    Ok(cwd.to_path_buf())
}
