use crate::{RunnerArtifact, RunnerChangedFile, RunnerCommandOutput, RunnerWorkspaceRef};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime},
};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SandboxLimits {
    pub max_changed_files: usize,
    pub max_diff_bytes: usize,
    pub max_runtime_seconds: u64,
    #[serde(default)]
    pub allowed_commands: Vec<String>,
}

impl Default for SandboxLimits {
    fn default() -> Self {
        Self {
            max_changed_files: 25,
            max_diff_bytes: 200_000,
            max_runtime_seconds: 600,
            allowed_commands: Vec::new(),
        }
    }
}

impl SandboxLimits {
    fn command_allowed(&self, request: &SandboxCommandRequest) -> bool {
        if self.allowed_commands.is_empty() {
            return false;
        }
        let command = request.command_line();
        self.allowed_commands
            .iter()
            .any(|allowed| allowed == &request.program || command.starts_with(allowed))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SandboxWorkspaceSpec {
    pub project: String,
    pub repo_root: PathBuf,
    pub run_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SandboxWorkspace {
    pub project: String,
    pub run_id: Uuid,
    pub repo_root: PathBuf,
    pub worktree_path: PathBuf,
    pub branch: String,
}

impl SandboxWorkspace {
    pub fn runner_workspace_ref(&self) -> RunnerWorkspaceRef {
        RunnerWorkspaceRef {
            repo_root: self.repo_root.display().to_string(),
            worktree_path: Some(self.worktree_path.display().to_string()),
            branch: Some(self.branch.clone()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SandboxCommandRequest {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
}

impl SandboxCommandRequest {
    pub fn new(
        program: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }

    pub fn command_line(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SandboxCommandLog {
    pub command: String,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u128,
    pub timed_out: bool,
    #[serde(default)]
    pub limit_violations: Vec<String>,
}

impl SandboxCommandLog {
    pub fn runner_command_output(&self) -> RunnerCommandOutput {
        RunnerCommandOutput {
            command: self.command.clone(),
            exit_code: self.exit_code,
            stdout: (!self.stdout.is_empty()).then(|| self.stdout.clone()),
            stderr: (!self.stderr.is_empty()).then(|| self.stderr.clone()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SandboxCapture {
    pub diff: String,
    pub diff_bytes: usize,
    #[serde(default)]
    pub changed_files: Vec<RunnerChangedFile>,
    #[serde(default)]
    pub artifacts: Vec<RunnerArtifact>,
    #[serde(default)]
    pub command_logs: Vec<SandboxCommandLog>,
    #[serde(default)]
    pub limit_violations: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct WorktreeSandboxManager {
    root_dir: Option<PathBuf>,
}

impl WorktreeSandboxManager {
    pub fn new(root_dir: impl Into<PathBuf>) -> Self {
        Self {
            root_dir: Some(root_dir.into()),
        }
    }

    pub fn create_workspace(&self, spec: &SandboxWorkspaceSpec) -> io::Result<SandboxWorkspace> {
        let repo_root = fs::canonicalize(&spec.repo_root)?;
        let sandbox_root = self.sandbox_root(&repo_root);
        fs::create_dir_all(&sandbox_root)?;
        let branch = sandbox_branch_name(&spec.project, spec.run_id);
        if is_protected_branch(&branch) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "sandbox branch may not target a protected branch",
            ));
        }
        let worktree_path = sandbox_root.join(spec.run_id.simple().to_string());
        let base_ref = spec.base_ref.as_deref().unwrap_or("HEAD");
        run_git_checked_owned(
            &repo_root,
            &[
                "worktree".to_string(),
                "add".to_string(),
                "-b".to_string(),
                branch.clone(),
                path_arg(&worktree_path),
                base_ref.to_string(),
            ],
        )?;
        let workspace = SandboxWorkspace {
            project: spec.project.clone(),
            run_id: spec.run_id,
            repo_root,
            worktree_path,
            branch,
        };
        self.write_metadata(&workspace)?;
        Ok(workspace)
    }

    pub fn run_command(
        &self,
        workspace: &SandboxWorkspace,
        request: &SandboxCommandRequest,
        limits: &SandboxLimits,
    ) -> io::Result<SandboxCommandLog> {
        if !limits.command_allowed(request) {
            return Ok(SandboxCommandLog {
                command: request.command_line(),
                exit_code: -1,
                stdout: String::new(),
                stderr: "command is not allowed by sandbox limits".to_string(),
                duration_ms: 0,
                timed_out: false,
                limit_violations: vec!["command_not_allowed".to_string()],
            });
        }

        let start = Instant::now();
        let mut child = Command::new(&request.program)
            .args(&request.args)
            .current_dir(&workspace.worktree_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let timeout = Duration::from_secs(limits.max_runtime_seconds.max(1));
        let mut timed_out = false;
        loop {
            if child.try_wait()?.is_some() {
                break;
            }
            if start.elapsed() >= timeout {
                timed_out = true;
                let _ = child.kill();
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let output = child.wait_with_output()?;
        let duration_ms = start.elapsed().as_millis();
        let mut limit_violations = Vec::new();
        if timed_out {
            limit_violations.push("runtime_exceeded".to_string());
        }
        Ok(SandboxCommandLog {
            command: request.command_line(),
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            duration_ms,
            timed_out,
            limit_violations,
        })
    }

    pub fn capture_workspace(
        &self,
        workspace: &SandboxWorkspace,
        command_logs: Vec<SandboxCommandLog>,
        limits: &SandboxLimits,
    ) -> io::Result<SandboxCapture> {
        let diff = run_git_text(&workspace.worktree_path, &["diff", "--binary"])?;
        let changed_files = changed_files(&workspace.worktree_path)?;
        let artifacts = collect_artifacts(&workspace.worktree_path)?;
        let diff_bytes = diff.len();
        let mut limit_violations = Vec::new();
        if changed_files.len() > limits.max_changed_files {
            limit_violations.push("changed_file_limit_exceeded".to_string());
        }
        if diff_bytes > limits.max_diff_bytes {
            limit_violations.push("diff_size_limit_exceeded".to_string());
        }
        limit_violations.extend(
            command_logs
                .iter()
                .flat_map(|log| log.limit_violations.iter().cloned()),
        );
        Ok(SandboxCapture {
            diff,
            diff_bytes,
            changed_files,
            artifacts,
            command_logs,
            limit_violations,
        })
    }

    pub fn cleanup_workspace(&self, workspace: &SandboxWorkspace) -> io::Result<()> {
        self.ensure_safe_workspace_path(&workspace.repo_root, &workspace.worktree_path)?;
        let _ = run_git_checked_owned(
            &workspace.repo_root,
            &[
                "worktree".to_string(),
                "remove".to_string(),
                "--force".to_string(),
                path_arg(&workspace.worktree_path),
            ],
        );
        if workspace.worktree_path.exists() {
            fs::remove_dir_all(&workspace.worktree_path)?;
        }
        if workspace.branch.starts_with("memory/loops/") && !is_protected_branch(&workspace.branch)
        {
            let _ = run_git_checked(&workspace.repo_root, &["branch", "-D", &workspace.branch]);
        }
        Ok(())
    }

    pub fn cleanup_abandoned_workspaces(
        &self,
        repo_root: &Path,
        older_than: Duration,
    ) -> io::Result<Vec<SandboxWorkspace>> {
        let repo_root = fs::canonicalize(repo_root)?;
        let sandbox_root = self.sandbox_root(&repo_root);
        if !sandbox_root.exists() {
            return Ok(Vec::new());
        }
        let now = SystemTime::now();
        let mut cleaned = Vec::new();
        for entry in fs::read_dir(sandbox_root)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let modified = entry
                .metadata()?
                .modified()
                .unwrap_or(SystemTime::UNIX_EPOCH);
            if now.duration_since(modified).unwrap_or_default() < older_than {
                continue;
            }
            let Some(workspace) = read_metadata(&path)? else {
                continue;
            };
            self.cleanup_workspace(&workspace)?;
            cleaned.push(workspace);
        }
        Ok(cleaned)
    }

    fn sandbox_root(&self, repo_root: &Path) -> PathBuf {
        self.root_dir.clone().unwrap_or_else(|| {
            repo_root
                .join(".mem")
                .join("runtime")
                .join("loop-worktrees")
        })
    }

    fn write_metadata(&self, workspace: &SandboxWorkspace) -> io::Result<()> {
        let payload = serde_json::to_string_pretty(workspace).map_err(io::Error::other)?;
        fs::write(
            workspace.worktree_path.join(".memory-loop-sandbox.json"),
            payload,
        )
    }

    fn ensure_safe_workspace_path(&self, repo_root: &Path, worktree_path: &Path) -> io::Result<()> {
        let sandbox_root = self.sandbox_root(repo_root);
        let expected_root = fs::canonicalize(&sandbox_root).unwrap_or(sandbox_root);
        let candidate = if worktree_path.exists() {
            fs::canonicalize(worktree_path)?
        } else {
            worktree_path.to_path_buf()
        };
        if !candidate.starts_with(&expected_root) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "refusing to clean a path outside the loop sandbox root",
            ));
        }
        Ok(())
    }
}

fn read_metadata(path: &Path) -> io::Result<Option<SandboxWorkspace>> {
    let metadata_path = path.join(".memory-loop-sandbox.json");
    if !metadata_path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(metadata_path)?;
    let workspace = serde_json::from_str(&text).map_err(io::Error::other)?;
    Ok(Some(workspace))
}

fn changed_files(worktree_path: &Path) -> io::Result<Vec<RunnerChangedFile>> {
    let status = run_git_text(worktree_path, &["status", "--porcelain"])?;
    Ok(status
        .lines()
        .filter_map(parse_status_line)
        .collect::<Vec<_>>())
}

fn parse_status_line(line: &str) -> Option<RunnerChangedFile> {
    if line.len() < 4 {
        return None;
    }
    let status = line[..2].trim();
    let mut path = line[3..].trim();
    if let Some((_, new_path)) = path.split_once(" -> ") {
        path = new_path.trim();
    }
    if path.is_empty() {
        return None;
    }
    let change_type = match status.chars().next()? {
        'A' | '?' => "added",
        'D' => "deleted",
        'R' => "renamed",
        _ => "modified",
    };
    Some(RunnerChangedFile {
        path: path.to_string(),
        change_type: change_type.to_string(),
    })
}

fn collect_artifacts(worktree_path: &Path) -> io::Result<Vec<RunnerArtifact>> {
    let artifact_root = worktree_path.join(".mem").join("loop-artifacts");
    if !artifact_root.exists() {
        return Ok(Vec::new());
    }
    let mut artifacts = Vec::new();
    for entry in fs::read_dir(artifact_root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            artifacts.push(RunnerArtifact {
                path: path_arg(&path),
                artifact_type: path
                    .extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or("file")
                    .to_string(),
                summary: None,
            });
        }
    }
    Ok(artifacts)
}

fn sandbox_branch_name(project: &str, run_id: Uuid) -> String {
    format!(
        "memory/loops/{}/{}",
        sanitize_branch_fragment(project),
        run_id.simple()
    )
}

fn sanitize_branch_fragment(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    sanitized.trim_matches('-').to_string()
}

fn is_protected_branch(branch: &str) -> bool {
    matches!(branch, "main" | "master" | "trunk")
}

fn run_git_checked(repo_root: &Path, args: &[&str]) -> io::Result<()> {
    run_git_checked_owned(
        repo_root,
        &args
            .iter()
            .map(|arg| (*arg).to_string())
            .collect::<Vec<_>>(),
    )
}

fn run_git_checked_owned(repo_root: &Path, args: &[String]) -> io::Result<()> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_root)
        .output()?;
    if output.status.success() {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    )))
}

fn run_git_text(repo_root: &Path, args: &[&str]) -> io::Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_root)
        .output()?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).to_string());
    }
    Err(io::Error::other(format!(
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    )))
}

fn path_arg(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("mem-loop-sandbox-{name}-{}", Uuid::new_v4()))
    }

    fn init_repo(name: &str) -> PathBuf {
        let repo = temp_path(name);
        fs::create_dir_all(&repo).unwrap();
        run_git_checked(&repo, &["init", "-b", "main"]).unwrap();
        fs::write(repo.join("README.md"), "initial\n").unwrap();
        run_git_checked(&repo, &["add", "README.md"]).unwrap();
        run_git_checked(
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
        )
        .unwrap();
        repo
    }

    #[test]
    fn worktree_sandbox_creates_captures_and_cleans_workspace() {
        let repo = init_repo("capture");
        let manager = WorktreeSandboxManager::default();
        let spec = SandboxWorkspaceSpec {
            project: "Memory Layer".to_string(),
            repo_root: repo.clone(),
            run_id: Uuid::new_v4(),
            base_ref: None,
        };
        let workspace = manager.create_workspace(&spec).unwrap();
        assert!(workspace.worktree_path.exists());
        assert!(workspace.branch.starts_with("memory/loops/memory-layer/"));
        assert_ne!(workspace.branch, "main");

        let limits = SandboxLimits {
            allowed_commands: vec!["sh".to_string()],
            ..SandboxLimits::default()
        };
        let log = manager
            .run_command(
                &workspace,
                &SandboxCommandRequest::new("sh", ["-c", "printf changed >> README.md"]),
                &limits,
            )
            .unwrap();
        assert_eq!(log.exit_code, 0);

        let capture = manager
            .capture_workspace(&workspace, vec![log], &limits)
            .unwrap();
        assert!(capture.diff.contains("changed"));
        assert_eq!(capture.changed_files[0].path, "README.md");
        assert!(capture.limit_violations.is_empty());
        assert_eq!(
            workspace.runner_workspace_ref().branch.as_deref(),
            Some(workspace.branch.as_str())
        );

        manager.cleanup_workspace(&workspace).unwrap();
        assert!(!workspace.worktree_path.exists());
        let _ = fs::remove_dir_all(repo);
    }

    #[test]
    fn sandbox_blocks_disallowed_commands() {
        let repo = init_repo("blocked");
        let manager = WorktreeSandboxManager::default();
        let workspace = manager
            .create_workspace(&SandboxWorkspaceSpec {
                project: "memory".to_string(),
                repo_root: repo.clone(),
                run_id: Uuid::new_v4(),
                base_ref: None,
            })
            .unwrap();
        let limits = SandboxLimits {
            allowed_commands: vec!["cargo test".to_string()],
            ..SandboxLimits::default()
        };
        let log = manager
            .run_command(
                &workspace,
                &SandboxCommandRequest::new("sh", ["-c", "echo nope"]),
                &limits,
            )
            .unwrap();

        assert_eq!(log.exit_code, -1);
        assert!(
            log.limit_violations
                .contains(&"command_not_allowed".to_string())
        );
        manager.cleanup_workspace(&workspace).unwrap();
        let _ = fs::remove_dir_all(repo);
    }

    #[test]
    fn sandbox_reports_diff_and_file_limit_violations() {
        let repo = init_repo("limits");
        let manager = WorktreeSandboxManager::default();
        let workspace = manager
            .create_workspace(&SandboxWorkspaceSpec {
                project: "memory".to_string(),
                repo_root: repo.clone(),
                run_id: Uuid::new_v4(),
                base_ref: None,
            })
            .unwrap();
        fs::write(workspace.worktree_path.join("README.md"), "changed\n").unwrap();
        let limits = SandboxLimits {
            max_changed_files: 0,
            max_diff_bytes: 1,
            ..SandboxLimits::default()
        };
        let capture = manager
            .capture_workspace(&workspace, Vec::new(), &limits)
            .unwrap();

        assert!(
            capture
                .limit_violations
                .contains(&"changed_file_limit_exceeded".to_string())
        );
        assert!(
            capture
                .limit_violations
                .contains(&"diff_size_limit_exceeded".to_string())
        );
        manager.cleanup_workspace(&workspace).unwrap();
        let _ = fs::remove_dir_all(repo);
    }
}
