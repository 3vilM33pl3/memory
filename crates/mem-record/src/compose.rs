// SPDX-License-Identifier: AGPL-3.0-or-later

//! Canonical memory composition: the text, summaries, idempotency keys, and
//! plan derivations that define what a well-formed memory looks like. Owned
//! by the record crate so the service, MCP server, and CLI share ONE
//! implementation - a memory's identity must not depend on which client
//! wrote it.

use sha2::{Digest, Sha256};

/// Keep service-name fragments filesystem- and identifier-safe.
fn sanitize_fragment(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => ch,
            _ => '-',
        })
        .collect::<String>()
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

#[derive(Debug, Clone, serde::Serialize)]
pub struct PlanChecklistItem {
    pub text: String,
    pub checked: bool,
}

pub fn build_task_start_idempotency_key(
    project: &str,
    thread_key: &str,
    title: &str,
    prompt: &str,
    git_head: Option<&str>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"task-start");
    hasher.update(project.as_bytes());
    hasher.update(thread_key.as_bytes());
    hasher.update(title.trim().as_bytes());
    hasher.update(prompt.trim().as_bytes());
    if let Some(git_head) = git_head.map(str::trim).filter(|value| !value.is_empty()) {
        hasher.update(git_head.as_bytes());
    }
    format!("task-start:{:x}", hasher.finalize())
}

pub fn build_task_start_canonical_text(
    project: &str,
    title: &str,
    prompt: &str,
    thread_key: &str,
    git_head: Option<&str>,
) -> String {
    let mut lines = vec![
        format!("# Task: {}", title.trim()),
        String::new(),
        "Status: started".to_string(),
        format!("Project: {project}"),
        format!("Thread: {thread_key}"),
    ];
    if let Some(git_head) = git_head.map(str::trim).filter(|value| !value.is_empty()) {
        lines.push(format!("Git head: {git_head}"));
    }
    lines.extend([
        String::new(),
        "Original user request:".to_string(),
        prompt.trim().to_string(),
    ]);
    lines.join("\n")
}

pub fn build_plan_execution_idempotency_key(
    project: &str,
    thread_key: &str,
    plan_markdown: &str,
    git_head: Option<&str>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"plan-execution");
    hasher.update(project.as_bytes());
    hasher.update(thread_key.as_bytes());
    hasher.update(normalize_plan_markdown_for_hash(plan_markdown).as_bytes());
    if let Some(git_head) = git_head.map(str::trim).filter(|value| !value.is_empty()) {
        hasher.update(git_head.as_bytes());
    }
    format!("plan-execution:{:x}", hasher.finalize())
}

pub fn normalize_sentence_fragment(input: &str) -> String {
    let mut value = input.trim().replace('\n', " ");
    while value.contains("  ") {
        value = value.replace("  ", " ");
    }
    if value.is_empty() {
        return value;
    }
    if !value.ends_with('.') {
        value.push('.');
    }
    value
}

pub fn is_refactor_completion(
    title: &str,
    summary: &str,
    prompt: &str,
    notes: &[String],
    completed_items: &[String],
) -> bool {
    let mut haystack = format!(
        "{} {} {}",
        title.to_ascii_lowercase(),
        summary.to_ascii_lowercase(),
        prompt.to_ascii_lowercase()
    );
    for note in notes {
        haystack.push(' ');
        haystack.push_str(&note.to_ascii_lowercase());
    }
    for item in completed_items {
        haystack.push(' ');
        haystack.push_str(&item.to_ascii_lowercase());
    }

    let has_refactor_cue = [
        "refactor",
        "refactored",
        "refactoring",
        "restructure",
        "restructured",
        "reorganize",
        "reorganized",
        "rename",
        "renamed",
        "move",
        "moved",
        "extract helper",
        "extracted helper",
        "cleanup",
        "clean up",
        "mechanical change",
    ]
    .iter()
    .any(|cue| haystack.contains(cue));
    if !has_refactor_cue {
        return false;
    }

    let behavior_preserving = [
        "no functional change",
        "no behavior change",
        "without functional change",
        "without behavior change",
        "behavior preserving",
        "behaviour preserving",
        "behavior-preserving",
        "behaviour-preserving",
        "pure refactor",
    ]
    .iter()
    .any(|cue| haystack.contains(cue));

    let functional_change = [
        "fix",
        "fixed",
        "bug",
        "feature",
        "implemented",
        "add support",
        "added support",
        "new behavior",
        "new behaviour",
    ]
    .iter()
    .any(|cue| haystack.contains(cue));

    behavior_preserving || !functional_change
}

pub fn build_implementation_canonical_text(
    title: &str,
    summary: &str,
    implemented_items: &[String],
    notes: &[String],
) -> String {
    let mut sections = vec![normalize_sentence_fragment(summary)];
    if !title.trim().is_empty() {
        sections.push(format!("Plan: {}.", title.trim()));
    }
    if !implemented_items.is_empty() {
        sections.push(format!(
            "Implemented items:\n{}",
            implemented_items
                .iter()
                .map(|item| format!("- {}", item.trim()))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    let cleaned_notes = notes
        .iter()
        .map(|note| note.trim())
        .filter(|note| !note.is_empty())
        .collect::<Vec<_>>();
    if !cleaned_notes.is_empty() {
        sections.push(format!(
            "Implementation notes:\n{}",
            cleaned_notes
                .iter()
                .map(|note| format!("- {note}"))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    sections.join("\n\n")
}

pub fn derive_summary(project: &str, files_changed: &[String]) -> String {
    if files_changed.is_empty() {
        format!("Captured meaningful work for project {project}.")
    } else {
        let preview = files_changed
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        format!("Updated files in project {project}: {preview}.")
    }
}

pub fn derive_plan_title(
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

pub fn derive_plan_thread_key(explicit_key: Option<&str>, title: &str, project: &str) -> String {
    let candidate = explicit_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(title);
    let sanitized = sanitize_fragment(candidate)
        .trim_matches('-')
        .to_ascii_lowercase();
    if sanitized.is_empty() {
        format!(
            "approved-plan-{}",
            sanitize_fragment(project)
                .trim_matches('-')
                .to_ascii_lowercase()
        )
    } else {
        sanitized
    }
}

pub fn parse_plan_checkboxes(markdown: &str) -> Vec<PlanChecklistItem> {
    markdown
        .lines()
        .filter_map(parse_plan_checkbox_line)
        .collect()
}

pub fn normalize_plan_markdown_for_hash(input: &str) -> String {
    input
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_string()
}
