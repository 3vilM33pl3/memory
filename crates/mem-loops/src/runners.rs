use crate::{
    LoopRunner, PolicyDecision, RunnerArtifact, RunnerInvocation, RunnerMemoryUpdateProposal,
    RunnerResult, RunnerStatus, SandboxCommandRequest, SandboxLimits, SandboxWorkspace,
    WorktreeSandboxManager,
};
use mem_api::LoopActionKind;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunnerKind {
    Codex,
    ClaudeCode,
    OpenHands,
}

impl AgentRunnerKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::ClaudeCode => "claude_code",
            Self::OpenHands => "openhands",
        }
    }

    fn display_name(&self) -> &'static str {
        match self {
            Self::Codex => "Codex CLI",
            Self::ClaudeCode => "Claude Code",
            Self::OpenHands => "OpenHands",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentRunnerConfig {
    pub kind: AgentRunnerKind,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub command_template: Vec<String>,
}

impl AgentRunnerConfig {
    pub fn codex_disabled() -> Self {
        Self {
            kind: AgentRunnerKind::Codex,
            enabled: false,
            command_template: vec![
                "codex".to_string(),
                "exec".to_string(),
                "--cd".to_string(),
                "{workspace}".to_string(),
                "{prompt}".to_string(),
            ],
        }
    }

    pub fn claude_code_disabled() -> Self {
        Self {
            kind: AgentRunnerKind::ClaudeCode,
            enabled: false,
            command_template: vec![
                "claude".to_string(),
                "-p".to_string(),
                "{prompt}".to_string(),
            ],
        }
    }

    pub fn openhands_disabled() -> Self {
        Self {
            kind: AgentRunnerKind::OpenHands,
            enabled: false,
            command_template: vec![
                "openhands".to_string(),
                "--workspace".to_string(),
                "{workspace}".to_string(),
                "--task".to_string(),
                "{prompt}".to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentCliRunner {
    config: AgentRunnerConfig,
    sandbox: WorktreeSandboxManager,
}

impl AgentCliRunner {
    pub fn new(config: AgentRunnerConfig) -> Self {
        Self {
            config,
            sandbox: WorktreeSandboxManager::default(),
        }
    }

    pub fn with_sandbox(config: AgentRunnerConfig, sandbox: WorktreeSandboxManager) -> Self {
        Self { config, sandbox }
    }
}

impl LoopRunner for AgentCliRunner {
    fn runner_id(&self) -> &str {
        self.config.kind.as_str()
    }

    fn invoke(&self, invocation: RunnerInvocation) -> RunnerResult {
        let placeholder_policy = PolicyDecision {
            action: LoopActionKind::InvokeRunner,
            allowed: true,
            requires_approval: true,
            reason: "allowed_by_mode".to_string(),
        };
        if !self.config.enabled {
            return RunnerResult {
                runner_id: self.runner_id().to_string(),
                status: RunnerStatus::Blocked,
                summary: format!("{} runner is disabled", self.config.kind.display_name()),
                artifacts: Vec::new(),
                changed_files: Vec::new(),
                command_outputs: Vec::new(),
                memory_updates: Vec::new(),
                policy_decision: placeholder_policy,
                metadata: json!({ "enabled": false }),
            };
        }

        match self.invoke_enabled(invocation, placeholder_policy.clone()) {
            Ok(result) => result,
            Err(error) => RunnerResult {
                runner_id: self.runner_id().to_string(),
                status: RunnerStatus::Failed,
                summary: format!("{} runner failed: {error}", self.config.kind.display_name()),
                artifacts: Vec::new(),
                changed_files: Vec::new(),
                command_outputs: Vec::new(),
                memory_updates: Vec::new(),
                policy_decision: placeholder_policy,
                metadata: json!({ "enabled": true }),
            },
        }
    }
}

impl AgentCliRunner {
    fn invoke_enabled(
        &self,
        invocation: RunnerInvocation,
        policy_decision: PolicyDecision,
    ) -> io::Result<RunnerResult> {
        let workspace = workspace_from_invocation(&invocation)?;
        let prompt = render_agent_runner_prompt(&self.config.kind, &invocation);
        let prompt_path =
            write_prompt_artifact(&workspace.worktree_path, &self.config.kind, &prompt)?;
        let command = command_from_template(
            &self.config.command_template,
            &workspace.worktree_path,
            &prompt_path,
            &prompt,
        )?;
        let limits = SandboxLimits {
            max_runtime_seconds: invocation.budget.max_seconds.max(1),
            allowed_commands: invocation.capability_profile.allowed_commands.clone(),
            ..SandboxLimits::default()
        };
        let command_log = self.sandbox.run_command(&workspace, &command, &limits)?;
        let capture =
            self.sandbox
                .capture_workspace(&workspace, vec![command_log.clone()], &limits)?;
        let success = command_log.exit_code == 0
            && !command_log.timed_out
            && capture.limit_violations.is_empty();
        let prompt_artifact = RunnerArtifact {
            path: prompt_path.display().to_string(),
            artifact_type: "prompt".to_string(),
            summary: Some(format!(
                "{} adapter prompt",
                self.config.kind.display_name()
            )),
        };
        let mut artifacts = vec![prompt_artifact];
        artifacts.extend(capture.artifacts);
        let memory_updates = success
            .then(|| RunnerMemoryUpdateProposal {
                proposal_type: "add".to_string(),
                summary: format!(
                    "{} implementation result for {}",
                    self.config.kind.display_name(),
                    invocation.task_pack.title
                ),
                candidate: json!({
                    "summary": format!(
                        "{} implementation result for {}",
                        self.config.kind.display_name(),
                        invocation.task_pack.title
                    ),
                    "memory_type": "implementation",
                    "tags": ["loop-engineering", self.config.kind.as_str()]
                }),
                evidence: json!([{
                    "source_kind": "command_output",
                    "excerpt": command_log.stdout.chars().take(1_000).collect::<String>()
                }]),
            })
            .into_iter()
            .collect();

        Ok(RunnerResult {
            runner_id: self.runner_id().to_string(),
            status: if success {
                RunnerStatus::Succeeded
            } else {
                RunnerStatus::Failed
            },
            summary: if success {
                format!("{} runner completed", self.config.kind.display_name())
            } else {
                format!(
                    "{} runner did not complete cleanly",
                    self.config.kind.display_name()
                )
            },
            artifacts,
            changed_files: capture.changed_files,
            command_outputs: vec![command_log.runner_command_output()],
            memory_updates,
            policy_decision,
            metadata: json!({
                "adapter": self.config.kind.as_str(),
                "enabled": true,
                "workspace": workspace.runner_workspace_ref(),
                "diff_bytes": capture.diff_bytes,
                "limit_violations": capture.limit_violations
            }),
        })
    }
}

pub fn render_agent_runner_prompt(kind: &AgentRunnerKind, invocation: &RunnerInvocation) -> String {
    let mut prompt = String::new();
    prompt.push_str(&format!("# {} Loop Task\n\n", kind.display_name()));
    prompt.push_str("## Task\n");
    prompt.push_str(&invocation.task_pack.title);
    prompt.push_str("\n\n");
    prompt.push_str(&invocation.task_pack.prompt);
    prompt.push_str("\n\n");
    if !invocation.task_pack.acceptance_criteria.is_empty() {
        prompt.push_str("## Acceptance Criteria\n");
        for item in &invocation.task_pack.acceptance_criteria {
            prompt.push_str("- ");
            prompt.push_str(item);
            prompt.push('\n');
        }
        prompt.push('\n');
    }
    prompt.push_str("## Workspace\n");
    prompt.push_str(&format!(
        "- Repo root: {}\n",
        invocation.workspace.repo_root
    ));
    if let Some(path) = invocation.workspace.worktree_path.as_deref() {
        prompt.push_str(&format!("- Worktree: {path}\n"));
    }
    if let Some(branch) = invocation.workspace.branch.as_deref() {
        prompt.push_str(&format!("- Branch: {branch}\n"));
    }
    prompt.push_str("\n## Context Memories\n");
    for memory in invocation.context_pack.memories.iter().take(12) {
        prompt.push_str(&format!(
            "- {} [{}]: {}\n",
            memory.memory_id, memory.memory_type, memory.summary
        ));
    }
    if invocation.context_pack.memories.is_empty() {
        prompt.push_str("- No memories were selected for this run.\n");
    }
    prompt.push_str("\n## Runner Contract\n");
    prompt.push_str("- Work only inside the provided workspace.\n");
    prompt.push_str("- Do not read or print credentials, tokens, or environment secrets.\n");
    prompt.push_str("- Leave a reviewable diff and run the narrowest useful validation.\n");
    prompt
}

fn workspace_from_invocation(invocation: &RunnerInvocation) -> io::Result<SandboxWorkspace> {
    let worktree_path = invocation
        .workspace
        .worktree_path
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&invocation.workspace.repo_root));
    Ok(SandboxWorkspace {
        project: invocation.context_pack.project.clone(),
        run_id: invocation
            .context_pack
            .run_id
            .unwrap_or(invocation.context_pack.id),
        repo_root: PathBuf::from(&invocation.workspace.repo_root),
        worktree_path,
        branch: invocation
            .workspace
            .branch
            .clone()
            .unwrap_or_else(|| "memory/loops/untracked".to_string()),
    })
}

fn write_prompt_artifact(
    workspace_path: &Path,
    kind: &AgentRunnerKind,
    prompt: &str,
) -> io::Result<PathBuf> {
    let artifact_root = workspace_path.join(".mem").join("loop-artifacts");
    fs::create_dir_all(&artifact_root)?;
    let prompt_path = artifact_root.join(format!("{}-prompt.md", kind.as_str()));
    fs::write(&prompt_path, prompt)?;
    Ok(prompt_path)
}

fn command_from_template(
    template: &[String],
    workspace_path: &Path,
    prompt_path: &Path,
    prompt: &str,
) -> io::Result<SandboxCommandRequest> {
    if template.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "runner command template must not be empty",
        ));
    }
    let values = template
        .iter()
        .map(|value| {
            value
                .replace("{workspace}", &workspace_path.display().to_string())
                .replace("{prompt_file}", &prompt_path.display().to_string())
                .replace("{prompt}", prompt)
        })
        .collect::<Vec<_>>();
    let mut iter = values.into_iter();
    let program = iter.next().expect("template is non-empty");
    Ok(SandboxCommandRequest {
        program,
        args: iter.collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        LoopContextPack, LoopMode, RunnerBudget, RunnerCapabilityProfile, RunnerTaskPack,
        RunnerWorkspaceRef, invoke_runner_with_policy,
    };
    use chrono::Utc;
    use std::process::Command;
    use uuid::Uuid;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("mem-loop-runner-{name}-{}", Uuid::new_v4()))
    }

    fn init_repo(name: &str) -> PathBuf {
        let repo = temp_path(name);
        fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-b", "main"]);
        fs::write(repo.join("README.md"), "initial\n").unwrap();
        git(&repo, &["add", "README.md"]);
        git(
            &repo,
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test User",
                "commit",
                "-m",
                "initial",
            ],
        );
        repo
    }

    fn git(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn invocation(repo: &Path, mode: LoopMode) -> RunnerInvocation {
        let run_id = Uuid::new_v4();
        RunnerInvocation {
            runner_id: "codex".to_string(),
            task_pack: RunnerTaskPack {
                title: "Edit README".to_string(),
                prompt: "Append adapter output to README.md.".to_string(),
                acceptance_criteria: vec!["README.md changes".to_string()],
                metadata: json!({}),
            },
            context_pack: LoopContextPack {
                id: Uuid::new_v4(),
                loop_id: crate::LOOP_DRAFT_PR.to_string(),
                project: "memory".to_string(),
                repo_root: Some(repo.display().to_string()),
                run_id: Some(run_id),
                generated_at: Utc::now(),
                token_budget: 500,
                estimated_tokens: 1,
                instructions: Vec::new(),
                memories: Vec::new(),
                exclusions: Vec::new(),
                warnings: Vec::new(),
                metadata: json!({}),
            },
            capability_profile: RunnerCapabilityProfile {
                can_read_repo: true,
                can_write_repo: true,
                can_run_commands: true,
                can_propose_memory: true,
                allowed_commands: vec!["sh".to_string()],
            },
            workspace: RunnerWorkspaceRef {
                repo_root: repo.display().to_string(),
                worktree_path: Some(repo.display().to_string()),
                branch: Some("memory/loops/test".to_string()),
            },
            budget: RunnerBudget {
                max_seconds: 30,
                max_tokens: 1_000,
                max_cost_usd: 0.25,
            },
            mode,
        }
    }

    #[test]
    fn disabled_adapter_is_independently_blocked() {
        let repo = init_repo("disabled");
        let runner = AgentCliRunner::new(AgentRunnerConfig::codex_disabled());
        let result = invoke_runner_with_policy(&runner, invocation(&repo, LoopMode::DraftOutput));

        assert_eq!(result.status, RunnerStatus::Blocked);
        assert_eq!(result.runner_id, "codex");
        assert!(result.summary.contains("disabled"));
        let _ = fs::remove_dir_all(repo);
    }

    #[test]
    fn codex_adapter_invokes_configured_command_and_normalizes_output() {
        let repo = init_repo("codex");
        let runner = AgentCliRunner::new(AgentRunnerConfig {
            kind: AgentRunnerKind::Codex,
            enabled: true,
            command_template: vec![
                "sh".to_string(),
                "-c".to_string(),
                "printf '\\nadapter' >> README.md && printf done".to_string(),
            ],
        });
        let result = invoke_runner_with_policy(&runner, invocation(&repo, LoopMode::DraftOutput));

        assert_eq!(result.status, RunnerStatus::Succeeded);
        assert_eq!(result.command_outputs[0].exit_code, 0);
        assert_eq!(result.command_outputs[0].stdout.as_deref(), Some("done"));
        assert!(
            result
                .changed_files
                .iter()
                .any(|file| file.path == "README.md")
        );
        assert!(result.artifacts.iter().any(|artifact| {
            artifact.path.ends_with("codex-prompt.md") && artifact.artifact_type == "prompt"
        }));
        assert_eq!(result.memory_updates.len(), 1);
        let _ = fs::remove_dir_all(repo);
    }

    #[test]
    fn adapter_prompt_does_not_include_secret_values() {
        let repo = init_repo("prompt");
        let prompt = render_agent_runner_prompt(
            &AgentRunnerKind::ClaudeCode,
            &invocation(&repo, LoopMode::DraftOutput),
        );

        assert!(prompt.contains("Claude Code Loop Task"));
        assert!(prompt.contains("Do not read or print credentials"));
        assert!(!prompt.contains("OPENAI_API_KEY"));
        assert!(!prompt.contains("ANTHROPIC_API_KEY"));
        let _ = fs::remove_dir_all(repo);
    }
}
