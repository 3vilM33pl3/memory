use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use mem_api::ResumeCheckpoint;
use mem_platform::preferred_user_state_dir;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct StoredCheckpoints {
    #[serde(default)]
    checkpoints: BTreeMap<String, ResumeCheckpoint>,
}

fn checkpoint_store_path() -> Result<PathBuf> {
    let state_dir = preferred_user_state_dir()
        .ok_or_else(|| anyhow::anyhow!("could not determine user state directory"))?;
    Ok(state_dir.join("resume-checkpoints.json"))
}

pub(crate) fn checkpoint_store_location() -> Result<PathBuf> {
    checkpoint_store_path()
}

fn scope_key(project: &str, repo_root: &Path) -> String {
    format!("{}::{}", project, repo_root.display())
}

fn load_store() -> Result<StoredCheckpoints> {
    let path = checkpoint_store_path()?;
    if !path.exists() {
        return Ok(StoredCheckpoints::default());
    }
    let contents = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&contents).context("parse checkpoint store")
}

fn save_store(store: &StoredCheckpoints) -> Result<PathBuf> {
    let path = checkpoint_store_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(&path, serde_json::to_vec_pretty(store)?)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct StoredPlanThreads {
    #[serde(default)]
    threads: BTreeMap<String, String>,
}

fn plan_thread_store_path() -> Result<PathBuf> {
    let state_dir = preferred_user_state_dir()
        .ok_or_else(|| anyhow::anyhow!("could not determine user state directory"))?;
    Ok(state_dir.join("active-plan-threads.json"))
}

fn load_plan_threads() -> Result<StoredPlanThreads> {
    let path = plan_thread_store_path()?;
    if !path.exists() {
        return Ok(StoredPlanThreads::default());
    }
    let contents = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&contents).context("parse plan thread store")
}

fn save_plan_threads(store: &StoredPlanThreads) -> Result<()> {
    let path = plan_thread_store_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(&path, serde_json::to_vec_pretty(store)?)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// Remember which plan thread `checkpoint start-execution` most recently
/// started in this repo, so `finish-execution` can resolve it without an
/// explicit --thread-key even when several plans are active.
pub(crate) fn save_active_plan_thread(
    project: &str,
    repo_root: &Path,
    thread_key: &str,
) -> Result<()> {
    let mut store = load_plan_threads()?;
    store
        .threads
        .insert(scope_key(project, repo_root), thread_key.to_string());
    save_plan_threads(&store)
}

pub(crate) fn load_active_plan_thread(project: &str, repo_root: &Path) -> Option<String> {
    load_plan_threads()
        .ok()
        .and_then(|store| store.threads.get(&scope_key(project, repo_root)).cloned())
}

pub(crate) fn clear_active_plan_thread(project: &str, repo_root: &Path) -> Result<()> {
    let mut store = load_plan_threads()?;
    if store
        .threads
        .remove(&scope_key(project, repo_root))
        .is_some()
    {
        save_plan_threads(&store)?;
    }
    Ok(())
}

fn git_value(args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?;
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

pub(crate) fn load_checkpoint(project: &str, repo_root: &Path) -> Result<Option<ResumeCheckpoint>> {
    let store = load_store()?;
    Ok(store
        .checkpoints
        .get(&scope_key(project, repo_root))
        .cloned())
}

pub(crate) fn save_checkpoint(
    project: &str,
    repo_root: &Path,
    note: Option<String>,
) -> Result<(ResumeCheckpoint, PathBuf)> {
    let mut store = load_store()?;
    let checkpoint = build_checkpoint(project, repo_root, note);
    store
        .checkpoints
        .insert(scope_key(project, repo_root), checkpoint.clone());
    let path = save_store(&store)?;
    Ok((checkpoint, path))
}

pub(crate) fn build_checkpoint(
    project: &str,
    repo_root: &Path,
    note: Option<String>,
) -> ResumeCheckpoint {
    ResumeCheckpoint {
        project: project.to_string(),
        repo_root: repo_root.display().to_string(),
        marked_at: Utc::now(),
        note,
        git_branch: git_value(&["branch", "--show-current"]),
        git_head: git_value(&["rev-parse", "--short", "HEAD"]),
    }
}

pub(crate) fn format_checkpoint(checkpoint: &ResumeCheckpoint) -> String {
    let mut lines = vec![
        format!("Project: {}", checkpoint.project),
        format!("Repo root: {}", checkpoint.repo_root),
        format!(
            "Marked at: {}",
            checkpoint
                .marked_at
                .with_timezone(&Utc)
                .format("%Y-%m-%d %H:%M:%S UTC")
        ),
    ];
    if let Some(branch) = &checkpoint.git_branch {
        lines.push(format!("Branch: {branch}"));
    }
    if let Some(head) = &checkpoint.git_head {
        lines.push(format!("HEAD: {head}"));
    }
    if let Some(note) = &checkpoint.note {
        lines.push(format!("Note: {note}"));
    }
    lines.join("\n")
}

pub(crate) fn checkpoint_age_hours(checkpoint: &ResumeCheckpoint, now: DateTime<Utc>) -> i64 {
    (now - checkpoint.marked_at).num_hours()
}
