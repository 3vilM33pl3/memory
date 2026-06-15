use anyhow::Result;
use chrono::{DateTime, Utc};
use mem_api::{
    LoopApprovalDecisionRequest, LoopApprovalStatus, LoopCancelRequest, LoopFeedbackRequest,
    LoopGlobalStateUpdateRequest, LoopMode, LoopRunRequest, LoopRunStatus, LoopScopeType,
    LoopSettingResponse, LoopSettingsUpdateRequest,
};
use serde::Serialize;
use serde_json::json;

use crate::commands::{
    api::ApiClient,
    runtime::{LoopApprovalDecisionArgs, LoopRunArgs, LoopSettingArgs, LoopsArgs, LoopsCommand},
};

pub(super) async fn handle(args: LoopsArgs, api: &ApiClient) -> Result<()> {
    match args.command {
        LoopsCommand::List(args) => {
            let response = api.loop_definitions().await?;
            if args.json {
                print_json(&response)?;
            } else {
                print_loop_definitions(&response);
            }
        }
        LoopsCommand::Show(args) => {
            let repo_root = args
                .repo_root
                .as_ref()
                .map(|path| path.display().to_string());
            let response = api
                .loop_definition(&args.loop_id, args.project.as_deref(), repo_root.as_deref())
                .await?;
            if args.json {
                print_json(&response)?;
            } else {
                print_loop_definition(&response);
            }
        }
        LoopsCommand::Enable(args) => {
            let request = setting_request(
                &args.setting,
                Some(true),
                args.mode.as_deref().map(parse_loop_mode).transpose()?,
                None,
                None,
            )?;
            let response = api.loop_enable(&args.setting.loop_id, &request).await?;
            print_setting_response(&response, args.setting.json, "enable")?;
        }
        LoopsCommand::Disable(args) => {
            let request =
                setting_request(&args.setting, Some(false), Some(LoopMode::Off), None, None)?;
            let response = api.loop_disable(&args.setting.loop_id, &request).await?;
            print_setting_response(&response, args.setting.json, "disable")?;
        }
        LoopsCommand::Pause(args) => {
            let request = setting_request(
                &args.setting,
                None,
                Some(LoopMode::Paused),
                Some(args.until),
                None,
            )?;
            let response = api.loop_pause(&args.setting.loop_id, &request).await?;
            print_setting_response(&response, args.setting.json, "pause")?;
        }
        LoopsCommand::Snooze(args) => {
            let request = setting_request(
                &args.setting,
                None,
                Some(LoopMode::Snoozed),
                None,
                Some(args.until),
            )?;
            let response = api.loop_snooze(&args.setting.loop_id, &request).await?;
            print_setting_response(&response, args.setting.json, "snooze")?;
        }
        LoopsCommand::Run(args) => {
            run_loop(api, args).await?;
        }
        LoopsCommand::Runs(args) => {
            let status = args.status.as_deref().map(parse_run_status).transpose()?;
            let response = api
                .loop_runs(
                    args.project.as_deref(),
                    args.loop_id.as_deref(),
                    status,
                    args.limit,
                )
                .await?;
            if args.json {
                print_json(&response)?;
            } else {
                print_loop_runs(&response);
            }
        }
        LoopsCommand::Inspect(args) => {
            let response = api.loop_run_detail(args.run_id).await?;
            if args.json {
                print_json(&response)?;
            } else {
                print_loop_run(&response, true);
            }
        }
        LoopsCommand::Cancel(args) => {
            let request = LoopCancelRequest {
                reason: args.reason.clone(),
            };
            let response = api.loop_cancel(args.run_id, &request).await?;
            if args.json {
                print_json(&response)?;
            } else {
                print_loop_run(&response, false);
            }
        }
        LoopsCommand::Feedback(args) => {
            let request = LoopFeedbackRequest {
                rating: args.rating,
                note: args.note,
            };
            request.validate().map_err(anyhow::Error::msg)?;
            let response = api.loop_feedback(args.run_id, &request).await?;
            if args.json {
                print_json(&response)?;
            } else {
                print_loop_run(&response, false);
            }
        }
        LoopsCommand::Approvals(args) => {
            let status = args
                .status
                .as_deref()
                .map(parse_approval_status)
                .transpose()?;
            let response = api
                .loop_approvals(args.project.as_deref(), status, args.limit)
                .await?;
            if args.json {
                print_json(&response)?;
            } else {
                print_loop_approvals(&response);
            }
        }
        LoopsCommand::Approve(args) => {
            let response = api
                .loop_approval_decision(args.approval_id, true, &decision_request(&args))
                .await?;
            if args.json {
                print_json(&response)?;
            } else {
                println!(
                    "Approved {} for loop {}.",
                    response.approval.id, response.approval.loop_id
                );
            }
        }
        LoopsCommand::Reject(args) => {
            let response = api
                .loop_approval_decision(args.approval_id, false, &decision_request(&args))
                .await?;
            if args.json {
                print_json(&response)?;
            } else {
                println!(
                    "Rejected {} for loop {}.",
                    response.approval.id, response.approval.loop_id
                );
            }
        }
        LoopsCommand::Replay(args) => {
            let original = api.loop_run_detail(args.run_id).await?;
            let request = LoopRunRequest {
                project: original.run.summary.project.clone(),
                repo_root: original.run.summary.repo_root.clone(),
                scope_type: None,
                scope_id: None,
                dry_run: args.dry_run,
                reason: args
                    .reason
                    .or_else(|| Some(format!("Replay of loop run {}", original.run.summary.id))),
                trigger_payload: Some(json!({ "replay_of": original.run.summary.id })),
            };
            let response = api
                .loop_run(&original.run.summary.loop_id, &request)
                .await?;
            if args.json {
                print_json(&response)?;
            } else {
                println!("Replayed run {}:", original.run.summary.id);
                print_loop_run(&response, true);
            }
        }
        LoopsCommand::GlobalKillSwitch(args) => {
            if let Some(enabled) = args.kill_switch_enabled {
                let request = LoopGlobalStateUpdateRequest {
                    kill_switch_enabled: enabled,
                    updated_by: args.updated_by,
                    reason: args.reason,
                };
                let response = api.loop_set_global_state(&request).await?;
                if args.json {
                    print_json(&response)?;
                } else {
                    print_global_state(&response);
                }
            } else {
                let response = api.loop_global_state().await?;
                if args.json {
                    print_json(&response)?;
                } else {
                    print_global_state(&response);
                }
            }
        }
    }
    Ok(())
}

async fn run_loop(api: &ApiClient, args: LoopRunArgs) -> Result<mem_api::LoopRunResponse> {
    let repo_root = args
        .repo_root
        .as_ref()
        .map(|path| path.display().to_string());
    let request = LoopRunRequest {
        project: args.project,
        repo_root,
        scope_type: args
            .scope_type
            .as_deref()
            .map(parse_scope_type)
            .transpose()?,
        scope_id: args.scope_id,
        dry_run: args.dry_run,
        reason: args.reason,
        trigger_payload: args.trigger_payload,
    };
    request.validate().map_err(anyhow::Error::msg)?;
    let response = api.loop_run(&args.loop_id, &request).await?;
    if args.json {
        print_json(&response)?;
    } else {
        print_loop_run(&response, true);
    }
    Ok(response)
}

fn setting_request(
    args: &LoopSettingArgs,
    enabled: Option<bool>,
    mode: Option<LoopMode>,
    paused_until: Option<DateTime<Utc>>,
    snoozed_until: Option<DateTime<Utc>>,
) -> Result<LoopSettingsUpdateRequest> {
    let request = LoopSettingsUpdateRequest {
        scope_type: args
            .scope_type
            .as_deref()
            .map(parse_scope_type)
            .transpose()?,
        scope_id: args.scope_id.clone(),
        project: args.project.clone(),
        repo_root: args
            .repo_root
            .as_ref()
            .map(|path| path.display().to_string()),
        enabled,
        mode,
        budgets: None,
        approval_overrides: None,
        paused_until,
        snoozed_until,
        updated_by: args.updated_by.clone(),
        reason: args.reason.clone(),
        explicit_user_approval: args.explicit_user_approval,
    };
    request.validate().map_err(anyhow::Error::msg)?;
    Ok(request)
}

fn decision_request(args: &LoopApprovalDecisionArgs) -> LoopApprovalDecisionRequest {
    LoopApprovalDecisionRequest {
        reviewer: args.reviewer.clone(),
        reason: args.reason.clone(),
    }
}

fn print_setting_response(
    response: &LoopSettingResponse,
    json_output: bool,
    action: &str,
) -> Result<()> {
    if json_output {
        return print_json(response);
    }
    if let Some(approval) = &response.approval {
        println!(
            "Loop {action} requires approval: {} [{}] {}",
            approval.id,
            approval.status.as_str(),
            approval.risk_reason
        );
        println!("Loop: {}", approval.loop_id);
        return Ok(());
    }
    println!(
        "Loop {action}: {} {}:{}",
        response.setting.loop_id, response.setting.scope_type, response.setting.scope_id
    );
    println!(
        "Effective: enabled={} mode={} blocked={}",
        response.effective_settings.enabled,
        response.effective_settings.mode,
        format_blocked(&response.effective_settings.blocked_reasons)
    );
    if let Some(budgets) = &response.effective_settings.budgets {
        println!("Budgets: {}", serde_json::to_string(budgets)?);
    }
    Ok(())
}

fn print_loop_definitions(response: &mem_api::LoopDefinitionsResponse) {
    if response.definitions.is_empty() {
        println!("No loop definitions registered.");
        return;
    }
    println!("Loop automations: {}", response.definitions.len());
    for definition in &response.definitions {
        println!(
            "- {} v{} risk={} default={} :: {}",
            definition.loop_id,
            definition.version,
            definition.risk_level,
            definition.default_mode,
            definition.name
        );
        println!("  {}", definition.description);
    }
}

fn print_loop_definition(response: &mem_api::LoopDefinitionResponse) {
    let definition = &response.definition;
    println!("Loop: {} v{}", definition.loop_id, definition.version);
    println!("Name: {}", definition.name);
    println!("Risk: {}", definition.risk_level);
    println!("Default mode: {}", definition.default_mode);
    println!("Description: {}", definition.description);
    if let Some(effective) = &response.effective_settings {
        println!(
            "Effective: enabled={} mode={} scope={}:{} blocked={}",
            effective.enabled,
            effective.mode,
            effective.scope_type,
            effective.scope_id,
            format_blocked(&effective.blocked_reasons)
        );
    }
}

fn print_loop_runs(response: &mem_api::LoopRunsResponse) {
    if response.runs.is_empty() {
        println!("No loop runs found.");
        return;
    }
    println!("Loop runs: {}", response.runs.len());
    for run in &response.runs {
        println!(
            "- {} {} status={} mode={} traces={} started={}",
            run.id,
            run.loop_id,
            run.status.as_str(),
            run.mode,
            run.trace_count,
            run.started_at.to_rfc3339()
        );
        if let Some(summary) = &run.output_summary {
            println!("  {summary}");
        }
        if !run.blocked_reasons.is_empty() {
            println!("  blocked: {}", run.blocked_reasons.join(", "));
        }
    }
}

fn print_loop_run(response: &mem_api::LoopRunResponse, include_traces: bool) {
    let run = &response.run.summary;
    println!("Run: {}", run.id);
    println!("Loop: {} v{}", run.loop_id, run.definition_version);
    println!("Status: {}", run.status.as_str());
    println!("Mode: {}", run.mode);
    if let Some(project) = &run.project {
        println!("Project: {project}");
    }
    if let Some(repo_root) = &run.repo_root {
        println!("Repo: {repo_root}");
    }
    println!("Started: {}", run.started_at.to_rfc3339());
    if let Some(finished) = run.finished_at {
        println!("Finished: {}", finished.to_rfc3339());
    }
    if let Some(summary) = &run.output_summary {
        println!("Summary: {summary}");
    }
    if !run.blocked_reasons.is_empty() {
        println!("Blocked: {}", run.blocked_reasons.join(", "));
    }
    println!("Traces: {}", run.trace_count);
    if include_traces && !response.run.traces.is_empty() {
        println!("\nTrace:");
        for trace in &response.run.traces {
            println!(
                "- #{} {} {} redacted={}",
                trace.sequence, trace.trace_type, trace.title, trace.redacted
            );
        }
    }
}

fn print_loop_approvals(response: &mem_api::LoopApprovalsResponse) {
    if response.approvals.is_empty() {
        println!("No loop approvals found.");
        return;
    }
    println!("Loop approvals: {}", response.approvals.len());
    for approval in &response.approvals {
        println!(
            "- {} loop={} action={} status={} created={}",
            approval.id,
            approval.loop_id,
            approval.action_type,
            approval.status.as_str(),
            approval.created_at.to_rfc3339()
        );
        println!("  {}", approval.risk_reason);
    }
}

fn print_global_state(response: &mem_api::LoopGlobalStateResponse) {
    println!(
        "Global kill switch: {}",
        if response.kill_switch_enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
    if let Some(reason) = &response.reason {
        println!("Reason: {reason}");
    }
    println!("Updated: {}", response.updated_at.to_rfc3339());
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn format_blocked(reasons: &[String]) -> String {
    if reasons.is_empty() {
        "none".to_string()
    } else {
        reasons.join(", ")
    }
}

fn parse_loop_mode(value: &str) -> Result<LoopMode> {
    match value {
        "off" => Ok(LoopMode::Off),
        "observe" => Ok(LoopMode::Observe),
        "suggest_only" | "suggest-only" => Ok(LoopMode::SuggestOnly),
        "draft_output" | "draft-output" => Ok(LoopMode::DraftOutput),
        "autonomous_safe" | "autonomous-safe" => Ok(LoopMode::AutonomousSafe),
        "paused" => Ok(LoopMode::Paused),
        "snoozed" => Ok(LoopMode::Snoozed),
        _ => anyhow::bail!("unsupported loop mode `{value}`"),
    }
}

fn parse_scope_type(value: &str) -> Result<LoopScopeType> {
    match value {
        "user" => Ok(LoopScopeType::User),
        "workspace" => Ok(LoopScopeType::Workspace),
        "project" => Ok(LoopScopeType::Project),
        "repo" => Ok(LoopScopeType::Repo),
        _ => anyhow::bail!("unsupported loop scope type `{value}`"),
    }
}

fn parse_run_status(value: &str) -> Result<LoopRunStatus> {
    match value {
        "queued" => Ok(LoopRunStatus::Queued),
        "running" => Ok(LoopRunStatus::Running),
        "succeeded" => Ok(LoopRunStatus::Succeeded),
        "failed" => Ok(LoopRunStatus::Failed),
        "cancelled" | "canceled" => Ok(LoopRunStatus::Cancelled),
        "blocked" => Ok(LoopRunStatus::Blocked),
        _ => anyhow::bail!("unsupported loop run status `{value}`"),
    }
}

fn parse_approval_status(value: &str) -> Result<LoopApprovalStatus> {
    match value {
        "pending" => Ok(LoopApprovalStatus::Pending),
        "approved" => Ok(LoopApprovalStatus::Approved),
        "rejected" => Ok(LoopApprovalStatus::Rejected),
        "edited" => Ok(LoopApprovalStatus::Edited),
        _ => anyhow::bail!("unsupported loop approval status `{value}`"),
    }
}
