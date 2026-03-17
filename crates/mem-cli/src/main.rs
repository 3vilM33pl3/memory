mod tui;

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
};

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use mem_api::{
    AppConfig, ArchiveRequest, ArchiveResponse, CaptureTaskRequest, CurateRequest, CurateResponse,
    MemoryEntryResponse, ProjectMemoriesResponse, ProjectOverviewResponse, QueryFilters,
    QueryRequest, QueryResponse, ReindexRequest, ReindexResponse, TestResult,
};
use mem_watch::{flush_path, load_state, run_once, to_status};
use reqwest::{Client, header::HeaderMap};

#[derive(Debug, Parser)]
#[command(name = "memctl")]
struct Cli {
    #[arg(long, env = "MEMORY_LAYER_CONFIG")]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Init(InitArgs),
    Query(QueryArgs),
    CaptureTask(CaptureTaskArgs),
    Remember(RememberArgs),
    Curate(CurateArgs),
    Reindex(ProjectArgs),
    Health,
    Stats,
    Archive(ArchiveArgs),
    Automation(AutomationArgs),
    Tui(TuiArgs),
}

#[derive(Debug, Args)]
struct InitArgs {
    #[arg(long)]
    project: Option<String>,
    #[arg(long)]
    force: bool,
    #[arg(long)]
    print: bool,
}

#[derive(Debug, Args)]
struct QueryArgs {
    #[arg(long)]
    project: String,
    #[arg(long)]
    question: String,
    #[arg(long = "type")]
    types: Vec<String>,
    #[arg(long = "tag")]
    tags: Vec<String>,
    #[arg(long, default_value_t = 8)]
    limit: i64,
    #[arg(long)]
    min_confidence: Option<f32>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct CaptureTaskArgs {
    #[arg(long)]
    file: PathBuf,
}

#[derive(Debug, Args)]
struct RememberArgs {
    #[arg(long)]
    project: Option<String>,
    #[arg(long)]
    title: Option<String>,
    #[arg(long)]
    prompt: Option<String>,
    #[arg(long)]
    summary: Option<String>,
    #[arg(long = "note")]
    notes: Vec<String>,
    #[arg(long = "file-changed")]
    files_changed: Vec<String>,
    #[arg(long = "test-passed")]
    tests_passed: Vec<String>,
    #[arg(long = "test-failed")]
    tests_failed: Vec<String>,
    #[arg(long)]
    command_output_file: Option<PathBuf>,
    #[arg(long, default_value_t = true)]
    auto_files: bool,
}

#[derive(Debug, Args)]
struct CurateArgs {
    #[arg(long)]
    project: String,
    #[arg(long)]
    batch_size: Option<i64>,
}

#[derive(Debug, Args)]
struct ProjectArgs {
    #[arg(long)]
    project: String,
}

#[derive(Debug, Args)]
struct ArchiveArgs {
    #[arg(long)]
    project: String,
    #[arg(long, default_value_t = 0.3)]
    max_confidence: f32,
    #[arg(long, default_value_t = 1)]
    max_importance: i32,
}

#[derive(Debug, Args)]
struct TuiArgs {
    #[arg(long)]
    project: Option<String>,
}

#[derive(Debug, Args)]
struct AutomationArgs {
    #[command(subcommand)]
    command: AutomationCommand,
}

#[derive(Debug, Subcommand)]
enum AutomationCommand {
    Status(ProjectArgs),
    Flush(ProjectArgs),
}

#[tokio::main]
async fn main() -> Result<()> {
    let Cli {
        config: cli_config,
        command,
    } = Cli::parse();

    if let Command::Init(args) = &command {
        let cwd = env::current_dir().context("read current directory")?;
        let project = resolve_project_slug(args.project.clone(), &cwd)?;
        let repo_root = resolve_repo_root(&cwd)?;
        let output = initialize_repo(&repo_root, &project, args.force, args.print)?;
        println!("{output}");
        return Ok(());
    }

    let config = AppConfig::load_from_path(cli_config).context("load config")?;
    let client = Client::builder()
        .timeout(config.service.request_timeout)
        .build()
        .context("build http client")?;

    match command {
        Command::Init(_) => unreachable!("init is handled before config loading"),
        Command::Query(args) => {
            let request = QueryRequest {
                project: args.project,
                query: args.question,
                filters: QueryFilters {
                    types: args
                        .types
                        .into_iter()
                        .map(parse_memory_type)
                        .collect::<Result<Vec<_>>>()?,
                    tags: args.tags,
                },
                top_k: args.limit,
                min_confidence: args.min_confidence,
            };
            let payload: QueryResponse = get_json(
                client
                    .post(service_url(&config, "/v1/query"))
                    .json(&request)
                    .send()
                    .await
                    .context("query request failed")?,
            )
            .await?;
            if args.json {
                println!("{}", serde_json::to_string(&payload)?);
            } else {
                print_query_response(payload);
            }
        }
        Command::CaptureTask(args) => {
            let request: CaptureTaskRequest =
                serde_json::from_str(&fs::read_to_string(args.file).context("read payload file")?)?;
            let response = client
                .post(service_url(&config, "/v1/capture/task"))
                .headers(write_headers(&config.service.api_token)?)
                .json(&request)
                .send()
                .await?;
            print_json_response(response).await?;
        }
        Command::Remember(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            let project = resolve_project_slug(args.project.clone(), &cwd)?;
            let request = build_remember_request(args, &project)?;
            let api = ApiClient::new(client, config);
            let capture = api.capture_task(&request).await?;
            let curate = api.curate(&project).await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "capture": capture,
                    "curate": curate
                }))?
            );
        }
        Command::Curate(args) => {
            let response = client
                .post(service_url(&config, "/v1/curate"))
                .headers(write_headers(&config.service.api_token)?)
                .json(&CurateRequest {
                    project: args.project,
                    batch_size: args.batch_size,
                })
                .send()
                .await?;
            print_json_response(response).await?;
        }
        Command::Reindex(args) => {
            let response = client
                .post(service_url(&config, "/v1/reindex"))
                .headers(write_headers(&config.service.api_token)?)
                .json(&ReindexRequest {
                    project: args.project,
                })
                .send()
                .await?;
            print_json_response(response).await?;
        }
        Command::Health => {
            let response = client.get(service_url(&config, "/healthz")).send().await?;
            print_json_response(response).await?;
        }
        Command::Stats => {
            let response = client.get(service_url(&config, "/v1/stats")).send().await?;
            print_json_response(response).await?;
        }
        Command::Archive(args) => {
            let response = client
                .post(service_url(&config, "/v1/archive"))
                .headers(write_headers(&config.service.api_token)?)
                .json(&ArchiveRequest {
                    project: args.project,
                    max_confidence: args.max_confidence,
                    max_importance: args.max_importance,
                })
                .send()
                .await?;
            print_json_response(response).await?;
        }
        Command::Automation(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            match args.command {
                AutomationCommand::Status(args) => {
                    let project = resolve_project_slug(Some(args.project), &cwd)?;
                    let repo_root = config
                        .automation
                        .repo_root
                        .as_ref()
                        .map(PathBuf::from)
                        .unwrap_or(cwd);
                    let state = load_state(&project, &repo_root, &config.automation).await?;
                    println!("{}", serde_json::to_string_pretty(&to_status(&state))?);
                }
                AutomationCommand::Flush(args) => {
                    let project = resolve_project_slug(Some(args.project), &cwd)?;
                    let repo_root = config
                        .automation
                        .repo_root
                        .as_ref()
                        .map(PathBuf::from)
                        .unwrap_or(cwd);
                    let api = ApiClient::new(client.clone(), config.clone());
                    tokio::fs::write(flush_path(&repo_root), b"flush\n")
                        .await
                        .ok();
                    run_once(&api.config, &api.client, &project, &repo_root, true).await?;
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "project": project,
                            "status": "flush_requested"
                        }))?
                    );
                }
            }
        }
        Command::Tui(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            let project = resolve_project_slug(args.project, &cwd)?;
            let api = ApiClient::new(client, config);
            tui::run(api, project).await?;
        }
    }

    Ok(())
}

fn initialize_repo(repo_root: &Path, project: &str, force: bool, print_only: bool) -> Result<String> {
    let mem_dir = repo_root.join(".mem");
    let runtime_dir = mem_dir.join("runtime");
    let config_path = mem_dir.join("config.toml");
    let project_path = mem_dir.join("project.toml");
    let local_gitignore_path = mem_dir.join(".gitignore");
    let root_gitignore_path = repo_root.join(".gitignore");

    if !force {
        for path in [&config_path, &project_path] {
            if path.exists() {
                anyhow::bail!(
                    "{} already exists; rerun with --force to overwrite generated files",
                    path.display()
                );
            }
        }
    }

    let config_contents = render_repo_config(repo_root);
    let project_contents = render_project_metadata(project, repo_root);
    let mem_gitignore_contents = "runtime/\n";
    let root_gitignore_line = "/.mem\n";

    if !print_only {
        fs::create_dir_all(&runtime_dir).context("create .mem/runtime")?;
        fs::write(&config_path, config_contents).context("write .mem/config.toml")?;
        fs::write(&project_path, project_contents).context("write .mem/project.toml")?;
        fs::write(&local_gitignore_path, mem_gitignore_contents).context("write .mem/.gitignore")?;
        ensure_root_gitignore_entry(&root_gitignore_path, root_gitignore_line)?;
    }

    Ok(render_init_summary(
        repo_root,
        project,
        &config_path,
        &project_path,
        print_only,
    ))
}

fn render_repo_config(repo_root: &Path) -> String {
    let repo_root = repo_root.display();
    format!(
        r#"# Fill in the values below before starting the backend.
# Secrets are kept local to this repo because `.mem/` is ignored by git.

[service]
bind_addr = "127.0.0.1:4040"
api_token = "dev-memory-token"
request_timeout = "30s"

[database]
url = "postgresql://memory:<password>@localhost:5432/memory"

[features]
llm_curation = false

[automation]
enabled = false
mode = "suggest"
repo_root = "{repo_root}"
poll_interval = "10s"
idle_threshold = "5m"
min_changed_files = 2
require_passing_test = false
ignored_paths = [".git/", "target/", ".mem/"]
audit_log_path = "{repo_root}/.mem/runtime/automation.log"
state_file_path = "{repo_root}/.mem/runtime/automation-state.json"
"#
    )
}

fn render_project_metadata(project: &str, repo_root: &Path) -> String {
    format!(
        r#"slug = "{project}"
repo_root = "{}"
"#,
        repo_root.display()
    )
}

fn ensure_root_gitignore_entry(path: &Path, line: &str) -> Result<()> {
    let mut content = if path.exists() {
        fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?
    } else {
        String::new()
    };

    if !content.lines().any(|existing| existing.trim() == line.trim()) {
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(line);
        fs::write(path, content).with_context(|| format!("write {}", path.display()))?;
    }

    Ok(())
}

fn render_init_summary(
    repo_root: &Path,
    project: &str,
    config_path: &Path,
    project_path: &Path,
    print_only: bool,
) -> String {
    let action = if print_only { "Would create" } else { "Created" };
    format!(
        "{action} repo-local memory bootstrap for project `{project}` at {}.\n\nFiles:\n- {}\n- {}\n- {}/runtime/\n\nNext steps:\n1. Fill in `database.url` and `service.api_token` in {}\n2. Start the backend:\n   mem-service {}\n3. Optional: start the watcher:\n   memory-watch --config {} run --project {}\n4. Open the TUI:\n   mem-cli --config {} tui --project {}",
        repo_root.display(),
        config_path.display(),
        project_path.display(),
        config_path.parent().unwrap_or(repo_root).display(),
        config_path.display(),
        config_path.display(),
        config_path.display(),
        project,
        config_path.display(),
        project
    )
}

fn resolve_repo_root(cwd: &Path) -> Result<PathBuf> {
    let output = ProcessCommand::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output();

    if let Ok(output) = output {
        if output.status.success() {
            let stdout = String::from_utf8(output.stdout).context("decode git rev-parse output")?;
            let root = stdout.trim();
            if !root.is_empty() {
                return Ok(PathBuf::from(root));
            }
        }
    }

    Ok(cwd.to_path_buf())
}

#[derive(Clone)]
pub(crate) struct ApiClient {
    client: Client,
    config: AppConfig,
}

impl ApiClient {
    pub(crate) fn new(client: Client, config: AppConfig) -> Self {
        Self { client, config }
    }

    pub(crate) async fn health(&self) -> Result<serde_json::Value> {
        get_json(
            self.client
                .get(service_url(&self.config, "/healthz"))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn project_memories(&self, project: &str) -> Result<ProjectMemoriesResponse> {
        get_json(
            self.client
                .get(service_url(
                    &self.config,
                    &format!("/v1/projects/{project}/memories"),
                ))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn project_overview(&self, project: &str) -> Result<ProjectOverviewResponse> {
        get_json(
            self.client
                .get(service_url(
                    &self.config,
                    &format!("/v1/projects/{project}/overview"),
                ))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn memory_detail(&self, memory_id: &str) -> Result<MemoryEntryResponse> {
        get_json(
            self.client
                .get(service_url(
                    &self.config,
                    &format!("/v1/memory/{memory_id}"),
                ))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn capture_task(
        &self,
        request: &CaptureTaskRequest,
    ) -> Result<mem_api::CaptureTaskResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/capture/task"))
                .headers(write_headers(&self.config.service.api_token)?)
                .json(request)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn curate(&self, project: &str) -> Result<CurateResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/curate"))
                .headers(write_headers(&self.config.service.api_token)?)
                .json(&CurateRequest {
                    project: project.to_string(),
                    batch_size: None,
                })
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn reindex(&self, project: &str) -> Result<ReindexResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/reindex"))
                .headers(write_headers(&self.config.service.api_token)?)
                .json(&ReindexRequest {
                    project: project.to_string(),
                })
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn archive_low_value(&self, project: &str) -> Result<ArchiveResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/archive"))
                .headers(write_headers(&self.config.service.api_token)?)
                .json(&ArchiveRequest {
                    project: project.to_string(),
                    max_confidence: 0.3,
                    max_importance: 1,
                })
                .send()
                .await?,
        )
        .await
    }
}

async fn get_json<T: serde::de::DeserializeOwned>(response: reqwest::Response) -> Result<T> {
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        anyhow::bail!("{status} {body}");
    }
    Ok(serde_json::from_str(&body)?)
}

async fn print_json_response(response: reqwest::Response) -> Result<()> {
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        anyhow::bail!("{status} {body}");
    }
    println!("{body}");
    Ok(())
}

fn print_query_response(payload: QueryResponse) {
    println!("Answer:\n{}\n", payload.answer);
    println!("Confidence: {:.2}\n", payload.confidence);
    for result in payload.results {
        println!(
            "- {} [{}] score={:.2}",
            result.summary, result.memory_type, result.score
        );
        println!("  {}", result.snippet);
        for source in result.sources {
            let path = source.file_path.unwrap_or_else(|| "<no-file>".to_string());
            println!(
                "  source: {} {}",
                path,
                source.source_kind.source_kind_string()
            );
        }
    }
}

fn parse_memory_type(input: String) -> Result<mem_api::MemoryType> {
    match input.as_str() {
        "architecture" => Ok(mem_api::MemoryType::Architecture),
        "convention" => Ok(mem_api::MemoryType::Convention),
        "decision" => Ok(mem_api::MemoryType::Decision),
        "incident" => Ok(mem_api::MemoryType::Incident),
        "debugging" => Ok(mem_api::MemoryType::Debugging),
        "environment" => Ok(mem_api::MemoryType::Environment),
        "domain_fact" => Ok(mem_api::MemoryType::DomainFact),
        _ => anyhow::bail!("unknown memory type: {input}"),
    }
}

fn write_headers(token: &str) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert("x-api-token", token.parse()?);
    Ok(headers)
}

fn service_url(config: &AppConfig, path: &str) -> String {
    format!("http://{}{}", config.service.bind_addr, path)
}

fn resolve_project_slug(project: Option<String>, cwd: &Path) -> Result<String> {
    if let Some(project) = project {
        return Ok(project);
    }
    let Some(name) = cwd.file_name().and_then(|value| value.to_str()) else {
        anyhow::bail!("could not determine project slug from current directory");
    };
    Ok(name.to_string())
}

fn build_remember_request(args: RememberArgs, project: &str) -> Result<CaptureTaskRequest> {
    let mut files_changed = args.files_changed;
    if args.auto_files {
        for file in detect_changed_files()? {
            if !files_changed.contains(&file) {
                files_changed.push(file);
            }
        }
    }

    let command_output = match args.command_output_file {
        Some(path) => Some(fs::read_to_string(path).context("read command output file")?),
        None => None,
    };

    let tests = args
        .tests_passed
        .into_iter()
        .map(|command| TestResult {
            command,
            status: "passed".to_string(),
            output: None,
        })
        .chain(args.tests_failed.into_iter().map(|command| TestResult {
            command,
            status: "failed".to_string(),
            output: None,
        }))
        .collect();

    let title = args
        .title
        .unwrap_or_else(|| format!("Memory update for {project}"));
    let prompt = args
        .prompt
        .unwrap_or_else(|| format!("Auto-captured repository work in project {project}."));
    let summary = args
        .summary
        .unwrap_or_else(|| derive_summary(project, &files_changed));

    Ok(CaptureTaskRequest {
        project: project.to_string(),
        task_title: title,
        user_prompt: prompt,
        agent_summary: summary,
        files_changed,
        git_diff_summary: None,
        tests,
        notes: args.notes,
        command_output,
        idempotency_key: None,
    })
}

fn derive_summary(project: &str, files_changed: &[String]) -> String {
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

fn detect_changed_files() -> Result<Vec<String>> {
    let inside_repo = ProcessCommand::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .output();

    let Ok(output) = inside_repo else {
        return Ok(Vec::new());
    };
    if !output.status.success() {
        return Ok(Vec::new());
    }

    let output = ProcessCommand::new("git")
        .args(["status", "--porcelain"])
        .output()
        .context("run git status --porcelain")?;
    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8(output.stdout).context("decode git status output")?;
    let mut files = Vec::new();
    for line in stdout.lines() {
        if line.len() < 4 {
            continue;
        }
        let path = line[3..].trim();
        if path.is_empty() {
            continue;
        }
        let normalized = if let Some((_, new_path)) = path.split_once(" -> ") {
            new_path.to_string()
        } else {
            path.to_string()
        };
        if !files.contains(&normalized) {
            files.push(normalized);
        }
    }
    Ok(files)
}

pub(crate) trait SourceKindString {
    fn source_kind_string(&self) -> &'static str;
}

impl SourceKindString for mem_api::SourceKind {
    fn source_kind_string(&self) -> &'static str {
        match self {
            mem_api::SourceKind::TaskPrompt => "task_prompt",
            mem_api::SourceKind::File => "file",
            mem_api::SourceKind::GitCommit => "git_commit",
            mem_api::SourceKind::CommandOutput => "command_output",
            mem_api::SourceKind::Test => "test",
            mem_api::SourceKind::Note => "note",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::{
        RememberArgs, build_remember_request, initialize_repo, resolve_project_slug,
        resolve_repo_root,
    };

    #[test]
    fn project_flag_wins() {
        let cwd = PathBuf::from("/tmp/example");
        assert_eq!(
            resolve_project_slug(Some("override".to_string()), &cwd).unwrap(),
            "override"
        );
    }

    #[test]
    fn project_defaults_to_cwd_name() {
        let cwd = PathBuf::from("/tmp/memory");
        assert_eq!(resolve_project_slug(None, &cwd).unwrap(), "memory");
    }

    #[test]
    fn remember_request_uses_defaults() {
        let request = build_remember_request(
            RememberArgs {
                project: None,
                title: None,
                prompt: None,
                summary: None,
                notes: vec!["durable fact".to_string()],
                files_changed: vec!["src/main.rs".to_string()],
                tests_passed: vec![],
                tests_failed: vec![],
                command_output_file: None,
                auto_files: false,
            },
            "memory",
        )
        .unwrap();

        assert_eq!(request.task_title, "Memory update for memory");
        assert!(request.user_prompt.contains("Auto-captured"));
        assert!(request.agent_summary.contains("src/main.rs"));
    }

    #[test]
    fn init_print_describes_repo_layout() {
        let repo_root = PathBuf::from("/tmp/memory");
        let summary = initialize_repo(&repo_root, "memory", false, true).unwrap();

        assert!(summary.contains(".mem/config.toml"));
        assert!(summary.contains("memory-watch --config /tmp/memory/.mem/config.toml run --project memory"));
    }

    #[test]
    fn init_creates_repo_files_and_gitignore_entry() {
        let repo_root = unique_temp_dir("mem-init");
        fs::create_dir_all(&repo_root).unwrap();

        initialize_repo(&repo_root, "memory", false, false).unwrap();

        assert!(repo_root.join(".mem/config.toml").is_file());
        assert!(repo_root.join(".mem/project.toml").is_file());
        assert!(repo_root.join(".mem/runtime").is_dir());
        assert_eq!(
            fs::read_to_string(repo_root.join(".mem/.gitignore")).unwrap(),
            "runtime/\n"
        );
        assert!(
            fs::read_to_string(repo_root.join(".gitignore"))
                .unwrap()
                .contains("/.mem")
        );

        let _ = fs::remove_dir_all(repo_root);
    }

    #[test]
    fn resolve_repo_root_falls_back_to_cwd() {
        let cwd = PathBuf::from("/tmp/not-a-repo");
        assert_eq!(resolve_repo_root(&cwd).unwrap(), cwd);
    }

    fn unique_temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        if path.exists() {
            let _ = fs::remove_dir_all(&path);
        }
        path
    }
}
