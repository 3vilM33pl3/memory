pub(crate) use mem_record::compose::{
    PlanChecklistItem, derive_plan_thread_key, derive_plan_title, normalize_plan_markdown_for_hash,
    parse_plan_checkboxes,
};
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Result;

pub(crate) fn ensure_checkbox_plan(items: &[PlanChecklistItem]) -> Result<()> {
    if items.is_empty() {
        anyhow::bail!(
            "approved plans must contain Markdown checkbox items like `- [ ] task` before execution starts"
        );
    }
    Ok(())
}

pub(crate) fn durable_plan_source_path(source_path: &Path, repo_root: &Path) -> Option<PathBuf> {
    let resolved_source = fs::canonicalize(source_path).ok()?;
    let resolved_repo_root = fs::canonicalize(repo_root).ok()?;
    resolved_source
        .starts_with(&resolved_repo_root)
        .then_some(resolved_source)
}
