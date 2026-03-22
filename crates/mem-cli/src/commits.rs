use std::{path::Path, process::Command as ProcessCommand};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use mem_api::CommitSyncItem;

pub(crate) fn collect_git_commits(
    repo_root: &Path,
    since: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<CommitSyncItem>> {
    let mut args = vec![
        "log".to_string(),
        "--date=iso-strict".to_string(),
        "--format=%x1e%H%x1f%h%x1f%cI%x1f%an%x1f%ae%x1f%P%x1f%s%x1f%b".to_string(),
        "--name-only".to_string(),
        "--no-merges".to_string(),
    ];
    let limit_value = limit.unwrap_or(0);
    if limit_value > 0 {
        args.push("-n".to_string());
        args.push(limit_value.to_string());
    }
    if let Some(since) = since {
        args.push("--since".to_string());
        args.push(since.to_string());
    }

    let output = git_output(repo_root, args)?;
    parse_git_log_output(&output)
}

fn git_output(repo_root: &Path, args: impl IntoIterator<Item = String>) -> Result<String> {
    let output = ProcessCommand::new("git")
        .args(args)
        .current_dir(repo_root)
        .output()
        .with_context(|| format!("run git in {}", repo_root.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!("git failed: {stderr}");
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn parse_git_log_output(output: &str) -> Result<Vec<CommitSyncItem>> {
    let mut commits = Vec::new();
    for record in output
        .split('\u{1e}')
        .filter(|record| !record.trim().is_empty())
    {
        let mut lines = record.lines();
        let Some(header) = lines.next() else {
            continue;
        };
        let fields = header.split('\u{1f}').collect::<Vec<_>>();
        if fields.len() < 8 {
            continue;
        }

        let committed_at = DateTime::parse_from_rfc3339(fields[2].trim())
            .with_context(|| format!("parse commit timestamp {}", fields[2].trim()))?
            .with_timezone(&Utc);
        let changed_paths = lines
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        let parent_hashes = fields[5]
            .split_whitespace()
            .filter(|value| !value.trim().is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        commits.push(CommitSyncItem {
            hash: fields[0].trim().to_string(),
            short_hash: fields[1].trim().to_string(),
            committed_at,
            author_name: non_empty(fields[3]),
            author_email: non_empty(fields[4]),
            parent_hashes,
            subject: fields[6].trim().to_string(),
            body: fields[7].trim().to_string(),
            changed_paths,
        });
    }
    Ok(commits)
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_git_log_output_extracts_commits() {
        let output = concat!(
            "\u{1e}",
            "abc123\u{1f}abc123\u{1f}2026-03-22T10:11:12+00:00\u{1f}Olivier\u{1f}olivier@example.com\u{1f}parent1 parent2\u{1f}Add feature\u{1f}Body line\n",
            "src/main.rs\n",
            "README.md\n"
        );

        let commits = parse_git_log_output(output).unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].hash, "abc123");
        assert_eq!(commits[0].subject, "Add feature");
        assert_eq!(commits[0].changed_paths, vec!["src/main.rs", "README.md"]);
        assert_eq!(commits[0].parent_hashes, vec!["parent1", "parent2"]);
    }
}
