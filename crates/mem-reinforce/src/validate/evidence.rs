// SPDX-License-Identifier: AGPL-3.0-or-later

//! Deterministic evidence gathering (stage 1 of validation): everything the
//! verdict provider is allowed to reason over, plus the reference allowlist
//! used to reject hallucinated citations. Read-only — nothing here mutates
//! project state, and gathering never counts as memory access.

use std::collections::HashSet;
use std::path::{Component, Path};

use anyhow::{Context, Result};
use mem_record::ValidationProofScope;
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
    pub proof_snippets: Vec<ProofSnippet>,
    pub related: Vec<RelatedMemorySnapshot>,
    pub prior_runs: Vec<PriorValidationRun>,
    /// `git log` lines (`<short-sha> <date> <subject>`) touching the
    /// memory's source paths since it was last validated (or created).
    pub git_log: Vec<String>,
    /// Exact references a verdict may cite as evidence.
    pub(crate) allowed_refs: HashSet<String>,
    pub proof_scope: ValidationProofScope,
    pub proof_fallback_used: bool,
}

impl ValidationContext {
    pub fn allows_reference(&self, reference: &str) -> bool {
        self.allowed_refs.contains(reference)
    }

    #[cfg(test)]
    pub fn insert_allowed_reference(&mut self, reference: &str) {
        self.allowed_refs.insert(reference.to_string());
    }
}

/// A bounded repository snippet that can be cited by a validation verdict.
#[derive(Debug, Clone)]
pub struct ProofSnippet {
    pub file_path: String,
    pub line_start: usize,
    pub line_end: usize,
    pub text: String,
    pub fallback: bool,
}

impl ProofSnippet {
    pub fn evidence_ref(&self) -> String {
        if self.line_start == self.line_end {
            format!("{}:L{}", self.file_path, self.line_start)
        } else {
            format!("{}:L{}-L{}", self.file_path, self.line_start, self.line_end)
        }
    }
}

const GIT_LOG_MAX_LINES: usize = 40;
const GIT_LOG_MAX_LINE_CHARS: usize = 200;
const PROOF_MAX_SOURCE_FILES: usize = 16;
const PROOF_MAX_REPO_FILES: usize = 80;
const PROOF_MAX_SNIPPETS: usize = 24;
const PROOF_MAX_FILE_BYTES: u64 = 256 * 1024;
const PROOF_CONTEXT_RADIUS: usize = 1;

/// Gathers the full deterministic context for one memory version.
pub async fn gather_context(
    pool: &PgPool,
    memory_id: Uuid,
    proof_scope: ValidationProofScope,
    proof_fallback_used: bool,
) -> Result<ValidationContext> {
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
    let proof_snippets = collect_proof_snippets(
        &memory.repo_root,
        &memory.summary,
        &memory.canonical_text,
        &source_paths,
        proof_scope,
        proof_fallback_used,
    )
    .await;

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
    for snippet in &proof_snippets {
        allowed_refs.insert(snippet.evidence_ref());
    }

    Ok(ValidationContext {
        memory,
        tags,
        sources,
        proof_snippets,
        related,
        prior_runs,
        git_log,
        allowed_refs,
        proof_scope,
        proof_fallback_used,
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

async fn collect_proof_snippets(
    repo_root: &str,
    summary: &str,
    canonical_text: &str,
    source_paths: &[String],
    proof_scope: ValidationProofScope,
    proof_fallback_used: bool,
) -> Vec<ProofSnippet> {
    if repo_root.trim().is_empty() || !Path::new(repo_root).is_dir() {
        return Vec::new();
    }
    let tokens = proof_tokens(summary, canonical_text);
    if tokens.is_empty() {
        return Vec::new();
    }
    let paths = match proof_scope {
        ValidationProofScope::SourceFilesFirst => source_paths
            .iter()
            .take(PROOF_MAX_SOURCE_FILES)
            .cloned()
            .collect(),
        ValidationProofScope::HybridFallback if !proof_fallback_used => source_paths
            .iter()
            .take(PROOF_MAX_SOURCE_FILES)
            .cloned()
            .collect(),
        ValidationProofScope::WholeRepoScan | ValidationProofScope::HybridFallback => {
            collect_git_files(repo_root).await
        }
    };
    let mut snippets = Vec::new();
    for path in paths {
        if snippets.len() >= PROOF_MAX_SNIPPETS {
            break;
        }
        if !is_safe_repo_relative_path(&path) || should_skip_proof_path(&path) {
            continue;
        }
        let absolute = Path::new(repo_root).join(&path);
        let Ok(metadata) = tokio::fs::metadata(&absolute).await else {
            continue;
        };
        if !metadata.is_file() || metadata.len() > PROOF_MAX_FILE_BYTES {
            continue;
        }
        let Ok(text) = tokio::fs::read_to_string(&absolute).await else {
            continue;
        };
        snippets.extend(snippets_for_file(
            &path,
            &text,
            &tokens,
            proof_fallback_used || proof_scope == ValidationProofScope::WholeRepoScan,
            PROOF_MAX_SNIPPETS.saturating_sub(snippets.len()),
        ));
    }
    snippets
}

async fn collect_git_files(repo_root: &str) -> Vec<String> {
    let output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("ls-files")
        .arg("-z")
        .output()
        .await;
    match output {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .split('\0')
            .filter(|path| !path.is_empty())
            .take(PROOF_MAX_REPO_FILES)
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn proof_tokens(summary: &str, canonical_text: &str) -> Vec<String> {
    let mut tokens: Vec<String> = format!("{summary} {canonical_text}")
        .split(|ch: char| !ch.is_alphanumeric() && ch != '_' && ch != '-')
        .filter_map(|token| {
            let token = token.trim().to_lowercase();
            (token.len() >= 4 && !PROOF_STOP_WORDS.contains(&token.as_str())).then_some(token)
        })
        .collect();
    tokens.sort();
    tokens.dedup();
    tokens.truncate(32);
    tokens
}

const PROOF_STOP_WORDS: &[&str] = &[
    "about", "after", "also", "been", "from", "have", "into", "memory", "should", "that", "their",
    "there", "this", "through", "when", "with", "without",
];

fn snippets_for_file(
    file_path: &str,
    text: &str,
    tokens: &[String],
    fallback: bool,
    limit: usize,
) -> Vec<ProofSnippet> {
    let lines: Vec<&str> = text.lines().collect();
    let mut snippets = Vec::new();
    let mut covered_until = 0usize;
    for (index, line) in lines.iter().enumerate() {
        if snippets.len() >= limit {
            break;
        }
        let lower = line.to_lowercase();
        if !tokens.iter().any(|token| lower.contains(token)) {
            continue;
        }
        let line_no = index + 1;
        if line_no <= covered_until {
            continue;
        }
        let start = line_no.saturating_sub(PROOF_CONTEXT_RADIUS).max(1);
        let end = (line_no + PROOF_CONTEXT_RADIUS).min(lines.len());
        covered_until = end;
        let text = lines[start - 1..end]
            .iter()
            .map(|line| line.chars().take(220).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        snippets.push(ProofSnippet {
            file_path: file_path.to_string(),
            line_start: start,
            line_end: end,
            text,
            fallback,
        });
    }
    snippets
}

fn should_skip_proof_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.starts_with("target/")
        || lower.starts_with("node_modules/")
        || lower.starts_with(".git/")
        || lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".gif")
        || lower.ends_with(".webp")
        || lower.ends_with(".pdf")
        || lower.ends_with(".zip")
        || lower.ends_with(".gz")
        || lower.ends_with(".lock")
        || lower.contains("secret")
        || lower.contains("token")
        || lower.contains("credential")
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

    #[test]
    fn proof_snippets_use_citable_line_refs() {
        let snippets = snippets_for_file(
            "src/lib.rs",
            "pub fn unrelated() {}\n\npub fn validate_memory() {}\n",
            &["validate_memory".to_string()],
            true,
            4,
        );

        assert_eq!(snippets.len(), 1);
        assert_eq!(snippets[0].evidence_ref(), "src/lib.rs:L2-L3");
        assert!(snippets[0].fallback);
        assert!(snippets[0].text.contains("validate_memory"));
    }

    #[test]
    fn proof_search_skips_generated_binary_and_secret_paths() {
        assert!(should_skip_proof_path("target/debug/memory"));
        assert!(should_skip_proof_path("docs/img/screenshot.png"));
        assert!(should_skip_proof_path("config/service-token.txt"));
        assert!(!should_skip_proof_path("crates/mem-service/src/routes.rs"));
    }

    #[test]
    fn proof_tokens_drop_short_and_common_words() {
        let tokens = proof_tokens(
            "The service owns validation",
            "Memory should use recorded source evidence.",
        );

        assert!(tokens.contains(&"service".to_string()));
        assert!(tokens.contains(&"validation".to_string()));
        assert!(tokens.contains(&"recorded".to_string()));
        assert!(!tokens.contains(&"the".to_string()));
        assert!(!tokens.contains(&"should".to_string()));
    }
}
