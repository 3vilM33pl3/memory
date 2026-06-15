#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    env, fs,
    io::{self, Cursor},
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use zip::ZipArchive;

pub const MEMORY_SKILL_NAMES: &[&str] = &[
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
const GITHUB_SKILL_TEMPLATE_TIMEOUT: Duration = Duration::from_secs(10);
const GITHUB_ARCHIVE_ENV: &str = "MEMORY_LAYER_GITHUB_SKILL_TEMPLATE_ARCHIVE_URL";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillVersionStatus {
    UpToDate,
    Missing,
    Outdated,
    NewerThanTemplate,
    Unversioned,
    InvalidVersion,
    TemplateMissing,
}

impl SkillVersionStatus {
    pub fn label(self) -> &'static str {
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillBundleStatus {
    Ok,
    Warn,
    Error,
}

impl SkillBundleStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillUpgradeAction {
    Install,
    Replace,
    ReplaceForced,
    Skip,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillInventoryFilter {
    MemoryLayer,
    All,
}

impl SkillInventoryFilter {
    pub fn from_query(value: Option<&str>) -> Self {
        match value.map(str::trim).filter(|value| !value.is_empty()) {
            Some("all") => Self::All,
            _ => Self::MemoryLayer,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::MemoryLayer => "memory-layer",
            Self::All => "all",
        }
    }

    pub fn skills(self) -> &'static [&'static str] {
        match self {
            Self::MemoryLayer => &["memory-layer"],
            Self::All => MEMORY_SKILL_NAMES,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillVersionInfo {
    pub name: String,
    pub project_path: String,
    pub template_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template_version: Option<String>,
    pub status: SkillVersionStatus,
    pub action: SkillUpgradeAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillInventoryReport {
    pub project_root: String,
    pub project_skill_root: String,
    pub template_root: Option<String>,
    pub bundle_version: String,
    pub status: SkillBundleStatus,
    pub summary: String,
    pub filter: String,
    pub skills: Vec<SkillVersionInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillUpgradeReport {
    pub dry_run: bool,
    pub force: bool,
    pub backup_root: Option<String>,
    pub inventory: SkillInventoryReport,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillContentResponse {
    pub skill: SkillVersionInfo,
    pub content: Option<String>,
    pub content_truncated: bool,
}

pub fn missing_memory_skill_dirs<'a>(skill_root: &'a Path) -> impl Iterator<Item = PathBuf> + 'a {
    MEMORY_SKILL_NAMES
        .iter()
        .map(|name| skill_root.join(name))
        .filter(|path| !path.is_dir())
}

pub fn project_skill_inventory(repo_root: &Path, force: bool) -> SkillInventoryReport {
    project_skill_inventory_filtered(repo_root, force, SkillInventoryFilter::All)
}

pub fn project_skill_inventory_filtered(
    repo_root: &Path,
    force: bool,
    filter: SkillInventoryFilter,
) -> SkillInventoryReport {
    project_skill_inventory_with_template(repo_root, discover_skill_template_dir(), force, filter)
}

pub fn project_skill_inventory_with_template(
    repo_root: &Path,
    template_root: Option<PathBuf>,
    force: bool,
    filter: SkillInventoryFilter,
) -> SkillInventoryReport {
    let skill_root = repo_root.join(".agents").join("skills");
    let skills: Vec<_> = filter
        .skills()
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
        filter: filter.label().to_string(),
        skills,
    }
}

pub fn skill_bundle_status(skills: &[SkillVersionInfo]) -> (SkillBundleStatus, String) {
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

pub fn skill_version_info(
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

pub fn skill_upgrade_action(
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

pub fn read_skill_version(skill_dir: &Path) -> Result<Option<String>> {
    let skill_md = skill_dir.join("SKILL.md");
    if let Some(version) = read_skill_md_frontmatter_version(&skill_md)? {
        return Ok(Some(version));
    }
    read_simple_yaml_version(&skill_dir.join("agents").join("openai.yaml"))
}

pub fn read_skill_md_frontmatter_version(path: &Path) -> Result<Option<String>> {
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

pub fn read_simple_yaml_version(path: &Path) -> Result<Option<String>> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    let lines = content.lines().collect::<Vec<_>>();
    Ok(simple_yaml_value(&lines, "version"))
}

pub fn simple_yaml_value(lines: &[&str], key: &str) -> Option<String> {
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

pub fn discover_skill_template_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join(".agents")
            .join("skills"),
    );
    if let Some(path) = mem_platform::current_exe_share_subdir("skill-template") {
        candidates.push(path);
    }
    if let Ok(data_home) = env::var("XDG_DATA_HOME") {
        candidates.push(
            PathBuf::from(data_home)
                .join("memory-layer")
                .join("skill-template"),
        );
    }
    if let Some(state_dir) = mem_platform::preferred_user_state_dir() {
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

pub fn sync_memory_skill_bundle(src_root: &Path, dest_root: &Path, force: bool) -> Result<()> {
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

pub fn upgrade_project_skills(
    repo_root: &Path,
    force: bool,
    dry_run: bool,
    filter: SkillInventoryFilter,
) -> Result<SkillUpgradeReport> {
    let template_root = if dry_run {
        discover_skill_template_dir()
    } else {
        download_github_skill_template()
            .ok()
            .or_else(discover_skill_template_dir)
    };
    upgrade_project_skills_with_template(repo_root, template_root, force, dry_run, filter)
}

pub fn upgrade_project_skills_with_template(
    repo_root: &Path,
    template_root: Option<PathBuf>,
    force: bool,
    dry_run: bool,
    filter: SkillInventoryFilter,
) -> Result<SkillUpgradeReport> {
    let inventory = project_skill_inventory_with_template(repo_root, template_root, force, filter);
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

pub fn read_skill_content(
    repo_root: &Path,
    skill_name: &str,
    max_bytes: usize,
    filter: SkillInventoryFilter,
) -> Result<SkillContentResponse> {
    if !MEMORY_SKILL_NAMES.contains(&skill_name) {
        anyhow::bail!("unknown Memory skill `{skill_name}`");
    }
    let inventory = project_skill_inventory_filtered(repo_root, false, filter);
    let Some(skill) = inventory
        .skills
        .into_iter()
        .find(|skill| skill.name == skill_name)
    else {
        anyhow::bail!("skill `{skill_name}` is not included by filter `{}`", filter.label());
    };
    let path = repo_root
        .join(".agents")
        .join("skills")
        .join(skill_name)
        .join("SKILL.md");
    let content = match fs::read_to_string(&path) {
        Ok(content) => {
            let truncated = content.len() > max_bytes;
            let content = if truncated {
                content.chars().take(max_bytes).collect()
            } else {
                content
            };
            return Ok(SkillContentResponse {
                skill,
                content: Some(content),
                content_truncated: truncated,
            });
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    Ok(SkillContentResponse {
        skill,
        content,
        content_truncated: false,
    })
}

pub fn download_github_skill_template() -> Result<PathBuf> {
    let archive_url = env::var(GITHUB_ARCHIVE_ENV)
        .unwrap_or_else(|_| GITHUB_SKILL_TEMPLATE_ARCHIVE_URL.to_string());
    let bytes = read_http_url(&archive_url)?;
    extract_github_skill_template_archive(&bytes)
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
        mem_platform::preferred_user_state_dir().ok_or_else(|| anyhow::anyhow!("HOME is not set"))?;
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

pub fn copy_directory_tree(src: &Path, dest: &Path) -> Result<()> {
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
fn set_copied_file_permissions(path: &Path, mode: u32) -> Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .with_context(|| format!("chmod {:o} {}", mode, path.display()))
}

#[cfg(not(unix))]
fn set_copied_file_permissions(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_inventory_filter_defaults_to_memory_layer() {
        assert_eq!(
            SkillInventoryFilter::from_query(None),
            SkillInventoryFilter::MemoryLayer
        );
        assert_eq!(
            SkillInventoryFilter::from_query(Some("all")),
            SkillInventoryFilter::All
        );
    }

    #[test]
    fn skill_upgrade_action_replaces_outdated_skill() {
        assert_eq!(
            skill_upgrade_action(SkillVersionStatus::Outdated, true, false),
            SkillUpgradeAction::Replace
        );
        assert_eq!(
            skill_upgrade_action(SkillVersionStatus::NewerThanTemplate, true, false),
            SkillUpgradeAction::Skip
        );
    }

    #[test]
    fn simple_yaml_value_reads_quoted_versions() {
        let lines = ["name: test", "version: \"0.9.4\""];
        assert_eq!(simple_yaml_value(&lines, "version").as_deref(), Some("0.9.4"));
    }
}
