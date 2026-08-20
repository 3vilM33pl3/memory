// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{resume, scan};
use std::path::Path;

use anyhow::Result;
use mem_config::AppConfig;
use mem_record::{
    ActivityListResponse, CodeGraphStatusResponse, CommitDetailResponse, CommitSyncResponse,
    ProjectCommitsResponse, ProjectMemoryImportPreview, ProjectMemoryImportResponse,
    ResumeResponse, UpToSpeedResponse,
};
use reqwest::header::HeaderMap;

use crate::commands::memory_ops::PlanExecutionFinishReport;

pub(crate) fn print_activities_response(response: &ActivityListResponse) {
    println!(
        "Activities for {} ({} returned)\n",
        response.project, response.total_returned
    );
    for event in &response.items {
        println!(
            "{} | {:<14} | {:>8} tok | {:>6} ms | {}{}",
            event.recorded_at.format("%Y-%m-%d %H:%M:%S UTC"),
            activity_kind_text(&event.kind),
            event
                .token_usage
                .as_ref()
                .map(|usage| usage.total_tokens.to_string())
                .unwrap_or_else(|| "-".to_string()),
            event
                .duration_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string()),
            event.summary,
            activity_graph_suffix(event)
        );
    }
}

pub(crate) fn activity_graph_suffix(event: &mem_record::ActivityEvent) -> String {
    match &event.details {
        Some(mem_record::ActivityDetails::Query {
            graph_status: Some(status),
            graph_candidates,
            graph_augmented_candidates,
            graph_duration_ms,
            graph_connection_count,
            ..
        }) => format!(
            " | graph {status}: {graph_candidates} candidates, {graph_augmented_candidates} augmented, {graph_connection_count} connections, {graph_duration_ms} ms"
        ),
        _ => String::new(),
    }
}

pub(crate) fn print_up_to_speed_response(response: &UpToSpeedResponse) {
    println!("{}", response.briefing);
    println!();
    println!(
        "Support data: {} activities | {} useful memories | {} token-tracked actions",
        response.recent_activities.len(),
        response.useful_memories.len(),
        response.token_usage.action_count
    );
    if !response.warnings.is_empty() {
        println!("\nWarnings:");
        for warning in &response.warnings {
            println!("- {warning}");
        }
    }
}

pub(crate) fn activity_kind_text(kind: &mem_record::ActivityKind) -> &'static str {
    match kind {
        mem_record::ActivityKind::Checkpoint => "checkpoint",
        mem_record::ActivityKind::Scan => "scan",
        mem_record::ActivityKind::Plan => "plan",
        mem_record::ActivityKind::CommitSync => "commit_sync",
        mem_record::ActivityKind::BundleExport => "bundle_export",
        mem_record::ActivityKind::BundleImport => "bundle_import",
        mem_record::ActivityKind::GraphExtract => "graph_extract",
        mem_record::ActivityKind::Query => "query",
        mem_record::ActivityKind::QueryError => "query_error",
        mem_record::ActivityKind::WatcherHealth => "watcher_health",
        mem_record::ActivityKind::MemoryReplacement => "replacement",
        mem_record::ActivityKind::CaptureTask => "capture",
        mem_record::ActivityKind::Curate => "curate",
        mem_record::ActivityKind::Reindex => "reindex",
        mem_record::ActivityKind::Reembed => "reembed",
        mem_record::ActivityKind::Archive => "archive",
        mem_record::ActivityKind::DeleteMemory => "delete",
        mem_record::ActivityKind::Briefing => "briefing",
        mem_record::ActivityKind::Diagnostic => "diagnostic",
        mem_record::ActivityKind::LlmAudit => "llm_audit",
        mem_record::ActivityKind::MemoryValidation => "memory_validation",
        mem_record::ActivityKind::LoopRunStarted => "loop_run_started",
        mem_record::ActivityKind::LoopRunFinished => "loop_run_finished",
        mem_record::ActivityKind::LoopRunFailed => "loop_run_failed",
        mem_record::ActivityKind::LoopSettingChanged => "loop_setting_changed",
        mem_record::ActivityKind::ProposalCreated => "proposal_created",
        mem_record::ActivityKind::ProposalDecided => "proposal_decided",
        mem_record::ActivityKind::ProposalApplied => "proposal_applied",
        mem_record::ActivityKind::Consolidation => "consolidation",
        mem_record::ActivityKind::ProvenanceCheck => "provenance_check",
        mem_record::ActivityKind::WorkspaceChanged => "workspace_changed",
        mem_record::ActivityKind::TriggerReceived => "trigger_received",
        mem_record::ActivityKind::AuthEvent => "auth_event",
    }
}

pub(crate) fn print_bundle_import_preview(preview: &ProjectMemoryImportPreview) {
    println!("Bundle: {}", preview.bundle_id);
    println!("Source project: {}", preview.source_project);
    println!("Target project: {}", preview.target_project);
    println!(
        "Memories: {} total | {} new | {} unchanged | {} replacing",
        preview.memory_count, preview.new_count, preview.unchanged_count, preview.replacing_count
    );
    println!("Warnings: {}", preview.warning_count);
    println!("\n{}", preview.summary_markdown);
}

pub(crate) fn print_bundle_import_response(response: &ProjectMemoryImportResponse) {
    println!(
        "Imported bundle {} into {}",
        response.bundle_id, response.target_project
    );
    println!(
        "Imported: {} | Replaced: {} | Skipped: {} | Relations: {}",
        response.imported_count,
        response.replaced_count,
        response.skipped_count,
        response.relation_count
    );
}

pub(crate) fn print_resume_response(response: &ResumeResponse) {
    println!("Resume for {}\n", response.project);

    if let Some(checkpoint) = &response.checkpoint {
        println!(
            "Checkpoint: {}",
            checkpoint.marked_at.format("%Y-%m-%d %H:%M UTC")
        );
        if let Some(note) = &checkpoint.note {
            println!("Checkpoint note: {note}");
        }
        println!(
            "Checkpoint age: {} hour(s)\n",
            resume::checkpoint_age_hours(checkpoint, response.generated_at)
        );
    }

    if let Some(current_thread) = &response.current_thread {
        println!("Current thread:\n- {}\n", current_thread);
    }

    if let Some(action) = &response.primary_next_step {
        println!("Next step:");
        println!("- {}: {}", action.title, action.rationale);
        if let Some(command_hint) = &action.command_hint {
            println!("  {}", command_hint);
        }
        println!();
    }

    if !response.change_summary.is_empty() {
        println!("What changed:");
        for item in &response.change_summary {
            println!("- {item}");
        }
        println!();
    }

    if !response.attention_items.is_empty() {
        println!("Needs attention:");
        for item in &response.attention_items {
            println!("- {item}");
        }
        println!();
    }

    if !response.context_items.is_empty() {
        println!("Keep in mind:");
        for item in &response.context_items {
            println!("- [{}] {}", item.memory_type, item.summary);
        }
        println!();
    }

    if !response.secondary_next_steps.is_empty() {
        println!("Other useful follow-ups:");
        for action in &response.secondary_next_steps {
            println!("- {}: {}", action.title, action.rationale);
            if let Some(command_hint) = &action.command_hint {
                println!("  {}", command_hint);
            }
        }
        println!();
    }

    println!(
        "Support data: {} timeline event(s) | {} commit(s) | {} changed memory entry/entries",
        response.timeline.len(),
        response.commits.len(),
        response.changed_memories.len(),
    );

    if !response.warnings.is_empty() {
        println!("\nAll warnings:");
        for warning in &response.warnings {
            println!("- {warning}");
        }
    }

    if !response.actions.is_empty() {
        println!("\nAll suggested next actions:");
        for action in &response.actions {
            println!("- {}: {}", action.title, action.rationale);
            if let Some(command_hint) = &action.command_hint {
                println!("  {}", command_hint);
            }
        }
    }

    if response.current_thread.is_none()
        && response.change_summary.is_empty()
        && response.attention_items.is_empty()
        && response.context_items.is_empty()
    {
        println!("\n{}", response.briefing);
    }
}

pub(crate) fn print_plan_execution_finish_report(report: &PlanExecutionFinishReport) {
    if report.verified_complete {
        println!(
            "Verified approved plan for `{}`\n- Thread: {}\n- Plan: {}\n- Completed: {}/{} items",
            report.project,
            report.thread_key,
            report.plan_title,
            report.completed_items,
            report.total_items
        );
    } else {
        println!(
            "Approved plan is still in progress for `{}`\n- Thread: {}\n- Plan: {}\n- Completed: {}/{} items\n- Remaining items:",
            report.project,
            report.thread_key,
            report.plan_title,
            report.completed_items,
            report.total_items
        );
        for item in &report.remaining_items {
            println!("  - {item}");
        }
    }
}

pub(crate) fn print_scan_report(report: &scan::ScanReport) {
    println!("Scan summary:\n{}\n", report.summary);
    println!(
        "Project: {} | Files: {} | Commits: {} | Candidates: {} | Written: {} | Index: {}",
        report.project,
        report.files_considered,
        report.commits_considered,
        report.candidate_count,
        if report.written { "yes" } else { "no" },
        if report.index_reused {
            "reused"
        } else {
            "rebuilt"
        }
    );
    println!(
        "Coverage: rust {} | ts/js {} | python {} | docs {} | config {} | other {}",
        report.language_coverage.rust_files,
        report.language_coverage.ts_js_files,
        report.language_coverage.python_files,
        report.language_coverage.docs_files,
        report.language_coverage.config_files,
        report.language_coverage.other_files,
    );
    println!("Index: {}", report.index_path);
    println!("Report: {}", report.report_path);
    if !report.written {
        println!(
            "Dry run: no scan report file, activity event, capture, or curate run was written."
        );
    }
    if !report.candidate_previews.is_empty() {
        println!("\nCandidates:");
        for preview in &report.candidate_previews {
            println!("- {}", preview.summary);
            println!(
                "  type={} confidence={:.2} importance={}",
                preview.memory_type, preview.confidence, preview.importance,
            );
            if !preview.provenance_preview.is_empty() {
                println!("  provenance: {}", preview.provenance_preview.join(" | "));
            }
        }
    }
    if let Some(capture_id) = &report.capture_id {
        println!("Capture: {capture_id}");
    }
    if let Some(run_id) = &report.curate_run_id {
        println!("Curate run: {run_id}");
    }
}

pub(crate) fn print_index_report(report: &scan::RepoIndexReport) {
    println!(
        "Repository index {} for {}\n",
        if report.dry_run { "preview" } else { "built" },
        report.project
    );
    println!(
        "Files: {} selected / {} tracked | Commits: {} | Evidence bundles: {}",
        report.files_indexed,
        report.tracked_files,
        report.commits_indexed,
        report.evidence_bundle_count,
    );
    println!(
        "Coverage: rust {} | ts/js {} | python {} | docs {} | config {} | other {}",
        report.language_coverage.rust_files,
        report.language_coverage.ts_js_files,
        report.language_coverage.python_files,
        report.language_coverage.docs_files,
        report.language_coverage.config_files,
        report.language_coverage.other_files,
    );
    println!(
        "Analyzer facts: symbols {} | imports {} | references {} | calls {} | test links {}",
        report.symbol_count,
        report.import_count,
        report.reference_count,
        report.call_count,
        report.test_link_count,
    );
    if !report.enabled_analyzers.is_empty() {
        println!("Enabled analyzers: {}", report.enabled_analyzers.join(", "));
    }
    for summary in &report.analyzer_summaries {
        println!(
            "- {}: seen {} | parsed {} | symbols {} | imports {} | refs {} | calls {} | tests {} | errors {}",
            summary.analyzer,
            summary.files_seen,
            summary.files_parsed,
            summary.symbol_count,
            summary.import_count,
            summary.reference_count,
            summary.call_count,
            summary.test_link_count,
            summary.errors.len(),
        );
    }
    if let Some(head) = &report.head {
        println!("HEAD: {head}");
    }
    if let Some(since) = &report.since {
        println!("Since: {since}");
    }
    println!("Index: {}", report.index_path);
    if report.dry_run {
        println!("Dry run: no index file was written.");
    }
}

pub(crate) fn print_index_status(status: &Option<scan::RepoIndexStatus>, project: &str) {
    let Some(status) = status else {
        println!("No repository index found for {project}.");
        println!("Build one with: memory repo index --project {project}");
        return;
    };
    println!("Repository index status for {}\n", status.project);
    println!(
        "Files: {} selected / {} tracked | Commits: {} | Evidence bundles: {}",
        status.files_indexed,
        status.tracked_files,
        status.commits_indexed,
        status.evidence_bundle_count,
    );
    println!(
        "Coverage: rust {} | ts/js {} | python {} | docs {} | config {} | other {}",
        status.language_coverage.rust_files,
        status.language_coverage.ts_js_files,
        status.language_coverage.python_files,
        status.language_coverage.docs_files,
        status.language_coverage.config_files,
        status.language_coverage.other_files,
    );
    println!(
        "Analyzer facts: symbols {} | imports {} | references {} | calls {} | test links {}",
        status.symbol_count,
        status.import_count,
        status.reference_count,
        status.call_count,
        status.test_link_count,
    );
    if !status.enabled_analyzers.is_empty() {
        println!("Enabled analyzers: {}", status.enabled_analyzers.join(", "));
    }
    for summary in &status.analyzer_summaries {
        println!(
            "- {}: seen {} | parsed {} | symbols {} | imports {} | refs {} | calls {} | tests {} | errors {}",
            summary.analyzer,
            summary.files_seen,
            summary.files_parsed,
            summary.symbol_count,
            summary.import_count,
            summary.reference_count,
            summary.call_count,
            summary.test_link_count,
            summary.errors.len(),
        );
    }
    if let Some(head) = &status.head {
        println!("HEAD: {head}");
    }
    if let Some(since) = &status.since {
        println!("Since: {since}");
    }
    println!("Built: {}", status.built_at);
    println!("Index: {}", status.index_path);
}

pub(crate) fn print_graph_extract_report(
    report: &mem_graph::GraphExtractionReport,
    index_path: &Path,
) {
    let mode = if report.dry_run {
        "Code graph extraction preview"
    } else if report.reused_existing_run {
        "Code graph extraction reused"
    } else {
        "Code graph extracted"
    };
    println!("{mode} for {}\n", report.project);
    println!(
        "Symbols: {} | References: {} | Resolved: {} | Unresolved: {} | Ambiguous: {}",
        report.symbol_count,
        report.reference_count,
        report.resolved_reference_count,
        report.unresolved_reference_count,
        report.ambiguous_reference_count,
    );
    println!(
        "Graph: nodes {} | edges {} | evidence {}",
        report.graph_node_count, report.graph_edge_count, report.evidence_count,
    );
    println!(
        "Analyzer: {} | Strategy: {}",
        report.analyzer_version, report.strategy_version
    );
    if let Some(head) = &report.git_head {
        println!("HEAD: {head}");
    }
    if let Some(since) = &report.since {
        println!("Since: {since}");
    }
    if let Some(run_id) = report.extraction_run_id {
        println!("Extraction run: {run_id}");
    }
    println!("Index: {}", index_path.display());
    if !report.sample_unresolved_references.is_empty() {
        println!("Sample unresolved/ambiguous references:");
        for reference in &report.sample_unresolved_references {
            println!(
                "- {}:{} {} {} ({})",
                reference.file_path,
                reference.start_line,
                reference.kind,
                reference.target_text,
                reference.resolution_status,
            );
        }
    }
    if report.dry_run {
        println!("Dry run: no database rows or index files were written.");
    }
}

pub(crate) fn print_graph_status(status: &CodeGraphStatusResponse) {
    if !status.has_graph {
        println!("No code graph extraction found for {}.", status.project);
        println!(
            "Build one with: memory graph extract --project {}",
            status.project
        );
        return;
    }
    println!("Code graph status for {}\n", status.project);
    if let Some(state) = &status.status {
        println!("Status: {state}");
    }
    if let Some(completed_at) = status.completed_at {
        println!("Completed: {completed_at}");
    }
    if let Some(run_id) = status.latest_run_id {
        println!("Extraction run: {run_id}");
    }
    println!(
        "Symbols: {} | References: {} | Resolved: {} | Unresolved: {} | Ambiguous: {}",
        status.symbol_count,
        status.reference_count,
        status.resolved_reference_count,
        status.unresolved_reference_count,
        status.ambiguous_reference_count,
    );
    println!(
        "Graph: nodes {} | edges {} | evidence {}",
        status.graph_node_count, status.graph_edge_count, status.evidence_count,
    );
    if let (Some(analyzer), Some(strategy)) = (&status.analyzer_version, &status.strategy_version) {
        println!("Analyzer: {analyzer} | Strategy: {strategy}");
    }
    if let Some(head) = &status.git_head {
        println!("HEAD: {head}");
    }
    if let Some(since) = &status.since {
        println!("Since: {since}");
    }
    if let Some(repo_root) = &status.repo_root {
        println!("Repo: {repo_root}");
    }
}

pub(crate) fn print_commit_sync_response(response: &CommitSyncResponse) {
    println!(
        "{}: {} imported, {} updated, {} received.",
        if response.dry_run {
            "Commit sync dry run"
        } else {
            "Commit sync complete"
        },
        response.imported_count,
        response.updated_count,
        response.total_received
    );
    if let Some(newest) = &response.newest_commit {
        println!("Newest commit: {newest}");
    }
    if let Some(oldest) = &response.oldest_commit {
        println!("Oldest commit: {oldest}");
    }
}

pub(crate) fn print_project_commits(response: &ProjectCommitsResponse) {
    println!(
        "Project {} commit history (showing {} / {}):",
        response.project,
        response.items.len(),
        response.total
    );
    for commit in &response.items {
        println!(
            "- {} {} ({})",
            commit.short_hash,
            commit.subject,
            commit.committed_at.format("%Y-%m-%d %H:%M UTC")
        );
        if let Some(author) = &commit.author_name {
            println!("  author: {author}");
        }
        if !commit.changed_paths.is_empty() {
            println!("  files: {}", commit.changed_paths.join(", "));
        }
    }
}

pub(crate) fn print_commit_detail(response: &CommitDetailResponse) {
    let commit = &response.commit;
    println!("Project: {}", response.project);
    println!("Commit: {} ({})", commit.hash, commit.short_hash);
    println!("When: {}", commit.committed_at.format("%Y-%m-%d %H:%M UTC"));
    if let Some(author) = &commit.author_name {
        if let Some(email) = &commit.author_email {
            println!("Author: {author} <{email}>");
        } else {
            println!("Author: {author}");
        }
    }
    println!("Subject: {}", commit.subject);
    if !commit.body.trim().is_empty() {
        println!("\nBody:\n{}", commit.body);
    }
    if !commit.parent_hashes.is_empty() {
        println!("\nParents: {}", commit.parent_hashes.join(", "));
    }
    if !commit.changed_paths.is_empty() {
        println!("\nChanged paths:");
        for path in &commit.changed_paths {
            println!("- {path}");
        }
    }
}

pub(crate) fn parse_memory_type(input: String) -> Result<mem_record::MemoryType> {
    match input.as_str() {
        "architecture" => Ok(mem_record::MemoryType::Architecture),
        "convention" => Ok(mem_record::MemoryType::Convention),
        "decision" => Ok(mem_record::MemoryType::Decision),
        "incident" => Ok(mem_record::MemoryType::Incident),
        "debugging" => Ok(mem_record::MemoryType::Debugging),
        "environment" => Ok(mem_record::MemoryType::Environment),
        "domain_fact" => Ok(mem_record::MemoryType::DomainFact),
        "documentation" => Ok(mem_record::MemoryType::Documentation),
        "task" => Ok(mem_record::MemoryType::Task),
        "plan" => Ok(mem_record::MemoryType::Plan),
        "implementation" => Ok(mem_record::MemoryType::Implementation),
        "refactor" => Ok(mem_record::MemoryType::Refactor),
        _ => anyhow::bail!("unknown memory type: {input}"),
    }
}

pub(crate) fn write_headers(config: &AppConfig) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    let token = client_api_token(config);
    headers.insert("x-api-token", token.parse()?);
    Ok(headers)
}

pub(crate) fn client_api_token(config: &AppConfig) -> String {
    std::env::var("MEMORY_LAYER_CLIENT_TOKEN")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| config.service.api_token.clone())
}

pub(crate) fn service_url(config: &AppConfig, path: &str) -> String {
    format!("http://{}{}", config.service.bind_addr, path)
}
