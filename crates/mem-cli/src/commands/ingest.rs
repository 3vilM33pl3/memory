use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use mem_api::{
    AppConfig, CaptureCandidateInput, CaptureCandidateSourceInput, CaptureTaskRequest,
    CurateRequest, MemoryType, SourceKind,
};
use reqwest::Client;

use crate::{
    commands::{
        output::{parse_memory_type, service_url, write_headers},
        runtime::IngestArgs,
    },
    writer_identity::resolve_writer_identity,
};

/// Extensions treated as ingestible text documents by default.
const DEFAULT_EXTENSIONS: &[&str] = &["md", "markdown", "txt", "rst", "org", "adoc"];
/// Files larger than this are skipped: a memory is a distillation surface,
/// not a blob store.
const MAX_FILE_BYTES: u64 = 256 * 1024;
/// Canonical text is capped to keep memories citable and curation cheap.
const MAX_CANONICAL_CHARS: usize = 2_000;
/// Candidates per capture request; large corpora are sent in batches.
const BATCH_SIZE: usize = 20;

pub(super) async fn handle(
    args: IngestArgs,
    client: Client,
    config: AppConfig,
    cli_writer_id: Option<String>,
) -> Result<()> {
    let root = args
        .path
        .canonicalize()
        .with_context(|| format!("resolve ingest path {}", args.path.display()))?;

    let extensions: BTreeSet<String> = if args.extensions.is_empty() {
        DEFAULT_EXTENSIONS
            .iter()
            .map(|ext| ext.to_string())
            .collect()
    } else {
        args.extensions
            .iter()
            .map(|ext| ext.trim_start_matches('.').to_ascii_lowercase())
            .collect()
    };

    let mut files = Vec::new();
    let mut skipped_large = 0usize;
    collect_files(&root, &extensions, &mut files, &mut skipped_large)?;
    files.sort();
    let truncated = files.len() > args.max_files;
    files.truncate(args.max_files);

    if files.is_empty() {
        println!(
            "No ingestible documents found under {} (extensions: {}).",
            root.display(),
            extensions.iter().cloned().collect::<Vec<_>>().join(", ")
        );
        return Ok(());
    }

    let memory_type = match args.memory_type.clone() {
        Some(value) => parse_memory_type(value)?,
        None => MemoryType::Reference,
    };

    let mut candidates = Vec::with_capacity(files.len());
    for file in &files {
        if let Some(candidate) = candidate_for_file(&root, file, &memory_type, &args.tags)? {
            candidates.push(candidate);
        }
    }

    if args.dry_run {
        println!(
            "Would ingest {} document(s) into project '{}' as type {}:",
            candidates.len(),
            args.project,
            memory_type
        );
        for candidate in &candidates {
            println!("  {}", candidate.summary);
        }
        if truncated {
            println!("  ... stopped at --max-files {}", args.max_files);
        }
        if skipped_large > 0 {
            println!("  (skipped {skipped_large} file(s) over 256 KB)");
        }
        return Ok(());
    }

    let writer = resolve_writer_identity(&config, cli_writer_id.as_deref())?;
    let total = candidates.len();
    let mut sent = 0usize;
    for batch in candidates.chunks(BATCH_SIZE) {
        let request = CaptureTaskRequest {
            project: args.project.clone(),
            task_title: format!("Ingest documents from {}", root.display()),
            user_prompt: format!(
                "Ingest {} document(s) from {} as durable {} memories.",
                total,
                root.display(),
                memory_type
            ),
            writer_id: writer.id.clone(),
            writer_name: writer.name.clone(),
            agent_summary: format!(
                "Ingested {} of {} document(s) from {}.",
                sent + batch.len(),
                total,
                root.display()
            ),
            files_changed: Vec::new(),
            git_diff_summary: None,
            tests: Vec::new(),
            notes: Vec::new(),
            structured_candidates: batch.to_vec(),
            command_output: None,
            idempotency_key: None,
            dry_run: false,
        };
        let response = client
            .post(service_url(&config, "/v1/capture/task"))
            .headers(write_headers(&config)?)
            .json(&request)
            .send()
            .await
            .context("send ingest capture request")?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("ingest capture failed ({status}): {body}");
        }
        let response: mem_api::CaptureTaskResponse = response
            .json()
            .await
            .context("parse ingest capture response")?;
        sent += batch.len();
        println!("Captured {sent}/{total} document(s)...");

        // Curate each batch's own capture (bounded — see 3VI-824).
        let curate = client
            .post(service_url(&config, "/v1/curate"))
            .headers(write_headers(&config)?)
            .json(&CurateRequest {
                project: args.project.clone(),
                batch_size: None,
                raw_capture_id: Some(response.raw_capture_id),
                replacement_policy: None,
                dry_run: false,
            })
            .send()
            .await
            .context("send ingest curate request")?;
        if !curate.status().is_success() {
            let status = curate.status();
            let body = curate.text().await.unwrap_or_default();
            anyhow::bail!("ingest curation failed ({status}): {body}");
        }
    }

    println!(
        "Ingested {total} document(s) into project '{}'.",
        args.project
    );
    if truncated {
        println!(
            "Stopped at --max-files {}; rerun with a higher cap for the rest.",
            args.max_files
        );
    }
    if skipped_large > 0 {
        println!("Skipped {skipped_large} file(s) over 256 KB.");
    }
    println!(
        "Try: memory query --project {} --question \"...\"",
        args.project
    );
    Ok(())
}

fn collect_files(
    dir: &Path,
    extensions: &BTreeSet<String>,
    files: &mut Vec<PathBuf>,
    skipped_large: &mut usize,
) -> Result<()> {
    if dir.is_file() {
        files.push(dir.to_path_buf());
        return Ok(());
    }
    for entry in fs::read_dir(dir).with_context(|| format!("read directory {}", dir.display()))? {
        let path = entry?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with('.') || name == "node_modules" || name == "target" {
            continue;
        }
        if path.is_dir() {
            collect_files(&path, extensions, files, skipped_large)?;
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if !extensions.contains(&ext.to_ascii_lowercase()) {
            continue;
        }
        if fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > MAX_FILE_BYTES {
            *skipped_large += 1;
            continue;
        }
        files.push(path);
    }
    Ok(())
}

/// One deterministic candidate per document: the title (first heading or the
/// file name) as the summary, a capped excerpt as the canonical text, and the
/// file as verifiable provenance. No LLM involved; curation still gates it.
fn candidate_for_file(
    root: &Path,
    file: &Path,
    memory_type: &MemoryType,
    extra_tags: &[String],
) -> Result<Option<CaptureCandidateInput>> {
    let content = match fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(None), // not valid UTF-8; skip binaries quietly
    };
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let relative = file
        .strip_prefix(root)
        .unwrap_or(file)
        .display()
        .to_string();
    let title = trimmed
        .lines()
        .find_map(|line| {
            let line = line.trim().trim_start_matches('#').trim();
            (!line.is_empty()).then(|| line.to_string())
        })
        .unwrap_or_else(|| relative.clone());

    let mut canonical: String = trimmed.chars().take(MAX_CANONICAL_CHARS).collect();
    if trimmed.chars().count() > MAX_CANONICAL_CHARS {
        canonical.push_str("\n[truncated: full text in the source document]");
    }

    let mut tags = vec!["ingested".to_string()];
    tags.extend(extra_tags.iter().cloned());

    Ok(Some(CaptureCandidateInput {
        canonical_text: canonical,
        summary: format!("{title} ({relative})"),
        memory_type: memory_type.clone(),
        confidence: 0.8,
        importance: 3,
        tags,
        sources: vec![CaptureCandidateSourceInput {
            source_kind: SourceKind::File,
            file_path: Some(file.display().to_string()),
            symbol_name: None,
            symbol_kind: None,
            excerpt: Some(title),
        }],
    }))
}
