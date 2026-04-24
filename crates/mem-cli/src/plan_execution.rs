use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Result;
use mem_platform as platform;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PlanChecklistItem {
    pub(crate) text: String,
    pub(crate) checked: bool,
}

pub(crate) fn derive_plan_title(
    explicit_title: Option<&str>,
    plan_markdown: &str,
    project: &str,
) -> String {
    if let Some(title) = explicit_title
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return title.to_string();
    }
    for line in plan_markdown.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(heading) = trimmed.strip_prefix('#') {
            let heading = heading.trim_start_matches('#').trim();
            if !heading.is_empty() {
                return heading.to_string();
            }
        }
        return trimmed.to_string();
    }
    format!("Approved plan for {project}")
}

pub(crate) fn derive_plan_thread_key(
    explicit_key: Option<&str>,
    title: &str,
    project: &str,
) -> String {
    let candidate = explicit_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(title);
    let sanitized = platform::sanitize_service_fragment(candidate)
        .trim_matches('-')
        .to_ascii_lowercase();
    if sanitized.is_empty() {
        format!(
            "approved-plan-{}",
            platform::sanitize_service_fragment(project)
                .trim_matches('-')
                .to_ascii_lowercase()
        )
    } else {
        sanitized
    }
}

pub(crate) fn parse_plan_checkboxes(markdown: &str) -> Vec<PlanChecklistItem> {
    markdown
        .lines()
        .filter_map(parse_plan_checkbox_line)
        .collect()
}

fn parse_plan_checkbox_line(line: &str) -> Option<PlanChecklistItem> {
    let trimmed = line.trim_start();
    let mut chars = trimmed.chars();
    let bullet = chars.next()?;
    if !matches!(bullet, '-' | '*' | '+') {
        return None;
    }
    if chars.next()? != ' ' || chars.next()? != '[' {
        return None;
    }
    let marker = chars.next()?;
    if chars.next()? != ']' || chars.next()? != ' ' {
        return None;
    }
    let checked = matches!(marker, 'x' | 'X');
    if !matches!(marker, ' ' | 'x' | 'X') {
        return None;
    }
    let text = chars.as_str().trim();
    Some(PlanChecklistItem {
        text: if text.is_empty() {
            "(empty checkbox item)".to_string()
        } else {
            text.to_string()
        },
        checked,
    })
}

pub(crate) fn ensure_checkbox_plan(items: &[PlanChecklistItem]) -> Result<()> {
    if items.is_empty() {
        anyhow::bail!(
            "approved plans must contain Markdown checkbox items like `- [ ] task` before execution starts"
        );
    }
    Ok(())
}

pub(crate) fn normalize_plan_markdown_for_hash(input: &str) -> String {
    input
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_string()
}

pub(crate) fn durable_plan_source_path(source_path: &Path, repo_root: &Path) -> Option<PathBuf> {
    let resolved_source = fs::canonicalize(source_path).ok()?;
    let resolved_repo_root = fs::canonicalize(repo_root).ok()?;
    resolved_source
        .starts_with(&resolved_repo_root)
        .then_some(resolved_source)
}
