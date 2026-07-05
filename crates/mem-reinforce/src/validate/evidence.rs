//! Deterministic evidence gathering (stage 1 of validation): everything the
//! verdict provider is allowed to reason over, plus the reference allowlist
//! used to reject hallucinated citations. Read-only — nothing here mutates
//! project state, and gathering never counts as memory access.

use std::collections::HashSet;
use std::path::{Component, Path};

use anyhow::{Context, Result};
use sqlx::PgPool;
use uuid::Uuid;

use crate::repository::{
    MemorySnapshot, PriorValidationRun, RelatedMemorySnapshot, SourceEvidence,
    fetch_memory_snapshot, fetch_memory_tags, fetch_prior_validation_runs, fetch_related_snapshots,
    fetch_source_evidence,
};

/// Everything a [`super::VerdictProvider`] may consult for one memory.
#[derive(Debug, Clone)]
pub struct ValidationContext {
    pub memory: MemorySnapshot,
    pub tags: Vec<String>,
    pub sources: Vec<SourceEvidence>,
    pub related: Vec<RelatedMemorySnapshot>,
    pub prior_runs: Vec<PriorValidationRun>,
    /// `git log` lines (`<short-sha> <date> <subject>`) touching the
    /// memory's source paths since it was last validated (or created).
    pub git_log: Vec<String>,
    /// Exact references a verdict may cite as evidence.
    pub(crate) allowed_refs: HashSet<String>,
}

impl ValidationContext {
    pub fn allows_reference(&self, reference: &str) -> bool {
        self.allowed_refs.contains(reference)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn insert_allowed_reference(&mut self, reference: &str) {
        self.allowed_refs.insert(reference.to_string());
    }
}

const GIT_LOG_MAX_LINES: usize = 40;
const GIT_LOG_MAX_LINE_CHARS: usize = 200;

/// Gathers the full deterministic context for one memory version.
pub async fn gather_context(pool: &PgPool, memory_id: Uuid) -> Result<ValidationContext> {
    let memory = fetch_memory_snapshot(pool, memory_id)
        .await?
        .with_context(|| format!("memory {memory_id} not found or tombstoned"))?;
    let tags = fetch_memory_tags(pool, memory_id).await?;
    let sources = fetch_source_evidence(pool, memory_id).await?;
    let related = fetch_related_snapshots(pool, memory_id).await?;
    let prior_runs = fetch_prior_validation_runs(pool, memory.canonical_id, 3).await?;

    let source_paths: Vec<String> = sources
        .iter()
        .filter_map(|source| source.file_path.clone())
        .filter(|path| is_safe_repo_relative_path(path))
        .collect();
    let since = prior_runs
        .iter()
        .filter_map(|run| run.finished_at)
        .max()
        .unwrap_or(memory.created_at);
    let git_log = collect_git_log(&memory.repo_root, since, &source_paths).await;

    let mut allowed_refs = HashSet::new();
    allowed_refs.insert(memory.memory_id.to_string());
    allowed_refs.insert(memory.canonical_id.to_string());
    for source in &sources {
        if let Some(path) = &source.file_path {
            allowed_refs.insert(path.clone());
            if let Some(symbol) = &source.symbol_name {
                allowed_refs.insert(format!("{path}#{symbol}"));
            }
        }
        if let Some(commit) = &source.git_commit {
            allowed_refs.insert(commit.clone());
        }
    }
    for entry in &related {
        allowed_refs.insert(entry.memory_id.to_string());
    }
    for line in &git_log {
        if let Some(sha) = line.split_whitespace().next() {
            allowed_refs.insert(sha.to_string());
        }
    }

    Ok(ValidationContext {
        memory,
        tags,
        sources,
        related,
        prior_runs,
        git_log,
        allowed_refs,
    })
}

/// Rejects absolute paths and any path escaping the repository root.
fn is_safe_repo_relative_path(path: &str) -> bool {
    let path = Path::new(path);
    !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

/// Read-only `git log` over the memory's source paths. Missing repository,
/// missing git, or any failure degrades to an empty log — validation must
/// work from database evidence alone if it has to.
async fn collect_git_log(
    repo_root: &str,
    since: chrono::DateTime<chrono::Utc>,
    paths: &[String],
) -> Vec<String> {
    if repo_root.trim().is_empty() || paths.is_empty() || !Path::new(repo_root).is_dir() {
        return Vec::new();
    }
    let mut command = tokio::process::Command::new("git");
    command
        .arg("-C")
        .arg(repo_root)
        .arg("log")
        .arg(format!("--since={}", since.format("%Y-%m-%dT%H:%M:%SZ")))
        .arg("--format=%h %as %s")
        .arg("--no-color")
        .arg(format!("--max-count={GIT_LOG_MAX_LINES}"))
        .arg("--");
    for path in paths {
        command.arg(path);
    }
    match command.output().await {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .lines()
            .take(GIT_LOG_MAX_LINES)
            .map(|line| line.chars().take(GIT_LOG_MAX_LINE_CHARS).collect())
            .collect(),
        Ok(output) => {
            tracing::debug!(
                repo_root,
                status = %output.status,
                "git log for validation evidence failed"
            );
            Vec::new()
        }
        Err(error) => {
            tracing::debug!(repo_root, error = %error, "git unavailable for validation evidence");
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsafe_source_paths() {
        assert!(is_safe_repo_relative_path("src/lib.rs"));
        assert!(is_safe_repo_relative_path("./docs/plan.md"));
        assert!(!is_safe_repo_relative_path("/etc/passwd"));
        assert!(!is_safe_repo_relative_path("../outside.rs"));
        assert!(!is_safe_repo_relative_path("src/../../outside.rs"));
    }
}
