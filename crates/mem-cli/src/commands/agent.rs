use std::{
    env,
    path::PathBuf,
    process::{self, Command},
};

use anyhow::{Context, Result, anyhow};
use mem_api::{
    AgentWorkspaceFinishRequest, AgentWorkspaceHeartbeatRequest, AgentWorkspaceListResponse,
    AgentWorkspaceRecord, AgentWorkspaceStartRequest, AgentWorkspaceStatus, Profile,
};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    commands::{
        api::ApiClient,
        runtime::{AgentArgs, AgentCommand, AgentFinishArgs, AgentStartArgs, AgentStatusArgs},
    },
    writer_identity::resolve_writer_identity,
};

pub(crate) async fn handle(
    args: AgentArgs,
    api: &ApiClient,
    cli_writer_id: Option<String>,
) -> Result<()> {
    match args.command {
        AgentCommand::Start(args) => start(args, api, cli_writer_id).await,
        AgentCommand::Status(args) => status(args, api).await,
        AgentCommand::Finish(args) => finish(args, api).await,
    }
}

async fn start(args: AgentStartArgs, api: &ApiClient, cli_writer_id: Option<String>) -> Result<()> {
    let git = GitWorkspace::detect(args.branch.as_deref())?;
    let writer = resolve_writer_identity(&api.config, cli_writer_id.as_deref())?;
    let request = AgentWorkspaceStartRequest {
        project: args.project.project,
        repo_root: git.repo_root,
        worktree_path: git.worktree_path,
        branch: git.branch,
        base_commit: git.base_commit,
        head_commit: git.head_commit,
        dirty_files: git.dirty_files,
        agent_cli: args.agent_cli,
        agent_session_id: Some(resolve_agent_session_id(args.agent_session_id)),
        hostname: resolve_hostname(),
        writer_id: Some(writer.id),
        profile: Some(Profile::detect().to_string()),
        service_endpoint: Some(service_endpoint(api)),
        task: normalize_option(args.task),
    };
    let workspace = api.start_agent_workspace(&request).await?;
    if args.json {
        print_json(&workspace)?;
    } else {
        print_workspace_started(&workspace);
    }
    Ok(())
}

async fn status(args: AgentStatusArgs, api: &ApiClient) -> Result<()> {
    let response = api
        .agent_workspaces(&args.project.project, args.include_finished)
        .await?;
    if args.json {
        print_json(&response)?;
    } else {
        print_workspace_status(&response);
    }
    Ok(())
}

async fn finish(args: AgentFinishArgs, api: &ApiClient) -> Result<()> {
    let git = GitWorkspace::detect(None)?;
    let workspace_id = match args.workspace_id {
        Some(id) => id,
        None => resolve_current_workspace(api, &args.project.project, &git).await?,
    };
    let status = if args.abandoned {
        AgentWorkspaceStatus::Abandoned
    } else {
        AgentWorkspaceStatus::Completed
    };
    let request = AgentWorkspaceFinishRequest {
        status: Some(status),
        head_commit: git.head_commit,
        dirty_files: git.dirty_files,
        finish_summary: normalize_option(args.summary),
        pushed_branch: Some(args.pushed.unwrap_or_else(detect_branch_pushed)),
        merged_commit: normalize_option(args.merged_commit),
    };
    let workspace = api
        .finish_agent_workspace(workspace_id, &request)
        .await
        .with_context(|| format!("finish agent workspace {workspace_id}"))?;
    if args.json {
        print_json(&workspace)?;
    } else {
        print_workspace_finished(&workspace);
    }
    Ok(())
}

#[allow(dead_code)]
pub(crate) async fn heartbeat(
    api: &ApiClient,
    workspace_id: Uuid,
    service_endpoint: Option<String>,
) -> Result<AgentWorkspaceRecord> {
    let git = GitWorkspace::detect(None)?;
    let request = AgentWorkspaceHeartbeatRequest {
        head_commit: git.head_commit,
        dirty_files: git.dirty_files,
        service_endpoint,
    };
    api.heartbeat_agent_workspace(workspace_id, &request).await
}

async fn resolve_current_workspace(
    api: &ApiClient,
    project: &str,
    git: &GitWorkspace,
) -> Result<Uuid> {
    let response = api.agent_workspaces(project, false).await?;
    let session_id = agent_session_id_from_env();
    let mut matches = response
        .workspaces
        .iter()
        .filter(|workspace| workspace.status == AgentWorkspaceStatus::Active)
        .filter(|workspace| workspace.repo_root == git.repo_root && workspace.branch == git.branch)
        .filter(|workspace| {
            session_id
                .as_ref()
                .is_none_or(|session_id| workspace.agent_session_id.as_ref() == Some(session_id))
        })
        .collect::<Vec<_>>();

    if matches.is_empty() && session_id.is_some() {
        matches = response
            .workspaces
            .iter()
            .filter(|workspace| workspace.status == AgentWorkspaceStatus::Active)
            .filter(|workspace| {
                workspace.repo_root == git.repo_root && workspace.branch == git.branch
            })
            .collect::<Vec<_>>();
    }

    match matches.as_slice() {
        [workspace] => Ok(workspace.id),
        [] => Err(anyhow!(
            "no active agent workspace matches repo {} on branch {}; run memory agent status --project {}",
            git.repo_root,
            git.branch,
            project
        )),
        _ => Err(anyhow!(
            "multiple active workspaces match repo {} on branch {}; pass --workspace-id",
            git.repo_root,
            git.branch
        )),
    }
}

#[derive(Debug)]
struct GitWorkspace {
    repo_root: String,
    worktree_path: String,
    branch: String,
    base_commit: Option<String>,
    head_commit: Option<String>,
    dirty_files: Vec<String>,
}

impl GitWorkspace {
    fn detect(branch_override: Option<&str>) -> Result<Self> {
        let repo_root = git_text(&["rev-parse", "--show-toplevel"])
            .or_else(|| env::current_dir().ok().map(path_to_string))
            .ok_or_else(|| anyhow!("could not determine current directory"))?;
        let worktree_path = git_text(&["rev-parse", "--path-format=absolute", "--show-toplevel"])
            .unwrap_or_else(|| repo_root.clone());
        let branch = branch_override
            .and_then(|value| normalize_string(value.to_string()))
            .or_else(|| git_text(&["branch", "--show-current"]))
            .or_else(|| git_text(&["rev-parse", "--short", "HEAD"]))
            .unwrap_or_else(|| "HEAD".to_string());
        let head_commit = git_text(&["rev-parse", "HEAD"]);
        let base_commit = git_text(&["merge-base", "HEAD", "origin/main"])
            .or_else(|| git_text(&["merge-base", "HEAD", "main"]))
            .or_else(|| git_text(&["merge-base", "HEAD", "master"]));
        let dirty_files = git_dirty_files();
        Ok(Self {
            repo_root,
            worktree_path,
            branch,
            base_commit,
            head_commit,
            dirty_files,
        })
    }
}

fn git_text(args: &[&str]) -> Option<String> {
    command_text("git", args)
}

fn command_text(command: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(command).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn git_dirty_files() -> Vec<String> {
    let Some(output) = git_text(&["status", "--porcelain"]) else {
        return Vec::new();
    };
    output
        .lines()
        .filter_map(|line| {
            let path = line.get(3..)?.trim();
            if path.is_empty() {
                return None;
            }
            Some(
                path.rsplit_once(" -> ")
                    .map(|(_, renamed)| renamed)
                    .unwrap_or(path)
                    .trim()
                    .to_string(),
            )
        })
        .collect()
}

fn detect_branch_pushed() -> bool {
    let Some(ahead) = git_text(&["rev-list", "--count", "@{u}..HEAD"]) else {
        return false;
    };
    ahead.parse::<u64>().is_ok_and(|count| count == 0)
}

fn resolve_agent_session_id(value: Option<String>) -> String {
    normalize_option(value)
        .or_else(agent_session_id_from_env)
        .unwrap_or_else(|| format!("pid-{}", process::id()))
}

fn agent_session_id_from_env() -> Option<String> {
    [
        "CODEX_SESSION_ID",
        "MEMORY_LAYER_AGENT_SESSION_ID",
        "CLAUDE_SESSION_ID",
    ]
    .iter()
    .find_map(|key| env::var(key).ok().and_then(normalize_string))
}

fn resolve_hostname() -> Option<String> {
    env::var("HOSTNAME")
        .ok()
        .and_then(normalize_string)
        .or_else(|| command_text("hostname", &[]))
}

fn service_endpoint(api: &ApiClient) -> String {
    format!("http://{}", api.config.service.bind_addr)
}

fn normalize_option(value: Option<String>) -> Option<String> {
    value.and_then(normalize_string)
}

fn normalize_string(value: impl Into<String>) -> Option<String> {
    let value = value.into();
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn path_to_string(path: PathBuf) -> String {
    path.to_string_lossy().into_owned()
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn print_workspace_started(workspace: &AgentWorkspaceRecord) {
    println!("Agent workspace active: {}", workspace.id);
    println!("Project: {}", workspace.project);
    println!("Branch: {}", workspace.branch);
    println!("Worktree: {}", workspace.worktree_path);
    println!("Dirty files: {}", workspace.dirty_count);
    print_warnings(workspace);
}

fn print_workspace_finished(workspace: &AgentWorkspaceRecord) {
    println!("Agent workspace {}: {}", workspace.status, workspace.id);
    println!("Branch: {}", workspace.branch);
    println!("Worktree: {}", workspace.worktree_path);
    println!(
        "Pushed: {}",
        workspace
            .pushed_branch
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    );
    if let Some(summary) = &workspace.finish_summary {
        println!("Summary: {summary}");
    }
    print_warnings(workspace);
}

fn print_workspace_status(response: &AgentWorkspaceListResponse) {
    println!("Agent workspaces for {}", response.project);
    if response.workspaces.is_empty() {
        println!("No agent workspaces found.");
        return;
    }
    for workspace in &response.workspaces {
        println!(
            "- {} | {} | {} | dirty {} | {}",
            workspace.status,
            workspace.branch,
            workspace.worktree_path,
            workspace.dirty_count,
            workspace.id
        );
        if let Some(session_id) = &workspace.agent_session_id {
            println!("  session: {} via {}", session_id, workspace.agent_cli);
        }
        if let Some(head) = &workspace.head_commit {
            println!("  head: {head}");
        }
        if let Some(endpoint) = &workspace.service_endpoint {
            println!("  service: {endpoint}");
        }
        print_warnings(workspace);
    }
    if !response.warnings.is_empty() {
        println!("\nWarnings:");
        for warning in &response.warnings {
            println!("- {}: {}", warning.code, warning.message);
        }
    }
}

fn print_warnings(workspace: &AgentWorkspaceRecord) {
    if workspace.warnings.is_empty() {
        return;
    }
    println!("Warnings:");
    for warning in &workspace.warnings {
        println!("- {}: {}", warning.code, warning.message);
    }
}
