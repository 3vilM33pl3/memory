use anyhow::{Context, Result};
use mem_api::{AppConfig, PlanActivityAction};
use reqwest::Client;
use std::env;

use crate::commands::{
    api::ApiClient,
    init_support::repo_replacement_policy,
    memory_ops::{
        ImplementationMemoryPreview, ImplementationMemoryResult,
        build_finish_execution_implementation_request, build_plan_activity_request,
        build_plan_execution_finish_report, build_plan_execution_request, build_task_start_request,
        derive_finish_execution_implementation_summary, load_plan_content,
        plan_detail_from_markdown, preview_checkpoint, repo_git_head,
        resolve_active_plan_selection, resolve_project_slug, save_checkpoint_with_activity,
        verify_task_start_memory,
    },
    output::print_plan_execution_finish_report,
    runtime::{CheckpointArgs, CheckpointCommand},
    skill_support::resolve_repo_root,
};
use crate::plan_execution::{
    derive_plan_thread_key, derive_plan_title, ensure_checkbox_plan, parse_plan_checkboxes,
};
use crate::resume as checkpoint_store;
use crate::writer_identity::resolve_writer_identity;

pub(super) async fn handle(
    args: CheckpointArgs,
    client: Client,
    config: AppConfig,
    cli_writer_id: Option<String>,
) -> Result<()> {
    let cwd = env::current_dir().context("read current directory")?;
    let repo_root = resolve_repo_root(&cwd)?;
    match args.command {
        CheckpointCommand::Save(args) => {
            let project = resolve_project_slug(args.project, &cwd)?;
            if args.dry_run {
                let (checkpoint, path) = preview_checkpoint(&project, &repo_root, args.note)?;
                if args.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "checkpoint": {
                                "path": path.display().to_string(),
                                "data": checkpoint,
                            },
                            "dry_run": true,
                        }))?
                    );
                } else {
                    println!(
                        "Would save checkpoint for `{project}` to {}\n\n{}",
                        path.display(),
                        checkpoint_store::format_checkpoint(&checkpoint)
                    );
                }
            } else {
                let api = ApiClient::new(client.clone(), config.clone());
                let (checkpoint, path) =
                    save_checkpoint_with_activity(&api, &project, &repo_root, args.note).await?;
                if args.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "checkpoint": {
                                "path": path.display().to_string(),
                                "data": checkpoint,
                            },
                            "dry_run": false,
                        }))?
                    );
                } else {
                    println!(
                        "Saved checkpoint for `{project}` to {}\n\n{}",
                        path.display(),
                        checkpoint_store::format_checkpoint(&checkpoint)
                    );
                }
            }
        }
        CheckpointCommand::Show(args) => {
            let project = resolve_project_slug(args.project, &cwd)?;
            if let Some(checkpoint) = checkpoint_store::load_checkpoint(&project, &repo_root)? {
                println!("{}", checkpoint_store::format_checkpoint(&checkpoint));
            } else {
                println!("No checkpoint stored for `{project}`.");
            }
        }
        CheckpointCommand::StartExecution(args) => {
            let project = resolve_project_slug(args.project, &cwd)?;
            let api = ApiClient::new(client.clone(), config.clone());
            let (plan_markdown, source_path) =
                load_plan_content(args.plan_file.as_deref(), args.plan_stdin)?;
            let plan_items = parse_plan_checkboxes(&plan_markdown);
            ensure_checkbox_plan(&plan_items)?;
            let note = args
                .note
                .unwrap_or_else(|| "Plan approved; starting implementation".to_string());
            let title = derive_plan_title(args.title.as_deref(), &plan_markdown, &project);
            let thread_key = derive_plan_thread_key(args.thread_key.as_deref(), &title, &project);
            let (checkpoint, path) = if args.dry_run {
                preview_checkpoint(&project, &repo_root, Some(note))?
            } else {
                save_checkpoint_with_activity(&api, &project, &repo_root, Some(note)).await?
            };
            let writer = resolve_writer_identity(&config, cli_writer_id.as_deref())?;
            let mut request = build_plan_execution_request(
                &project,
                &writer,
                &title,
                &thread_key,
                &plan_markdown,
                source_path.as_deref(),
                &repo_root,
                repo_git_head(&repo_root).as_deref(),
            );
            request.dry_run = args.dry_run;
            let capture = match api.capture_task(&request).await {
                Ok(capture) => capture,
                Err(error) => {
                    if args.dry_run {
                        eprintln!(
                            "Would save checkpoint for `{project}` to {}\n\n{}",
                            path.display(),
                            checkpoint_store::format_checkpoint(&checkpoint)
                        );
                    } else {
                        eprintln!(
                            "Saved checkpoint for `{project}` to {}\n\n{}",
                            path.display(),
                            checkpoint_store::format_checkpoint(&checkpoint)
                        );
                    }
                    return Err(error.context("checkpoint saved, but approved plan capture failed"));
                }
            };
            let curate = match api
                .curate(&project, repo_replacement_policy(&repo_root), args.dry_run)
                .await
            {
                Ok(curate) => curate,
                Err(error) => {
                    if args.dry_run {
                        eprintln!(
                            "Would save checkpoint for `{project}` to {}\n\n{}",
                            path.display(),
                            checkpoint_store::format_checkpoint(&checkpoint)
                        );
                    } else {
                        eprintln!(
                            "Saved checkpoint for `{project}` to {}\n\n{}",
                            path.display(),
                            checkpoint_store::format_checkpoint(&checkpoint)
                        );
                    }
                    return Err(
                        error.context("checkpoint saved and plan captured, but curation failed")
                    );
                }
            };
            let start_request = build_plan_activity_request(
                &project,
                PlanActivityAction::Started,
                &title,
                &thread_key,
                plan_items.len(),
                plan_items.iter().filter(|item| item.checked).count(),
                plan_items
                    .iter()
                    .filter(|item| !item.checked)
                    .map(|item| item.text.clone())
                    .collect(),
                source_path.as_ref().map(|path| path.display().to_string()),
            );
            if !args.dry_run
                && let Err(error) = api.log_plan_activity(&start_request).await
            {
                eprintln!("warning: failed to log plan activity for `{project}`: {error}");
            }
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "checkpoint": {
                        "path": path.display().to_string(),
                        "data": checkpoint,
                    },
                    "plan": {
                        "title": title,
                        "thread_key": thread_key,
                        "total_items": plan_items.len(),
                    },
                    "capture": capture,
                    "curate": curate,
                    "dry_run": args.dry_run,
                }))?
            );
        }
        CheckpointCommand::StartTask(args) => {
            let project = resolve_project_slug(args.project, &cwd)?;
            let api = ApiClient::new(client.clone(), config.clone());
            let title = args.title.trim();
            let prompt = args.prompt.trim();
            if title.is_empty() {
                anyhow::bail!("--title must be non-empty");
            }
            if prompt.is_empty() {
                anyhow::bail!("--prompt must be non-empty");
            }
            let thread_key = derive_plan_thread_key(args.thread_key.as_deref(), title, &project);
            let note = format!("Direct task started: {title}");
            let (checkpoint, path) = if args.dry_run {
                preview_checkpoint(&project, &repo_root, Some(note))?
            } else {
                save_checkpoint_with_activity(&api, &project, &repo_root, Some(note)).await?
            };
            let writer = resolve_writer_identity(&config, cli_writer_id.as_deref())?;
            let mut request = build_task_start_request(
                &project,
                &writer,
                title,
                prompt,
                &thread_key,
                repo_git_head(&repo_root).as_deref(),
            );
            request.dry_run = args.dry_run;
            let capture = api
                .capture_task(&request)
                .await
                .context("capture direct task start")?;
            let curate = api
                .curate_capture(
                    &project,
                    capture.raw_capture_id,
                    repo_replacement_policy(&repo_root),
                    args.dry_run,
                )
                .await
                .context("curate direct task start")?;
            let task_memory = if args.dry_run {
                None
            } else {
                Some(
                    verify_task_start_memory(&api, &project, &thread_key)
                        .await
                        .context("verify direct task memory was created")?,
                )
            };
            let report = serde_json::json!({
                "checkpoint": {
                    "path": path.display().to_string(),
                    "data": checkpoint,
                },
                "task": {
                    "title": title,
                    "thread_key": thread_key,
                    "prompt": prompt,
                },
                "capture": capture,
                "curate": curate,
                "task_memory": task_memory,
                "dry_run": args.dry_run,
            });
            if args.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("Task execution started.");
                println!("Checkpoint: {}", path.display());
                println!("Task: {title} ({thread_key})");
                if args.dry_run {
                    println!("Dry run: true");
                }
            }
        }
        CheckpointCommand::FinishExecution(args) => {
            let project = resolve_project_slug(args.project, &cwd)?;
            let api = ApiClient::new(client.clone(), config.clone());
            let selection =
                resolve_active_plan_selection(&api, &project, args.thread_key.as_deref()).await?;
            let mut synced_plan = false;
            let mut synced_source_path = None;

            let detail = if args.plan_file.is_some() || args.plan_stdin {
                let (plan_markdown, source_path) =
                    load_plan_content(args.plan_file.as_deref(), args.plan_stdin)?;
                if args.dry_run {
                    synced_plan = true;
                    synced_source_path =
                        source_path.as_ref().map(|path| path.display().to_string());
                    plan_detail_from_markdown(&selection, &plan_markdown, selection.memory_id)?
                } else {
                    let plan_items = parse_plan_checkboxes(&plan_markdown);
                    ensure_checkbox_plan(&plan_items)?;
                    let writer = resolve_writer_identity(&config, cli_writer_id.as_deref())?;
                    let mut request = build_plan_execution_request(
                        &project,
                        &writer,
                        &selection.title,
                        &selection.thread_key,
                        &plan_markdown,
                        source_path.as_deref(),
                        &repo_root,
                        repo_git_head(&repo_root).as_deref(),
                    );
                    request.dry_run = false;
                    api.capture_task(&request)
                        .await
                        .context("sync updated plan before finish verification")?;
                    api.curate(&project, repo_replacement_policy(&repo_root), false)
                        .await
                        .context("curate updated plan before finish verification")?;
                    synced_plan = true;
                    synced_source_path =
                        source_path.as_ref().map(|path| path.display().to_string());
                    let refreshed = resolve_active_plan_selection(
                        &api,
                        &project,
                        Some(selection.thread_key.as_str()),
                    )
                    .await?;
                    api.memory_detail(&refreshed.memory_id.to_string())
                        .await
                        .context("load refreshed active plan")?
                }
            } else {
                api.memory_detail(&selection.memory_id.to_string())
                    .await
                    .context("load active plan")?
            };

            let report = build_plan_execution_finish_report(&project, &detail)?;
            if synced_plan && !args.dry_run {
                let sync_request = build_plan_activity_request(
                    &project,
                    PlanActivityAction::Synced,
                    &report.plan_title,
                    &report.thread_key,
                    report.total_items,
                    report.completed_items,
                    report.remaining_items.clone(),
                    synced_source_path,
                );
                if let Err(error) = api.log_plan_activity(&sync_request).await {
                    eprintln!("warning: failed to log plan activity for `{project}`: {error}");
                }
            }
            let implementation = if report.verified_complete {
                let writer = resolve_writer_identity(&config, cli_writer_id.as_deref())?;
                let summary = derive_finish_execution_implementation_summary(
                    args.implementation_summary.as_deref(),
                    &report,
                );
                let mut request = build_finish_execution_implementation_request(
                    &project,
                    &writer,
                    &report,
                    &summary,
                    &args.implementation_notes,
                    repo_git_head(&repo_root).as_deref(),
                );
                request.dry_run = args.dry_run;
                let preview = request.structured_candidates.first().map(|candidate| {
                    ImplementationMemoryPreview {
                        summary: candidate.summary.clone(),
                        memory_type: candidate.memory_type.clone(),
                        tags: candidate.tags.clone(),
                        canonical_text: candidate.canonical_text.clone(),
                    }
                });
                if args.dry_run {
                    Some(ImplementationMemoryResult {
                        recorded: false,
                        summary,
                        preview,
                        capture: None,
                        curate: None,
                    })
                } else {
                    let capture = api.capture_task(&request).await.with_context(
                        || "plan verification succeeded, but implementation capture failed",
                    )?;
                    let curate = api
                        .curate(&project, repo_replacement_policy(&repo_root), false)
                        .await
                        .with_context(|| {
                            "plan verification succeeded and implementation was captured, but curation failed"
                        })?;
                    Some(ImplementationMemoryResult {
                        recorded: true,
                        summary,
                        preview,
                        capture: Some(capture),
                        curate: Some(curate),
                    })
                }
            } else {
                None
            };
            if !args.dry_run {
                let finish_request = build_plan_activity_request(
                    &project,
                    if report.verified_complete {
                        PlanActivityAction::FinishVerified
                    } else {
                        PlanActivityAction::FinishBlocked
                    },
                    &report.plan_title,
                    &report.thread_key,
                    report.total_items,
                    report.completed_items,
                    report.remaining_items.clone(),
                    None,
                );
                if let Err(error) = api.log_plan_activity(&finish_request).await {
                    eprintln!("warning: failed to log plan activity for `{project}`: {error}");
                }
            }
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "report": report,
                        "implementation": implementation,
                        "dry_run": args.dry_run,
                    }))?
                );
            } else {
                print_plan_execution_finish_report(&report);
                if let Some(implementation) = &implementation {
                    if args.dry_run {
                        println!(
                            "\nWould record implementation memory: {}",
                            implementation.summary
                        );
                    } else if implementation.recorded {
                        println!(
                            "\nRecorded implementation memory: {}",
                            implementation.summary
                        );
                    }
                }
                if args.dry_run {
                    println!("\nDry run only: no plan state was synced, logged, or persisted.");
                }
            }
            if !report.verified_complete {
                anyhow::bail!("approved plan still has unchecked items");
            }
        }
    }

    Ok(())
}
