mod commits;
mod resume;
mod scan;
mod tui;
mod wizard;

#[cfg(target_os = "macos")]
use std::collections::BTreeMap;
#[cfg(unix)]
use std::os::unix::{fs::PermissionsExt, net::UnixStream};
use std::{
    env, fs,
    io::Read,
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    time::Duration,
};

use anyhow::{Context, Result};
use chrono::Utc;
use clap::{Args, Parser, Subcommand};
use mem_api::{
    AppConfig, ArchiveRequest, ArchiveResponse, CaptureTaskRequest, CheckpointActivityRequest,
    CommitDetailResponse, CommitSyncRequest, CommitSyncResponse, CurateRequest, CurateResponse,
    DeleteMemoryRequest, DeleteMemoryResponse, MemoryEntryResponse, PlanActivityAction,
    PlanActivityRequest, ProjectCommitsResponse, ProjectMemoriesResponse,
    ProjectMemoryBundlePreview, ProjectMemoryExportOptions, ProjectMemoryImportPreview,
    ProjectMemoryImportResponse, ProjectOverviewResponse, PruneEmbeddingsRequest,
    PruneEmbeddingsResponse, QueryFilters, QueryRequest, QueryResponse, ReembedRequest,
    ReembedResponse, ReindexRequest, ReindexResponse, ReplacementPolicy, ResumeRequest,
    ResumeResponse, ScanActivityRequest, TestResult, discover_global_config_path,
    discover_repo_env_path, load_repo_replacement_policy, read_repo_project_slug,
};
use mem_platform as platform;
use mem_service as service_runtime;
use mem_watch::{WatcherRunArgs, run_watcher_daemon};
use mem_watch::{
    build_capture_request as build_automation_capture_request,
    detect_changed_files as watch_detect_changed_files,
    fetch_project_overview as fetch_automation_overview, flush_path, load_state, run_once,
    should_capture, should_curate, to_status, update_session_from_repo,
};
use reqwest::{
    Client,
    header::{HeaderMap, ORIGIN},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{Row, postgres::PgPoolOptions};
use uuid::Uuid;

const ROOT_AFTER_HELP: &str = "\
Examples:
  memory wizard --global
  memory query --project memory --question \"What changed recently?\"
  memory remember --project memory --note \"Durable fact\"

See also:
  docs/user/README.md";

const WIZARD_AFTER_HELP: &str = "\
Examples:
  memory wizard --global
  memory wizard
  memory wizard --project memory --dry-run

See also:
  docs/user/cli/wizard.md";

const INIT_AFTER_HELP: &str = "\
Examples:
  memory init
  memory init --project memory --dry-run
  memory init --force

See also:
  docs/user/cli/init.md";

const SERVICE_GROUP_AFTER_HELP: &str = "\
Examples:
  memory service run
  memory service enable --dry-run
  memory service status

See also:
  docs/user/cli/service.md";

const SERVICE_RUN_AFTER_HELP: &str = "\
Examples:
  memory service run

See also:
  docs/user/cli/service.md";

const SERVICE_ENABLE_AFTER_HELP: &str = "\
Examples:
  memory service enable
  memory service enable --dry-run

See also:
  docs/user/cli/service.md";

const SERVICE_DISABLE_AFTER_HELP: &str = "\
Examples:
  memory service disable
  memory service disable --dry-run

See also:
  docs/user/cli/service.md";

const SERVICE_STATUS_AFTER_HELP: &str = "\
Examples:
  memory service status

See also:
  docs/user/cli/service.md";

const SERVICE_TOKEN_AFTER_HELP: &str = "\
Examples:
  memory service ensure-api-token --shared
  memory service ensure-api-token --rotate-placeholder --dry-run

See also:
  docs/user/cli/service.md";

const DOCTOR_AFTER_HELP: &str = "\
Examples:
  memory doctor
  memory doctor --project memory
  memory doctor --json

See also:
  docs/user/cli/doctor.md";

const WATCHER_GROUP_AFTER_HELP: &str = "\
Examples:
  memory watcher run --project memory
  memory watcher enable --project memory
  memory watcher status --project memory

See also:
  docs/user/cli/watchers.md";

const WATCHER_RUN_AFTER_HELP: &str = "\
Examples:
  memory watcher run --project memory
  memory watcher run --repo-root /path/to/repo

See also:
  docs/user/cli/watchers.md";

const WATCHER_ENABLE_AFTER_HELP: &str = "\
Examples:
  memory watcher enable --project memory
  memory watcher enable --project memory --dry-run

See also:
  docs/user/cli/watchers.md";

const WATCHER_DISABLE_AFTER_HELP: &str = "\
Examples:
  memory watcher disable --project memory
  memory watcher disable --project memory --dry-run

See also:
  docs/user/cli/watchers.md";

const WATCHER_STATUS_AFTER_HELP: &str = "\
Examples:
  memory watcher status --project memory

See also:
  docs/user/cli/watchers.md";

const QUERY_AFTER_HELP: &str = "\
Examples:
  memory query --project memory --question \"How does resume work?\"
  memory query --project memory --question \"What changed?\" --type plan --tag plan

See also:
  docs/user/cli/query.md";

const COMMITS_GROUP_AFTER_HELP: &str = "\
Examples:
  memory commits sync --project memory
  memory commits list --project memory
  memory commits show <commit> --project memory

See also:
  docs/user/cli/commits.md";

const COMMITS_SYNC_AFTER_HELP: &str = "\
Examples:
  memory commits sync --project memory
  memory commits sync --project memory --since 2026-04-01 --dry-run

See also:
  docs/user/cli/commits.md";

const COMMITS_LIST_AFTER_HELP: &str = "\
Examples:
  memory commits list --project memory
  memory commits list --project memory --limit 50 --json

See also:
  docs/user/cli/commits.md";

const COMMITS_SHOW_AFTER_HELP: &str = "\
Examples:
  memory commits show abc123 --project memory

See also:
  docs/user/cli/commits.md";

const REPO_GROUP_AFTER_HELP: &str = "\
Examples:
  memory repo index --project memory
  memory repo status --project memory

See also:
  docs/user/cli/repo.md";

const REPO_INDEX_AFTER_HELP: &str = "\
Examples:
  memory repo index --project memory
  memory repo index --project memory --since 2026-04-01 --dry-run

See also:
  docs/user/cli/repo.md";

const REPO_STATUS_AFTER_HELP: &str = "\
Examples:
  memory repo status --project memory
  memory repo status --project memory --json

See also:
  docs/user/cli/repo.md";

const BUNDLE_GROUP_AFTER_HELP: &str = "\
Examples:
  memory bundle export --project memory --out /tmp/memory.mlbundle.zip
  memory bundle import --project memory /tmp/memory.mlbundle.zip --dry-run

See also:
  docs/user/cli/bundles.md";

const BUNDLE_EXPORT_AFTER_HELP: &str = "\
Examples:
  memory bundle export --project memory --out /tmp/memory.mlbundle.zip
  memory bundle export --project memory --out /tmp/memory.mlbundle.zip --dry-run

See also:
  docs/user/cli/bundles.md";

const BUNDLE_IMPORT_AFTER_HELP: &str = "\
Examples:
  memory bundle import --project memory /tmp/memory.mlbundle.zip
  memory bundle import --project memory /tmp/memory.mlbundle.zip --dry-run --json

See also:
  docs/user/cli/bundles.md";

const RESUME_AFTER_HELP: &str = "\
Examples:
  memory resume --project memory
  memory resume --project memory --json

See also:
  docs/user/cli/resume.md";

const CHECKPOINT_GROUP_AFTER_HELP: &str = "\
Examples:
  memory checkpoint save --project memory
  memory checkpoint start-execution --project memory --plan-file /tmp/plan.md
  memory checkpoint finish-execution --project memory

See also:
  docs/user/cli/checkpoint.md";

const CHECKPOINT_SAVE_AFTER_HELP: &str = "\
Examples:
  memory checkpoint save --project memory
  memory checkpoint save --project memory --note \"Waiting on review\" --dry-run

See also:
  docs/user/cli/checkpoint.md";

const CHECKPOINT_SHOW_AFTER_HELP: &str = "\
Examples:
  memory checkpoint show --project memory

See also:
  docs/user/cli/checkpoint.md";

const CHECKPOINT_START_AFTER_HELP: &str = "\
Examples:
  memory checkpoint start-execution --project memory --plan-file /tmp/plan.md
  memory checkpoint start-execution --project memory --plan-stdin --thread-key task-123

See also:
  docs/user/cli/checkpoint.md";

const CHECKPOINT_FINISH_AFTER_HELP: &str = "\
Examples:
  memory checkpoint finish-execution --project memory
  memory checkpoint finish-execution --project memory --plan-file /tmp/plan.md --json

See also:
  docs/user/cli/checkpoint.md";

const CAPTURE_GROUP_AFTER_HELP: &str = "\
Examples:
  memory capture task --file /tmp/task.json

See also:
  docs/user/cli/capture.md";

const CAPTURE_TASK_AFTER_HELP: &str = "\
Examples:
  memory capture task --file /tmp/task.json
  memory capture task --file /tmp/task.json --dry-run

See also:
  docs/user/cli/capture.md";

const SCAN_AFTER_HELP: &str = "\
Examples:
  memory scan --project memory
  memory scan --project memory --dry-run
  memory scan --project memory --rebuild-index

See also:
  docs/user/cli/scan.md";

const REMEMBER_AFTER_HELP: &str = "\
Examples:
  memory remember --project memory --note \"Durable fact\"
  memory remember --project memory --title \"Task title\" --summary \"What changed\"

See also:
  docs/user/cli/remember.md";

const CURATE_AFTER_HELP: &str = "\
Examples:
  memory curate --project memory
  memory curate --project memory --batch-size 10 --dry-run

See also:
  docs/user/cli/curate.md";

const EMBEDDINGS_GROUP_AFTER_HELP: &str = "\
Examples:
  memory embeddings reindex --project memory
  memory embeddings reembed --project memory
  memory embeddings prune --project memory --dry-run

See also:
  docs/user/cli/embeddings.md";

const EMBEDDINGS_REINDEX_AFTER_HELP: &str = "\
Examples:
  memory embeddings reindex --project memory
  memory embeddings reindex --project memory --dry-run

See also:
  docs/user/cli/embeddings.md";

const EMBEDDINGS_REEMBED_AFTER_HELP: &str = "\
Examples:
  memory embeddings reembed --project memory
  memory embeddings reembed --project memory --dry-run

See also:
  docs/user/cli/embeddings.md";

const EMBEDDINGS_PRUNE_AFTER_HELP: &str = "\
Examples:
  memory embeddings prune --project memory
  memory embeddings prune --project memory --dry-run

See also:
  docs/user/cli/embeddings.md";

const HEALTH_AFTER_HELP: &str = "\
Examples:
  memory health
  memory stats

See also:
  docs/user/cli/health.md";

const STATS_AFTER_HELP: &str = "\
Examples:
  memory stats
  memory health

See also:
  docs/user/cli/health.md";

const ARCHIVE_AFTER_HELP: &str = "\
Examples:
  memory archive --project memory
  memory archive --project memory --max-confidence 0.2 --dry-run

See also:
  docs/user/cli/archive.md";

const AUTOMATION_GROUP_AFTER_HELP: &str = "\
Examples:
  memory automation status --project memory
  memory automation flush --project memory --curate --dry-run

See also:
  docs/user/cli/automation.md";

const AUTOMATION_STATUS_AFTER_HELP: &str = "\
Examples:
  memory automation status --project memory

See also:
  docs/user/cli/automation.md";

const AUTOMATION_FLUSH_AFTER_HELP: &str = "\
Examples:
  memory automation flush --project memory
  memory automation flush --project memory --curate --dry-run

See also:
  docs/user/cli/automation.md";

const TUI_AFTER_HELP: &str = "\
Examples:
  memory tui
  memory tui --project memory

See also:
  docs/user/tui/README.md";

#[derive(Debug, Parser)]
#[command(
    name = "memory",
    version,
    about = "Project memory CLI for setup, retrieval, capture, curation, and operations.",
    after_help = ROOT_AFTER_HELP
)]
struct Cli {
    /// Use a specific config file instead of the discovered default.
    #[arg(long, env = "MEMORY_LAYER_CONFIG")]
    config: Option<PathBuf>,
    /// Override the writer identity used for write-capable commands.
    #[arg(
        long = "writer-id",
        visible_alias = "agent-id",
        env = "MEMORY_LAYER_WRITER_ID"
    )]
    writer_id: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    #[command(about = "Run the interactive setup wizard.", after_help = WIZARD_AFTER_HELP)]
    Wizard(WizardArgs),
    #[command(about = "Bootstrap a repo-local Memory Layer setup.", after_help = INIT_AFTER_HELP)]
    Init(InitArgs),
    #[command(about = "Manage the Memory Layer backend service.", after_help = SERVICE_GROUP_AFTER_HELP)]
    Service(ServiceArgs),
    #[command(about = "Manage project watchers and watcher daemons.", after_help = WATCHER_GROUP_AFTER_HELP)]
    Watcher(WatcherArgs),
    #[command(about = "Inspect configuration and environment health.", after_help = DOCTOR_AFTER_HELP)]
    Doctor(DoctorArgs),
    #[command(about = "Import and inspect git commit history.", after_help = COMMITS_GROUP_AFTER_HELP)]
    Commits(CommitsArgs),
    #[command(about = "Build and inspect the local repository index.", after_help = REPO_GROUP_AFTER_HELP)]
    Repo(RepoArgs),
    #[command(about = "Export and import shareable project memory bundles.", after_help = BUNDLE_GROUP_AFTER_HELP)]
    Bundle(BundleArgs),
    #[command(about = "Save, inspect, and verify execution checkpoints.", after_help = CHECKPOINT_GROUP_AFTER_HELP)]
    Checkpoint(CheckpointArgs),
    #[command(about = "Generate a resume briefing for a project.", after_help = RESUME_AFTER_HELP)]
    Resume(ResumeArgs),
    #[command(about = "Ask a project-specific question against curated memory.", after_help = QUERY_AFTER_HELP)]
    Query(QueryArgs),
    #[command(about = "Scan a repository for candidate durable memories.", after_help = SCAN_AFTER_HELP)]
    Scan(ScanArgs),
    #[command(about = "Capture structured task context from a file.", after_help = CAPTURE_GROUP_AFTER_HELP)]
    Capture(CaptureArgs),
    #[command(about = "Capture and curate completed work into memory.", after_help = REMEMBER_AFTER_HELP)]
    Remember(RememberArgs),
    #[command(about = "Curate raw captures into canonical memory.", after_help = CURATE_AFTER_HELP)]
    Curate(CurateArgs),
    #[command(about = "Rebuild and maintain embedding spaces.", after_help = EMBEDDINGS_GROUP_AFTER_HELP)]
    Embeddings(EmbeddingsArgs),
    #[command(about = "Check backend service health.", after_help = HEALTH_AFTER_HELP)]
    Health,
    #[command(about = "Show memory and project summary statistics.", after_help = STATS_AFTER_HELP)]
    Stats,
    #[command(about = "Archive low-signal memories by confidence and importance.", after_help = ARCHIVE_AFTER_HELP)]
    Archive(ArchiveArgs),
    #[command(about = "Inspect and flush automation state.", after_help = AUTOMATION_GROUP_AFTER_HELP)]
    Automation(AutomationArgs),
    #[command(about = "Open the terminal UI.", after_help = TUI_AFTER_HELP)]
    Tui(TuiArgs),
}

#[derive(Debug, Args)]
#[command(
    about = "Run the interactive setup wizard for global or repo-local Memory Layer configuration.",
    after_help = WIZARD_AFTER_HELP
)]
struct WizardArgs {
    /// Override the project slug used for repo-local setup.
    #[arg(long)]
    project: Option<String>,
    /// Edit shared machine-level configuration instead of only the current repo.
    #[arg(long)]
    global: bool,
    /// Preview the wizard's file and service actions without applying them.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Create or refresh the repo-local Memory Layer bootstrap files.",
    after_help = INIT_AFTER_HELP
)]
struct InitArgs {
    /// Override the project slug written into the repo-local bootstrap files.
    #[arg(long)]
    project: Option<String>,
    /// Replace existing managed bootstrap files instead of preserving them.
    #[arg(long)]
    force: bool,
    /// Preview the files and skill bundle paths that would be written.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Manage the Memory Layer backend service for local or packaged installs.",
    after_help = SERVICE_GROUP_AFTER_HELP
)]
struct ServiceArgs {
    #[command(subcommand)]
    command: ServiceCommand,
}

#[derive(Debug, Subcommand)]
enum ServiceCommand {
    #[command(about = "Run the backend service in the foreground.", after_help = SERVICE_RUN_AFTER_HELP)]
    Run,
    #[command(about = "Enable and start the packaged backend service.", after_help = SERVICE_ENABLE_AFTER_HELP)]
    Enable(ServiceLifecycleArgs),
    #[command(about = "Disable and stop the packaged backend service.", after_help = SERVICE_DISABLE_AFTER_HELP)]
    Disable(ServiceLifecycleArgs),
    #[command(about = "Show the current packaged service status.", after_help = SERVICE_STATUS_AFTER_HELP)]
    Status,
    #[command(about = "Provision or rotate the shared service API token.", after_help = SERVICE_TOKEN_AFTER_HELP)]
    EnsureApiToken(ServiceEnsureApiTokenArgs),
}

#[derive(Debug, Args)]
struct ServiceLifecycleArgs {
    /// Preview the service manager actions without changing service state.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Args)]
struct ServiceEnsureApiTokenArgs {
    /// Operate on the shared machine-level env file instead of a repo-local override.
    #[arg(long)]
    shared: bool,
    /// Replace the development placeholder token if it is still configured.
    #[arg(long)]
    rotate_placeholder: bool,
    /// Preview the env-file change without writing it.
    #[arg(long)]
    dry_run: bool,
    /// Emit the result as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Inspect configuration, connectivity, watchers, and skill runtime prerequisites.",
    after_help = DOCTOR_AFTER_HELP
)]
struct DoctorArgs {
    /// Limit checks to one project context instead of the inferred current repo.
    #[arg(long)]
    project: Option<String>,
    /// Attempt automatic repairs for supported problems.
    #[arg(long)]
    fix: bool,
    /// Emit the diagnostic report as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Manage project watcher daemons and watcher registration.",
    after_help = WATCHER_GROUP_AFTER_HELP
)]
struct WatcherArgs {
    #[command(subcommand)]
    command: WatcherCommand,
}

#[derive(Debug, Subcommand)]
enum WatcherCommand {
    #[command(about = "Run the watcher daemon in the foreground.", after_help = WATCHER_RUN_AFTER_HELP)]
    Run(WatcherRunCliArgs),
    #[command(about = "Enable the watcher for a project.", after_help = WATCHER_ENABLE_AFTER_HELP)]
    Enable(WatcherManageArgs),
    #[command(about = "Disable the watcher for a project.", after_help = WATCHER_DISABLE_AFTER_HELP)]
    Disable(WatcherManageArgs),
    #[command(about = "Show watcher status for a project.", after_help = WATCHER_STATUS_AFTER_HELP)]
    Status(WatchProjectArgs),
}

#[derive(Debug, Args)]
struct WatchProjectArgs {
    /// Project slug to inspect; defaults to the current repo when available.
    #[arg(long)]
    project: Option<String>,
}

#[derive(Debug, Args)]
struct WatcherManageArgs {
    /// Project slug to manage; defaults to the current repo when available.
    #[arg(long)]
    project: Option<String>,
    /// Preview the watcher service action without applying it.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Args)]
struct WatcherRunCliArgs {
    /// Project slug to watch; defaults to the current repo when available.
    #[arg(long)]
    project: Option<String>,
    /// Override the repository root used for watcher state and file detection.
    #[arg(long)]
    repo_root: Option<PathBuf>,
}

#[derive(Debug, Args)]
#[command(
    about = "Query curated project memory for a project-specific question.",
    after_help = QUERY_AFTER_HELP
)]
struct QueryArgs {
    /// Project slug to query.
    #[arg(long)]
    project: String,
    /// Natural-language question to answer from project memory.
    #[arg(long)]
    question: String,
    /// Restrict results to one or more memory types.
    #[arg(long = "type")]
    types: Vec<String>,
    /// Restrict results to one or more tags.
    #[arg(long = "tag")]
    tags: Vec<String>,
    /// Maximum number of memories to retrieve before answer synthesis.
    #[arg(long, default_value_t = 8)]
    limit: i64,
    /// Ignore memories below this confidence threshold.
    #[arg(long)]
    min_confidence: Option<f32>,
    /// Emit the query result as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Import and inspect git commit history for a project.",
    after_help = COMMITS_GROUP_AFTER_HELP
)]
struct CommitsArgs {
    #[command(subcommand)]
    command: CommitsCommand,
}

#[derive(Debug, Subcommand)]
enum CommitsCommand {
    #[command(about = "Import git commits into the project backend.", after_help = COMMITS_SYNC_AFTER_HELP)]
    Sync(CommitSyncArgs),
    #[command(about = "List imported commits for a project.", after_help = COMMITS_LIST_AFTER_HELP)]
    List(CommitListArgs),
    #[command(about = "Show one imported commit in detail.", after_help = COMMITS_SHOW_AFTER_HELP)]
    Show(CommitShowArgs),
}

#[derive(Debug, Args)]
struct CommitSyncArgs {
    /// Project slug to sync into; defaults to the current repo when available.
    #[arg(long)]
    project: Option<String>,
    /// Limit imported commits to those after this timestamp or revision marker.
    #[arg(long)]
    since: Option<String>,
    /// Cap the number of commits scanned from git.
    #[arg(long)]
    limit: Option<usize>,
    /// Preview the sync without persisting commits.
    #[arg(long)]
    dry_run: bool,
    /// Emit the sync preview or result as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct CommitListArgs {
    /// Project slug to list commits for; defaults to the current repo when available.
    #[arg(long)]
    project: Option<String>,
    /// Maximum number of imported commits to return.
    #[arg(long, default_value_t = 20)]
    limit: i64,
    /// Number of imported commits to skip before listing.
    #[arg(long, default_value_t = 0)]
    offset: i64,
    /// Emit the commit list as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct CommitShowArgs {
    /// Commit SHA or imported commit identifier to show.
    commit: String,
    /// Project slug to read from; defaults to the current repo when available.
    #[arg(long)]
    project: Option<String>,
    /// Emit the commit detail as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Build and inspect the repository index used by scan and analysis flows.",
    after_help = REPO_GROUP_AFTER_HELP
)]
struct RepoArgs {
    #[command(subcommand)]
    command: RepoCommand,
}

#[derive(Debug, Subcommand)]
enum RepoCommand {
    #[command(about = "Build or refresh the local repository index.", after_help = REPO_INDEX_AFTER_HELP)]
    Index(IndexRepoArgs),
    #[command(about = "Show local repository index status and analyzer coverage.", after_help = REPO_STATUS_AFTER_HELP)]
    Status(IndexStatusArgs),
}

#[derive(Debug, Args)]
struct IndexRepoArgs {
    /// Project slug to index; defaults to the current repo when available.
    #[arg(long)]
    project: Option<String>,
    /// Limit indexing to changes after this timestamp or revision marker.
    #[arg(long)]
    since: Option<String>,
    /// Preview indexing work without writing the local index.
    #[arg(long)]
    dry_run: bool,
    /// Emit the index preview or result as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct IndexStatusArgs {
    /// Project slug to inspect; defaults to the current repo when available.
    #[arg(long)]
    project: Option<String>,
    /// Emit the status report as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Export and import portable memory bundles.",
    after_help = BUNDLE_GROUP_AFTER_HELP
)]
struct BundleArgs {
    #[command(subcommand)]
    command: BundleCommand,
}

#[derive(Debug, Subcommand)]
enum BundleCommand {
    #[command(about = "Export a project memory bundle to a zip archive.", after_help = BUNDLE_EXPORT_AFTER_HELP)]
    Export(ExportArgs),
    #[command(about = "Import a project memory bundle from a zip archive.", after_help = BUNDLE_IMPORT_AFTER_HELP)]
    Import(ImportArgs),
}

#[derive(Debug, Args)]
struct ExportArgs {
    /// Project slug to export from.
    #[arg(long)]
    project: String,
    /// Output bundle path.
    #[arg(long)]
    out: PathBuf,
    /// Include archived memories in the bundle.
    #[arg(long)]
    include_archived: bool,
    /// Include source file paths in the bundle provenance.
    #[arg(long)]
    include_source_file_paths: bool,
    /// Include git commit identifiers in the bundle provenance.
    #[arg(long)]
    include_git_commits: bool,
    /// Include source excerpts in the bundle provenance.
    #[arg(long)]
    include_source_excerpts: bool,
    /// Preview the bundle contents without writing the output file.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Args)]
struct ImportArgs {
    /// Project slug to import into.
    #[arg(long)]
    project: String,
    /// Bundle file to import.
    bundle: PathBuf,
    /// Preview the import without writing memories.
    #[arg(long)]
    dry_run: bool,
    /// Emit the import preview or result as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Build a project resume pack from checkpoints, timeline, and durable memory.",
    after_help = RESUME_AFTER_HELP
)]
struct ResumeArgs {
    /// Project slug to resume; defaults to the current repo when available.
    #[arg(long)]
    project: Option<String>,
    /// Emit the resume pack as JSON.
    #[arg(long)]
    json: bool,
    /// Include the optional LLM summary in the resume output.
    #[arg(long, default_value_t = true)]
    include_llm_summary: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Manage project checkpoints and plan-backed execution transitions.",
    after_help = CHECKPOINT_GROUP_AFTER_HELP
)]
struct CheckpointArgs {
    #[command(subcommand)]
    command: CheckpointCommand,
}

#[derive(Debug, Subcommand)]
enum CheckpointCommand {
    #[command(about = "Save a checkpoint for the current project state.", after_help = CHECKPOINT_SAVE_AFTER_HELP)]
    Save(CheckpointSaveArgs),
    #[command(about = "Show the current saved checkpoint.", after_help = CHECKPOINT_SHOW_AFTER_HELP)]
    Show(CheckpointShowArgs),
    #[command(about = "Save a checkpoint and record the approved execution plan.", after_help = CHECKPOINT_START_AFTER_HELP)]
    StartExecution(CheckpointStartExecutionArgs),
    #[command(about = "Verify that the active approved plan is fully complete.", after_help = CHECKPOINT_FINISH_AFTER_HELP)]
    FinishExecution(CheckpointFinishExecutionArgs),
}

#[derive(Debug, Args)]
struct CheckpointSaveArgs {
    /// Project slug to checkpoint; defaults to the current repo when available.
    #[arg(long)]
    project: Option<String>,
    /// Optional human note describing the checkpoint.
    #[arg(long)]
    note: Option<String>,
    /// Preview the checkpoint payload without writing it.
    #[arg(long)]
    dry_run: bool,
    /// Emit the checkpoint result as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct CheckpointShowArgs {
    /// Project slug to inspect; defaults to the current repo when available.
    #[arg(long)]
    project: Option<String>,
}

#[derive(Debug, Args)]
struct CheckpointStartExecutionArgs {
    /// Project slug to update; defaults to the current repo when available.
    #[arg(long)]
    project: Option<String>,
    /// Optional checkpoint note to store alongside the plan transition.
    #[arg(long)]
    note: Option<String>,
    /// Read the approved plan markdown from a file.
    #[arg(long)]
    plan_file: Option<PathBuf>,
    /// Read the approved plan markdown from stdin.
    #[arg(long)]
    plan_stdin: bool,
    /// Explicit title for the saved plan memory.
    #[arg(long)]
    title: Option<String>,
    /// Stable thread key used to replace later revisions of the same plan.
    #[arg(long)]
    thread_key: Option<String>,
    /// Validate and preview the execution-start flow without writing checkpoint or memory state.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Args)]
struct CheckpointFinishExecutionArgs {
    /// Project slug to verify; defaults to the current repo when available.
    #[arg(long)]
    project: Option<String>,
    /// Explicit plan thread key when multiple active plans exist.
    #[arg(long)]
    thread_key: Option<String>,
    /// Optional updated plan file to sync before completion verification.
    #[arg(long)]
    plan_file: Option<PathBuf>,
    /// Optional updated plan markdown from stdin to sync before verification.
    #[arg(long)]
    plan_stdin: bool,
    /// Preview whether verification would pass or fail without writing.
    #[arg(long)]
    dry_run: bool,
    /// Emit the completion report as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Capture structured task evidence from a file payload.",
    after_help = CAPTURE_GROUP_AFTER_HELP
)]
struct CaptureArgs {
    #[command(subcommand)]
    command: CaptureCommand,
}

#[derive(Debug, Subcommand)]
enum CaptureCommand {
    #[command(about = "Send one structured task capture payload to the backend.", after_help = CAPTURE_TASK_AFTER_HELP)]
    Task(CaptureTaskArgs),
}

#[derive(Debug, Args)]
struct CaptureTaskArgs {
    /// JSON file containing the capture payload.
    #[arg(long)]
    file: PathBuf,
    /// Validate and preview the capture without writing it.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Scan a repository for durable-memory candidates using the local index and analyzers.",
    after_help = SCAN_AFTER_HELP
)]
struct ScanArgs {
    /// Project slug to scan; defaults to the current repo when available.
    #[arg(long)]
    project: Option<String>,
    /// Limit the scan to files or commits after this timestamp or revision marker.
    #[arg(long)]
    since: Option<String>,
    /// Force a local repository index rebuild before scanning.
    #[arg(long)]
    rebuild_index: bool,
    /// Preview candidate memories without persisting anything.
    #[arg(long)]
    dry_run: bool,
    /// Emit the scan report as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Capture recent work and curate it into durable project memory.",
    after_help = REMEMBER_AFTER_HELP
)]
struct RememberArgs {
    /// Project slug to write into; defaults to the current repo when available.
    #[arg(long)]
    project: Option<String>,
    /// Explicit task title for the remember capture.
    #[arg(long)]
    title: Option<String>,
    /// Original user prompt or task framing to attach to the capture.
    #[arg(long)]
    prompt: Option<String>,
    /// High-level summary of what changed.
    #[arg(long)]
    summary: Option<String>,
    /// Durable note to preserve as evidence for curation.
    #[arg(long = "note")]
    notes: Vec<String>,
    /// File path to attach as changed during the task.
    #[arg(long = "file-changed")]
    files_changed: Vec<String>,
    /// Test name or command that passed.
    #[arg(long = "test-passed")]
    tests_passed: Vec<String>,
    /// Test name or command that failed.
    #[arg(long = "test-failed")]
    tests_failed: Vec<String>,
    /// File containing command output to attach as evidence.
    #[arg(long)]
    command_output_file: Option<PathBuf>,
    /// Auto-detect changed files from git status when possible.
    #[arg(long, default_value_t = true)]
    auto_files: bool,
    /// Preview the derived capture and curate actions without writing them.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Curate raw captures into canonical memory entries.",
    after_help = CURATE_AFTER_HELP
)]
struct CurateArgs {
    /// Project slug to curate.
    #[arg(long)]
    project: String,
    /// Limit the number of raw captures processed in one run.
    #[arg(long)]
    batch_size: Option<i64>,
    /// Preview curation decisions without writing memory state.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Manage embedding indexes and spaces for semantic retrieval.",
    after_help = EMBEDDINGS_GROUP_AFTER_HELP
)]
struct EmbeddingsArgs {
    #[command(subcommand)]
    command: EmbeddingsCommand,
}

#[derive(Debug, Subcommand)]
enum EmbeddingsCommand {
    #[command(about = "Build or refresh the active embedding index.", after_help = EMBEDDINGS_REINDEX_AFTER_HELP)]
    Reindex(EmbeddingsProjectArgs),
    #[command(about = "Generate embeddings for eligible chunks.", after_help = EMBEDDINGS_REEMBED_AFTER_HELP)]
    Reembed(EmbeddingsProjectArgs),
    #[command(about = "Delete stale or orphaned embedding rows.", after_help = EMBEDDINGS_PRUNE_AFTER_HELP)]
    Prune(EmbeddingsProjectArgs),
}

#[derive(Debug, Args)]
struct ProjectArgs {
    /// Project slug to operate on.
    #[arg(long)]
    project: String,
}

#[derive(Debug, Args)]
struct EmbeddingsProjectArgs {
    /// Project slug to operate on.
    #[arg(long)]
    project: String,
    /// Preview the embedding maintenance action without writing it.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Archive low-confidence, low-importance memories in a project.",
    after_help = ARCHIVE_AFTER_HELP
)]
struct ArchiveArgs {
    /// Project slug to archive within.
    #[arg(long)]
    project: String,
    /// Maximum confidence allowed for candidate memories.
    #[arg(long, default_value_t = 0.3)]
    max_confidence: f32,
    /// Maximum importance allowed for candidate memories.
    #[arg(long, default_value_t = 1)]
    max_importance: i32,
    /// Preview archive candidates without changing memory state.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Open the terminal UI for browsing memories, activity, and project state.",
    after_help = TUI_AFTER_HELP
)]
struct TuiArgs {
    /// Project slug to open initially; defaults to the current repo when available.
    #[arg(long)]
    project: Option<String>,
}

#[derive(Debug, Args)]
#[command(
    about = "Inspect or flush automation state for a project.",
    after_help = AUTOMATION_GROUP_AFTER_HELP
)]
struct AutomationArgs {
    #[command(subcommand)]
    command: AutomationCommand,
}

#[derive(Debug, Subcommand)]
enum AutomationCommand {
    #[command(about = "Show the current automation state for a project.", after_help = AUTOMATION_STATUS_AFTER_HELP)]
    Status(ProjectArgs),
    #[command(about = "Flush pending automation work into capture and optional curation.", after_help = AUTOMATION_FLUSH_AFTER_HELP)]
    Flush(AutomationFlushArgs),
}

#[derive(Debug, Args)]
struct AutomationFlushArgs {
    #[command(flatten)]
    project: ProjectArgs,
    /// Run curation after flushing capture state.
    #[arg(long)]
    curate: bool,
    /// Preview the flush without creating capture or automation state.
    #[arg(long)]
    dry_run: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let Cli {
        config: cli_config,
        writer_id: cli_writer_id,
        command,
    } = Cli::parse();

    match &command {
        Command::Wizard(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            let repo_root = resolve_repo_root(&cwd)?;
            let project = if repo_root == cwd || repo_root.join(".git").exists() {
                resolve_project_slug(args.project.clone(), &repo_root).ok()
            } else {
                args.project.clone()
            };
            wizard::run(&cwd, &repo_root, project, args.global, args.dry_run).await?;
            return Ok(());
        }
        Command::Init(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            let project = resolve_project_slug(args.project.clone(), &cwd)?;
            let repo_root = resolve_repo_root(&cwd)?;
            let output = initialize_repo(&repo_root, &project, args.force, args.dry_run)?;
            println!("{output}");
            return Ok(());
        }
        Command::Service(args) => {
            let config_path = cli_config
                .clone()
                .unwrap_or_else(default_global_config_path);
            match &args.command {
                ServiceCommand::Run => {
                    service_runtime::run_service(cli_config.clone()).await?;
                }
                ServiceCommand::Enable(args) => {
                    if args.dry_run {
                        println!("{}", preview_enable_backend_service(&config_path));
                    } else {
                        let token_result =
                            ensure_shared_service_api_token_for_config(&config_path, None, true)?;
                        if token_result.changed {
                            println!("{}", token_result.summary_line());
                        }
                        println!("{}", enable_backend_service(&config_path)?);
                    }
                }
                ServiceCommand::Disable(args) => {
                    if args.dry_run {
                        println!("{}", preview_disable_backend_service(&config_path));
                    } else {
                        println!("{}", disable_backend_service()?);
                    }
                }
                ServiceCommand::Status => println!("{}", backend_service_status(&config_path)?),
                ServiceCommand::EnsureApiToken(args) => {
                    let _ = args.shared;
                    let result = if args.dry_run {
                        preview_shared_service_api_token_for_config(
                            &config_path,
                            None,
                            args.rotate_placeholder,
                        )?
                    } else {
                        ensure_shared_service_api_token_for_config(
                            &config_path,
                            None,
                            args.rotate_placeholder,
                        )?
                    };
                    if args.json {
                        println!("{}", serde_json::to_string_pretty(&result)?);
                    } else {
                        println!("{}", result.summary_line());
                    }
                }
            }
            return Ok(());
        }
        Command::Watcher(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            let repo_root = resolve_repo_root(&cwd)?;
            match &args.command {
                WatcherCommand::Run(_) => {}
                WatcherCommand::Enable(args) => {
                    let project = resolve_project_slug(args.project.clone(), &cwd)?;
                    let output = if args.dry_run {
                        preview_enable_watch_service(&repo_root, &project)?
                    } else {
                        enable_watch_service(&repo_root, &project)?
                    };
                    println!("{output}");
                }
                WatcherCommand::Disable(args) => {
                    let project = resolve_project_slug(args.project.clone(), &cwd)?;
                    let output = if args.dry_run {
                        preview_disable_watch_service(&project)?
                    } else {
                        disable_watch_service(&project)?
                    };
                    println!("{output}");
                }
                WatcherCommand::Status(args) => {
                    let project = resolve_project_slug(args.project.clone(), &cwd)?;
                    let output = watch_service_status(&repo_root, &project)?;
                    println!("{output}");
                }
            }
            if !matches!(args.command, WatcherCommand::Run(_)) {
                return Ok(());
            }
        }
        Command::Doctor(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            let repo_root = resolve_repo_root(&cwd)?;
            let project = resolve_project_slug(args.project.clone(), &cwd).unwrap_or_else(|_| {
                repo_root
                    .file_name()
                    .and_then(|v| v.to_str())
                    .unwrap_or("memory")
                    .to_string()
            });
            let report = run_doctor(cli_config.clone(), &repo_root, &project, args.fix).await?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_doctor_report(&report);
            }
            return Ok(());
        }
        _ => {}
    }

    let config = AppConfig::load_from_path(cli_config).context("load config")?;
    let client = Client::builder()
        .timeout(config.service.request_timeout)
        .build()
        .context("build http client")?;

    match command {
        Command::Wizard(_) => unreachable!("wizard is handled before config loading"),
        Command::Init(_) => unreachable!("init is handled before config loading"),
        Command::Service(ServiceArgs {
            command: ServiceCommand::Run,
        }) => unreachable!("service run is handled before config loading"),
        Command::Service(_) => unreachable!("service management is handled before config loading"),
        Command::Watcher(WatcherArgs {
            command:
                WatcherCommand::Enable(_) | WatcherCommand::Disable(_) | WatcherCommand::Status(_),
        }) => unreachable!("watcher lifecycle commands are handled before config loading"),
        Command::Doctor(_) => unreachable!("doctor is handled before config loading"),
        Command::Commits(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            let repo_root = resolve_repo_root(&cwd)?;
            let api = ApiClient::new(client, config);
            match args.command {
                CommitsCommand::Sync(args) => {
                    let project = resolve_project_slug(args.project, &cwd)?;
                    let commits = commits::collect_git_commits(
                        &repo_root,
                        args.since.as_deref(),
                        args.limit,
                    )?;
                    let response = api
                        .sync_commits(&CommitSyncRequest {
                            project,
                            repo_root: repo_root.display().to_string(),
                            commits,
                            dry_run: args.dry_run,
                        })
                        .await?;
                    if args.json {
                        println!("{}", serde_json::to_string_pretty(&response)?);
                    } else {
                        print_commit_sync_response(&response);
                    }
                }
                CommitsCommand::List(args) => {
                    let project = resolve_project_slug(args.project, &cwd)?;
                    let response = api
                        .project_commits(&project, args.limit.clamp(1, 500), args.offset.max(0))
                        .await?;
                    if args.json {
                        println!("{}", serde_json::to_string_pretty(&response)?);
                    } else {
                        print_project_commits(&response);
                    }
                }
                CommitsCommand::Show(args) => {
                    let project = resolve_project_slug(args.project, &cwd)?;
                    let response = api.project_commit(&project, &args.commit).await?;
                    if args.json {
                        println!("{}", serde_json::to_string_pretty(&response)?);
                    } else {
                        print_commit_detail(&response);
                    }
                }
            }
        }
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
        Command::Repo(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            let repo_root = resolve_repo_root(&cwd)?;
            match args.command {
                RepoCommand::Index(args) => {
                    let project = resolve_project_slug(args.project, &cwd)?;
                    let report = scan::run_index(
                        &repo_root,
                        &project,
                        args.since.as_deref(),
                        &config,
                        args.dry_run,
                    )?;
                    if args.json {
                        println!("{}", serde_json::to_string_pretty(&report)?);
                    } else {
                        print_index_report(&report);
                    }
                }
                RepoCommand::Status(args) => {
                    let project = resolve_project_slug(args.project, &cwd)?;
                    let status = scan::read_index_status(&repo_root, &project)?;
                    if args.json {
                        println!("{}", serde_json::to_string_pretty(&status)?);
                    } else {
                        print_index_status(&status, &project);
                    }
                }
            }
        }
        Command::Bundle(args) => {
            let api = ApiClient::new(client, config);
            match args.command {
                BundleCommand::Export(args) => {
                    let options = ProjectMemoryExportOptions {
                        include_archived: args.include_archived,
                        include_tags: true,
                        include_relations: true,
                        include_source_file_paths: args.include_source_file_paths,
                        include_git_commits: args.include_git_commits,
                        include_source_excerpts: args.include_source_excerpts,
                    };
                    let preview = api.export_bundle_preview(&args.project, &options).await?;
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "project": args.project,
                            "output": args.out.display().to_string(),
                            "preview": preview,
                            "dry_run": args.dry_run,
                        }))?
                    );
                    if !args.dry_run {
                        let bytes = api.export_bundle(&args.project, &options).await?;
                        fs::write(&args.out, bytes)
                            .with_context(|| format!("write {}", args.out.display()))?;
                    }
                }
                BundleCommand::Import(args) => {
                    let bytes = fs::read(&args.bundle)
                        .with_context(|| format!("read {}", args.bundle.display()))?;
                    if args.dry_run {
                        let preview = api.import_bundle_preview(&args.project, bytes).await?;
                        if args.json {
                            println!("{}", serde_json::to_string_pretty(&preview)?);
                        } else {
                            print_bundle_import_preview(&preview);
                        }
                    } else {
                        let response = api.import_bundle(&args.project, bytes).await?;
                        if args.json {
                            println!("{}", serde_json::to_string_pretty(&response)?);
                        } else {
                            print_bundle_import_response(&response);
                        }
                    }
                }
            }
        }
        Command::Checkpoint(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            let repo_root = resolve_repo_root(&cwd)?;
            match args.command {
                CheckpointCommand::Save(args) => {
                    let project = resolve_project_slug(args.project, &cwd)?;
                    if args.dry_run {
                        let (checkpoint, path) =
                            preview_checkpoint(&project, &repo_root, args.note)?;
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
                                resume::format_checkpoint(&checkpoint)
                            );
                        }
                    } else {
                        let api = ApiClient::new(client.clone(), config.clone());
                        let (checkpoint, path) =
                            save_checkpoint_with_activity(&api, &project, &repo_root, args.note)
                                .await?;
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
                                resume::format_checkpoint(&checkpoint)
                            );
                        }
                    }
                }
                CheckpointCommand::Show(args) => {
                    let project = resolve_project_slug(args.project, &cwd)?;
                    if let Some(checkpoint) = resume::load_checkpoint(&project, &repo_root)? {
                        println!("{}", resume::format_checkpoint(&checkpoint));
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
                    let thread_key =
                        derive_plan_thread_key(args.thread_key.as_deref(), &title, &project);
                    let (checkpoint, path) = if args.dry_run {
                        preview_checkpoint(&project, &repo_root, Some(note))?
                    } else {
                        save_checkpoint_with_activity(&api, &project, &repo_root, Some(note))
                            .await?
                    };
                    let writer = resolve_writer_identity(&config, cli_writer_id.as_deref())?;
                    let mut request = build_plan_execution_request(
                        &project,
                        &writer,
                        &title,
                        &thread_key,
                        &plan_markdown,
                        source_path.as_deref(),
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
                                    resume::format_checkpoint(&checkpoint)
                                );
                            } else {
                                eprintln!(
                                    "Saved checkpoint for `{project}` to {}\n\n{}",
                                    path.display(),
                                    resume::format_checkpoint(&checkpoint)
                                );
                            }
                            return Err(
                                error.context("checkpoint saved, but approved plan capture failed")
                            );
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
                                    resume::format_checkpoint(&checkpoint)
                                );
                            } else {
                                eprintln!(
                                    "Saved checkpoint for `{project}` to {}\n\n{}",
                                    path.display(),
                                    resume::format_checkpoint(&checkpoint)
                                );
                            }
                            return Err(error.context(
                                "checkpoint saved and plan captured, but curation failed",
                            ));
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
                    if !args.dry_run {
                        if let Err(error) = api.log_plan_activity(&start_request).await {
                            eprintln!(
                                "warning: failed to log plan activity for `{project}`: {error}"
                            );
                        }
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
                CheckpointCommand::FinishExecution(args) => {
                    let project = resolve_project_slug(args.project, &cwd)?;
                    let api = ApiClient::new(client.clone(), config.clone());
                    let selection =
                        resolve_active_plan_selection(&api, &project, args.thread_key.as_deref())
                            .await?;
                    let mut synced_plan = false;
                    let mut synced_source_path = None;

                    let detail = if args.plan_file.is_some() || args.plan_stdin {
                        let (plan_markdown, source_path) =
                            load_plan_content(args.plan_file.as_deref(), args.plan_stdin)?;
                        if args.dry_run {
                            synced_plan = true;
                            synced_source_path =
                                source_path.as_ref().map(|path| path.display().to_string());
                            plan_detail_from_markdown(
                                &selection,
                                &plan_markdown,
                                selection.memory_id,
                            )?
                        } else {
                            let plan_items = parse_plan_checkboxes(&plan_markdown);
                            ensure_checkbox_plan(&plan_items)?;
                            let writer =
                                resolve_writer_identity(&config, cli_writer_id.as_deref())?;
                            let mut request = build_plan_execution_request(
                                &project,
                                &writer,
                                &selection.title,
                                &selection.thread_key,
                                &plan_markdown,
                                source_path.as_deref(),
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
                            eprintln!(
                                "warning: failed to log plan activity for `{project}`: {error}"
                            );
                        }
                    }
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
                            eprintln!(
                                "warning: failed to log plan activity for `{project}`: {error}"
                            );
                        }
                    }
                    if args.json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&serde_json::json!({
                                "report": report,
                                "dry_run": args.dry_run,
                            }))?
                        );
                    } else {
                        print_plan_execution_finish_report(&report);
                        if args.dry_run {
                            println!(
                                "\nDry run only: no plan state was synced, logged, or persisted."
                            );
                        }
                    }
                    if !report.verified_complete {
                        anyhow::bail!("approved plan still has unchecked items");
                    }
                }
            }
        }
        Command::Resume(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            let repo_root = resolve_repo_root(&cwd)?;
            let project = resolve_project_slug(args.project, &cwd)?;
            let checkpoint = resume::load_checkpoint(&project, &repo_root)?;
            let api = ApiClient::new(client, config);
            let payload = api
                .resume(&ResumeRequest {
                    project: project.clone(),
                    checkpoint,
                    since: None,
                    include_llm_summary: args.include_llm_summary,
                    limit: 12,
                })
                .await?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&payload)?);
            } else {
                print_resume_response(&payload);
            }
        }
        Command::Scan(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            let repo_root = resolve_repo_root(&cwd)?;
            let project = resolve_project_slug(args.project, &cwd)?;
            let writer = resolve_writer_identity(&config, cli_writer_id.as_deref())?;
            let api = ApiClient::new(client, config);
            let report = scan::run_scan(
                &api,
                &repo_root,
                &project,
                args.since.as_deref(),
                args.rebuild_index,
                args.dry_run,
                &writer.id,
                writer.name.as_deref(),
            )
            .await?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_scan_report(&report);
            }
        }
        Command::Capture(args) => match args.command {
            CaptureCommand::Task(args) => {
                let mut request: CaptureTaskRequest = serde_json::from_str(
                    &fs::read_to_string(args.file).context("read payload file")?,
                )?;
                if request.writer_id.trim().is_empty() {
                    let writer = resolve_writer_identity(&config, cli_writer_id.as_deref())?;
                    request.writer_id = writer.id;
                    request.writer_name = request.writer_name.or(writer.name);
                }
                request.dry_run = args.dry_run;
                let response = client
                    .post(service_url(&config, "/v1/capture/task"))
                    .headers(write_headers(&config)?)
                    .json(&request)
                    .send()
                    .await?;
                print_json_response(response).await?;
            }
        },
        Command::Remember(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            let repo_root = resolve_repo_root(&cwd)?;
            let project = resolve_project_slug(args.project.clone(), &cwd)?;
            let writer = resolve_writer_identity(&config, cli_writer_id.as_deref())?;
            let dry_run = args.dry_run;
            let mut request =
                build_remember_request(args, &project, &writer.id, writer.name.as_deref())?;
            request.dry_run = dry_run;
            let api = ApiClient::new(client, config);
            let capture = api.capture_task(&request).await?;
            let curate = api
                .curate(&project, repo_replacement_policy(&repo_root), dry_run)
                .await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "capture": capture,
                    "curate": curate,
                    "dry_run": dry_run,
                }))?
            );
        }
        Command::Curate(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            let repo_root = resolve_repo_root(&cwd)?;
            let replacement_policy = repo_replacement_policy(&repo_root);
            let response = client
                .post(service_url(&config, "/v1/curate"))
                .headers(write_headers(&config)?)
                .json(&CurateRequest {
                    project: args.project,
                    batch_size: args.batch_size,
                    replacement_policy: Some(replacement_policy),
                    dry_run: args.dry_run,
                })
                .send()
                .await?;
            print_json_response(response).await?;
        }
        Command::Embeddings(args) => match args.command {
            EmbeddingsCommand::Reindex(args) => {
                let response = client
                    .post(service_url(&config, "/v1/reindex"))
                    .headers(write_headers(&config)?)
                    .json(&ReindexRequest {
                        project: args.project,
                        dry_run: args.dry_run,
                    })
                    .send()
                    .await?;
                print_json_response(response).await?;
            }
            EmbeddingsCommand::Reembed(args) => {
                let response = client
                    .post(service_url(&config, "/v1/reembed"))
                    .headers(write_headers(&config)?)
                    .json(&ReembedRequest {
                        project: args.project,
                        dry_run: args.dry_run,
                    })
                    .send()
                    .await?;
                print_json_response(response).await?;
            }
            EmbeddingsCommand::Prune(args) => {
                let api = ApiClient::new(client, config);
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &api.prune_embeddings(&args.project, args.dry_run).await?
                    )?
                );
            }
        },
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
                .headers(write_headers(&config)?)
                .json(&ArchiveRequest {
                    project: args.project,
                    max_confidence: args.max_confidence,
                    max_importance: args.max_importance,
                    dry_run: args.dry_run,
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
                    let project = resolve_project_slug(Some(args.project.project), &cwd)?;
                    let repo_root = config
                        .automation
                        .repo_root
                        .as_ref()
                        .map(PathBuf::from)
                        .unwrap_or(cwd);
                    let api = ApiClient::new(client.clone(), config.clone());
                    let writer = resolve_writer_identity(&config, cli_writer_id.as_deref())?;
                    if args.dry_run {
                        let preview = preview_automation_flush(
                            &api.config,
                            &api.client,
                            &project,
                            &repo_root,
                            args.curate,
                            &writer.id,
                            writer.name.as_deref(),
                        )
                        .await?;
                        println!("{}", serde_json::to_string_pretty(&preview)?);
                        return Ok(());
                    }
                    tokio::fs::write(flush_path(&repo_root), b"flush\n")
                        .await
                        .ok();
                    run_once(
                        &api.config,
                        &api.client,
                        &project,
                        &repo_root,
                        true,
                        args.curate,
                        &writer.id,
                        writer.name.as_deref(),
                    )
                    .await?;
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "project": project,
                            "status": "flush_requested",
                            "curate": args.curate
                        }))?
                    );
                }
            }
        }
        Command::Watcher(args) => match args.command {
            WatcherCommand::Run(args) => {
                let writer = resolve_writer_identity_for_tool(
                    &config,
                    cli_writer_id.as_deref(),
                    "memory-watcher",
                )?;
                run_watcher_daemon(
                    config,
                    WatcherRunArgs {
                        project: args.project,
                        repo_root: args.repo_root,
                    },
                    writer.id,
                    writer.name,
                )
                .await?;
            }
            WatcherCommand::Enable(_) | WatcherCommand::Disable(_) | WatcherCommand::Status(_) => {
                unreachable!("watcher lifecycle commands are handled before config loading")
            }
        },
        Command::Tui(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            let repo_root = resolve_repo_root(&cwd)?;
            let project = resolve_project_slug(args.project, &cwd)?;
            let api = ApiClient::new(client, config);
            tui::run(api, project, repo_root).await?;
        }
    }

    Ok(())
}

fn write_shared_env_file(path: &Path, key: &str, value: &str) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("env file path has no parent"))?;
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    let mut lines = if path.exists() {
        fs::read_to_string(path)
            .with_context(|| format!("read {}", path.display()))?
            .lines()
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let wanted = format!("{key}={value}");
    let mut replaced = false;
    for line in &mut lines {
        if line
            .split_once('=')
            .is_some_and(|(existing, _)| existing.trim() == key)
        {
            *line = wanted.clone();
            replaced = true;
        }
    }
    if !replaced {
        lines.push(wanted);
    }
    let mut content = lines.join("\n");
    if !content.ends_with('\n') {
        content.push('\n');
    }
    fs::write(path, content).with_context(|| format!("write {}", path.display()))?;
    set_private_file_permissions(path)
}

const DEV_API_TOKEN: &str = "dev-memory-token";
const SERVICE_API_TOKEN_KEY: &str = "MEMORY_LAYER__SERVICE__API_TOKEN";

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum ServiceApiTokenAction {
    Created,
    Rotated,
    Preserved,
}

#[derive(Debug, Serialize)]
struct ServiceApiTokenEnsureResult {
    path: String,
    changed: bool,
    action: ServiceApiTokenAction,
}

impl ServiceApiTokenEnsureResult {
    fn summary_line(&self) -> String {
        match self.action {
            ServiceApiTokenAction::Created => {
                format!("Created shared service API token in {}", self.path)
            }
            ServiceApiTokenAction::Rotated => {
                format!("Rotated shared service API token in {}", self.path)
            }
            ServiceApiTokenAction::Preserved => {
                format!("Kept existing shared service API token in {}", self.path)
            }
        }
    }
}

fn is_placeholder_service_api_token(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.is_empty() || trimmed == DEV_API_TOKEN
}

fn generate_service_api_token() -> String {
    format!("ml_{}", Uuid::new_v4().simple())
}

fn ensure_shared_service_api_token(
    shared_env_path: &Path,
    preferred_token: Option<&str>,
    rotate_placeholder: bool,
) -> Result<ServiceApiTokenEnsureResult> {
    plan_shared_service_api_token(shared_env_path, rotate_placeholder).and_then(|result| {
        let token = preferred_token
            .map(str::trim)
            .filter(|value| !is_placeholder_service_api_token(value))
            .map(ToOwned::to_owned)
            .unwrap_or_else(generate_service_api_token);
        if result.changed {
            write_shared_env_file(shared_env_path, SERVICE_API_TOKEN_KEY, &token)?;
        }
        Ok(result)
    })
}

fn preview_shared_service_api_token_for_config(
    config_path: &Path,
    preferred_token: Option<&str>,
    rotate_placeholder: bool,
) -> Result<ServiceApiTokenEnsureResult> {
    let _ = preferred_token;
    plan_shared_service_api_token(&shared_env_path_for_config(config_path), rotate_placeholder)
}

fn plan_shared_service_api_token(
    shared_env_path: &Path,
    rotate_placeholder: bool,
) -> Result<ServiceApiTokenEnsureResult> {
    let existing = shared_env_lookup(shared_env_path, SERVICE_API_TOKEN_KEY);
    if let Some(token) = existing.as_deref() {
        if !is_placeholder_service_api_token(token) {
            return Ok(ServiceApiTokenEnsureResult {
                path: shared_env_path.display().to_string(),
                changed: false,
                action: ServiceApiTokenAction::Preserved,
            });
        }
        if !rotate_placeholder {
            return Ok(ServiceApiTokenEnsureResult {
                path: shared_env_path.display().to_string(),
                changed: false,
                action: ServiceApiTokenAction::Preserved,
            });
        }
    }
    Ok(ServiceApiTokenEnsureResult {
        path: shared_env_path.display().to_string(),
        changed: true,
        action: if existing.is_some() {
            ServiceApiTokenAction::Rotated
        } else {
            ServiceApiTokenAction::Created
        },
    })
}

fn ensure_shared_service_api_token_for_config(
    config_path: &Path,
    preferred_token: Option<&str>,
    rotate_placeholder: bool,
) -> Result<ServiceApiTokenEnsureResult> {
    ensure_shared_service_api_token(
        &shared_env_path_for_config(config_path),
        preferred_token,
        rotate_placeholder,
    )
}

fn shared_env_lookup(path: &Path, key: &str) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((name, value)) = trimmed.split_once('=') {
            if name.trim() == key {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

fn shared_env_path_for_config(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("memory-layer.env")
}

fn default_global_config_path() -> PathBuf {
    platform::preferred_global_config_path()
}

fn default_shared_capnp_unix_socket() -> String {
    platform::default_shared_capnp_unix_socket()
}

fn backend_start_hint(config_path: &Path) -> String {
    if backend_service_available() {
        "memory service enable".to_string()
    } else {
        format!("memory --config {} service run", config_path.display())
    }
}

fn backend_service_available() -> bool {
    platform::backend_service_available()
}

#[cfg(not(target_os = "macos"))]
fn packaged_service_available() -> bool {
    platform::packaged_system_service_available()
}

#[cfg(not(target_os = "macos"))]
fn run_systemctl_system<const N: usize>(args: [&str; N]) -> Result<()> {
    let output = ProcessCommand::new("systemctl")
        .args(args)
        .output()
        .with_context(|| format!("run systemctl {}", args.join(" ")))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    anyhow::bail!(
        "systemctl {} failed: {}{}{}",
        args.join(" "),
        stderr.trim(),
        if stderr.trim().is_empty() || stdout.trim().is_empty() {
            ""
        } else {
            " | "
        },
        stdout.trim()
    )
}

#[derive(Debug, Clone, Serialize)]
struct DoctorReport {
    project: String,
    repo_root: String,
    config_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    global_config_path: Option<String>,
    fix_mode: bool,
    checks: Vec<DoctorCheckResult>,
}

#[derive(Debug, Clone, Serialize)]
struct DoctorCheckResult {
    id: String,
    status: DoctorStatus,
    summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    suggested_fix: Option<String>,
    #[serde(default)]
    fix_applied: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum DoctorStatus {
    Ok,
    Warn,
    Fail,
    Skipped,
}

impl DoctorReport {
    fn push(&mut self, result: DoctorCheckResult) {
        self.checks.push(result);
    }
}

fn doctor_check(
    id: &str,
    status: DoctorStatus,
    summary: impl Into<String>,
    details: Option<String>,
    suggested_fix: Option<String>,
    fix_applied: bool,
) -> DoctorCheckResult {
    DoctorCheckResult {
        id: id.to_string(),
        status,
        summary: summary.into(),
        details,
        suggested_fix,
        fix_applied,
    }
}

fn repo_uses_go_skill_runtime(repo_root: &Path) -> bool {
    repo_root
        .join(".agents/skills/memory-layer/scripts/go.mod")
        .is_file()
}

fn go_runtime_available() -> bool {
    ProcessCommand::new("go")
        .arg("version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

async fn run_doctor(
    cli_config: Option<PathBuf>,
    repo_root: &Path,
    project: &str,
    fix: bool,
) -> Result<DoctorReport> {
    let config_path = cli_config
        .clone()
        .unwrap_or_else(|| repo_root.join(".mem").join("config.toml"));
    let global_config_path = discover_global_config_path();
    let mut report = DoctorReport {
        project: project.to_string(),
        repo_root: repo_root.display().to_string(),
        config_path: config_path.display().to_string(),
        global_config_path: global_config_path
            .as_ref()
            .map(|path| path.display().to_string()),
        fix_mode: fix,
        checks: Vec::new(),
    };

    let mem_dir = repo_root.join(".mem");
    let project_path = mem_dir.join("project.toml");
    let root_gitignore_path = repo_root.join(".gitignore");
    let local_service_overrides = read_local_service_overrides(repo_root);

    let mut init_fix_applied = false;
    if !mem_dir.exists() && fix {
        initialize_repo(repo_root, project, false, false)?;
        init_fix_applied = true;
    }

    report.push(doctor_check(
        "repo.bootstrap_dir",
        if mem_dir.exists() || init_fix_applied {
            DoctorStatus::Ok
        } else {
            DoctorStatus::Fail
        },
        if mem_dir.exists() || init_fix_applied {
            "Repo-local .mem directory is present."
        } else {
            "Repo-local .mem directory is missing."
        },
        Some(mem_dir.display().to_string()),
        if mem_dir.exists() || init_fix_applied {
            None
        } else {
            Some("memory init".to_string())
        },
        init_fix_applied,
    ));

    let config_fix_applied = if !config_path.exists() && fix {
        repair_repo_bootstrap(repo_root, project)?;
        true
    } else {
        false
    };
    report.push(doctor_check(
        "repo.config_file",
        if config_path.exists() || config_fix_applied {
            DoctorStatus::Ok
        } else {
            DoctorStatus::Fail
        },
        if config_path.exists() || config_fix_applied {
            "Config file is present."
        } else {
            "Config file is missing."
        },
        Some(config_path.display().to_string()),
        if config_path.exists() || config_fix_applied {
            None
        } else {
            Some("memory init".to_string())
        },
        config_fix_applied,
    ));

    let project_fix_applied = if !project_path.exists() && fix {
        repair_repo_bootstrap(repo_root, project)?;
        true
    } else {
        false
    };
    report.push(doctor_check(
        "repo.project_file",
        if project_path.exists() || project_fix_applied {
            DoctorStatus::Ok
        } else {
            DoctorStatus::Fail
        },
        if project_path.exists() || project_fix_applied {
            "Project metadata file is present."
        } else {
            "Project metadata file is missing."
        },
        Some(project_path.display().to_string()),
        if project_path.exists() || project_fix_applied {
            None
        } else {
            Some("memory init".to_string())
        },
        project_fix_applied,
    ));

    report.push(doctor_check(
        "global.config_file",
        if global_config_path.is_some() {
            DoctorStatus::Ok
        } else {
            DoctorStatus::Warn
        },
        if global_config_path.is_some() {
            "Global shared config is present."
        } else {
            "Global shared config is missing."
        },
        Some(
            global_config_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(default_global_config_path_label),
        ),
        if global_config_path.is_some() {
            None
        } else {
            Some("Create the global config and set shared defaults there.".to_string())
        },
        false,
    ));

    let gitignore_fix_applied = if !root_gitignore_contains_mem(repo_root)? && fix {
        ensure_root_gitignore_entry(&root_gitignore_path, "/.mem\n")?;
        true
    } else {
        false
    };
    report.push(doctor_check(
        "repo.gitignore",
        if root_gitignore_contains_mem(repo_root)? {
            DoctorStatus::Ok
        } else {
            DoctorStatus::Warn
        },
        if root_gitignore_contains_mem(repo_root)? {
            "Root .gitignore ignores .mem."
        } else {
            "Root .gitignore does not ignore .mem."
        },
        Some(root_gitignore_path.display().to_string()),
        if root_gitignore_contains_mem(repo_root)? {
            None
        } else {
            Some("memory doctor --fix".to_string())
        },
        gitignore_fix_applied,
    ));

    let config = match AppConfig::load_from_path(cli_config.clone()) {
        Ok(config) => {
            report.push(doctor_check(
                "config.load",
                DoctorStatus::Ok,
                "Merged config loads successfully.",
                None,
                None,
                false,
            ));
            Some(config)
        }
        Err(error) => {
            report.push(doctor_check(
                "config.load",
                DoctorStatus::Fail,
                "Merged config failed to load.",
                Some(error.to_string()),
                Some(format!(
                    "Check {} and {}",
                    global_config_path
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(default_global_config_path_label),
                    config_path.display()
                )),
                false,
            ));
            None
        }
    };

    if let Some(config) = config {
        report.push(doctor_check(
            "config.database_url",
            if is_placeholder_database_url(&config.database.url) {
                DoctorStatus::Warn
            } else {
                DoctorStatus::Ok
            },
            if is_placeholder_database_url(&config.database.url) {
                "Database URL still uses the placeholder value."
            } else {
                "Database URL is configured."
            },
            Some(mask_database_url(&config.database.url)),
            if is_placeholder_database_url(&config.database.url) {
                Some(format!(
                    "Set [database].url in {}",
                    global_config_path
                        .as_ref()
                        .unwrap_or(&config_path)
                        .display()
                ))
            } else {
                None
            },
            false,
        ));

        if is_placeholder_database_url(&config.database.url) {
            report.push(doctor_check(
                "database.pgvector_extension",
                DoctorStatus::Skipped,
                "Skipped pgvector checks because the database URL is still a placeholder.",
                None,
                None,
                false,
            ));
        } else {
            match PgPoolOptions::new()
                .max_connections(1)
                .acquire_timeout(Duration::from_secs(3))
                .connect(&config.database.url)
                .await
            {
                Ok(pool) => {
                    report.push(doctor_check(
                        "database.connect",
                        DoctorStatus::Ok,
                        "Database connection succeeded.",
                        Some(mask_database_url(&config.database.url)),
                        None,
                        false,
                    ));

                    match sqlx::query(
                        "SELECT extversion FROM pg_extension WHERE extname = 'vector' LIMIT 1",
                    )
                    .fetch_optional(&pool)
                    .await
                    {
                        Ok(Some(row)) => report.push(doctor_check(
                            "database.pgvector_extension",
                            DoctorStatus::Ok,
                            "pgvector extension is enabled in the target database.",
                            Some(format!(
                                "vector extension version {}",
                                row.try_get::<String, _>("extversion")
                                    .unwrap_or_else(|_| "unknown".to_string())
                            )),
                            None,
                            false,
                        )),
                        Ok(None) => report.push(doctor_check(
                            "database.pgvector_extension",
                            DoctorStatus::Fail,
                            "pgvector extension is not enabled in the target database.",
                            None,
                            Some(
                                "Install pgvector for your PostgreSQL version and run CREATE EXTENSION vector; in the target database."
                                    .to_string(),
                            ),
                            false,
                        )),
                        Err(error) => report.push(doctor_check(
                            "database.pgvector_extension",
                            DoctorStatus::Fail,
                            "Could not verify pgvector extension state.",
                            Some(error.to_string()),
                            Some(
                                "Install pgvector for your PostgreSQL version and run CREATE EXTENSION vector; in the target database."
                                    .to_string(),
                            ),
                            false,
                        )),
                    }
                }
                Err(error) => {
                    report.push(doctor_check(
                        "database.connect",
                        DoctorStatus::Fail,
                        "Could not connect to the configured database directly.",
                        Some(error.to_string()),
                        Some("Fix the database URL or credentials first.".to_string()),
                        false,
                    ));
                    report.push(doctor_check(
                        "database.pgvector_extension",
                        DoctorStatus::Skipped,
                        "Skipped pgvector extension check because the database connection failed.",
                        None,
                        None,
                        false,
                    ));
                }
            }
        }

        report.push(doctor_check(
            "config.api_token",
            if config.service.api_token.trim().is_empty() {
                DoctorStatus::Fail
            } else if config.service.api_token == DEV_API_TOKEN {
                DoctorStatus::Warn
            } else {
                DoctorStatus::Ok
            },
            if config.service.api_token.trim().is_empty() {
                "API token is empty."
            } else if config.service.api_token == DEV_API_TOKEN {
                "API token is set to the development default."
            } else {
                "API token is configured."
            },
            None,
            if config.service.api_token.trim().is_empty() {
                Some(
                    "Run `memory wizard --global` or `memory service ensure-api-token --rotate-placeholder` to provision a machine-local token."
                        .to_string(),
                )
            } else if config.service.api_token == DEV_API_TOKEN {
                Some(
                    "Run `memory wizard --global` or `memory service ensure-api-token --rotate-placeholder` to provision a machine-local token."
                        .to_string(),
                )
            } else {
                None
            },
            false,
        ));

        report.push(doctor_check(
            "config.writer_id",
            DoctorStatus::Ok,
            if config.writer.id.trim().is_empty() {
                "Writer id will be auto-derived for write-capable workflows."
            } else {
                "Writer id is configured."
            },
            Some(resolve_writer_identity(&config, None)?.id),
            if config.writer.id.trim().is_empty() {
                Some(format!(
                    "Optional: set [writer].id in {} or export MEMORY_LAYER_WRITER_ID if you want a custom stable writer label.",
                    config_path.display()
                ))
            } else {
                None
            },
            false,
        ));

        report.push(doctor_check(
            "config.llm_model",
            if config.llm.model.trim().is_empty() {
                DoctorStatus::Fail
            } else {
                DoctorStatus::Ok
            },
            if config.llm.model.trim().is_empty() {
                "LLM model is not configured."
            } else {
                "LLM model is configured."
            },
            Some(format!(
                "provider={} base_url={}",
                config.llm.provider, config.llm.base_url
            )),
            if config.llm.model.trim().is_empty() {
                Some(format!(
                    "Set [llm].model in {}",
                    global_config_path
                        .as_ref()
                        .unwrap_or(&config_path)
                        .display()
                ))
            } else {
                None
            },
            false,
        ));

        let repo_env_path = discover_repo_env_path();
        let llm_api_key_value = env::var(&config.llm.api_key_env)
            .ok()
            .or_else(|| {
                repo_env_path
                    .as_ref()
                    .and_then(|path| shared_env_lookup(path, &config.llm.api_key_env))
            })
            .or_else(|| {
                global_config_path.as_ref().and_then(|path| {
                    shared_env_lookup(&shared_env_path_for_config(path), &config.llm.api_key_env)
                })
            })
            .unwrap_or_default();
        report.push(doctor_check(
            "config.llm_api_key",
            if llm_api_key_value.trim().is_empty() {
                DoctorStatus::Fail
            } else {
                DoctorStatus::Ok
            },
            if llm_api_key_value.trim().is_empty() {
                "LLM API key environment variable is missing."
            } else {
                "LLM API key environment variable is present."
            },
            Some(config.llm.api_key_env.clone()),
            if llm_api_key_value.trim().is_empty() {
                Some({
                    let mut locations = Vec::new();
                    if let Some(path) = repo_env_path.as_ref() {
                        locations.push(path.display().to_string());
                    }
                    locations.push(
                        global_config_path
                            .as_ref()
                            .map(|path| shared_env_path_for_config(path).display().to_string())
                            .unwrap_or_else(|| {
                                shared_env_path_for_config(&config_path)
                                    .display()
                                    .to_string()
                            }),
                    );
                    format!(
                        "Set {} in {} or export it in your shell",
                        config.llm.api_key_env,
                        locations.join(" or ")
                    )
                })
            } else {
                None
            },
            false,
        ));

        report.push(doctor_check(
            "config.service_endpoints",
            DoctorStatus::Ok,
            if local_service_overrides.is_some() {
                "Repo-local service endpoints are configured."
            } else {
                "Using shared/global service endpoints."
            },
            Some(format!(
                "http={} capnp_tcp={} capnp_unix={}",
                config.service.bind_addr,
                config.service.capnp_tcp_addr,
                config.service.capnp_unix_socket
            )),
            None,
            false,
        ));

        let runtime_dir = automation_runtime_dir(&config, repo_root);
        let runtime_fix_applied = if !runtime_dir.exists() && fix {
            fs::create_dir_all(&runtime_dir)
                .with_context(|| format!("create {}", runtime_dir.display()))?;
            true
        } else {
            false
        };
        report.push(doctor_check(
            "automation.runtime_dir",
            if runtime_dir.exists() {
                DoctorStatus::Ok
            } else if config.automation.enabled {
                DoctorStatus::Warn
            } else {
                DoctorStatus::Warn
            },
            if runtime_dir.exists() {
                "Automation runtime directory is present."
            } else {
                "Automation runtime directory is missing."
            },
            Some(runtime_dir.display().to_string()),
            if runtime_dir.exists() {
                None
            } else {
                Some("memory doctor --fix".to_string())
            },
            runtime_fix_applied,
        ));

        let resolved_repo_root = config
            .automation
            .repo_root
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| repo_root.to_path_buf());
        report.push(doctor_check(
            "automation.repo_root",
            if resolved_repo_root == repo_root {
                DoctorStatus::Ok
            } else {
                DoctorStatus::Warn
            },
            if resolved_repo_root == repo_root {
                "Automation repo_root matches the current repository."
            } else {
                "Automation repo_root differs from the current repository."
            },
            Some(resolved_repo_root.display().to_string()),
            if resolved_repo_root == repo_root {
                None
            } else {
                Some(format!(
                    "Edit {} and set [automation].repo_root",
                    config_path.display()
                ))
            },
            false,
        ));

        let client = Client::builder()
            .timeout(config.service.request_timeout)
            .build()
            .context("build doctor http client")?;
        let api = ApiClient::new(client, config.clone());

        match api.health().await {
            Ok(value) => {
                let role = value.get("role").and_then(|field| field.as_str());
                let upstream = value.get("upstream").cloned();
                report.push(doctor_check(
                    "backend.health",
                    DoctorStatus::Ok,
                    "Backend health endpoint is reachable.",
                    Some(value.to_string()),
                    None,
                    false,
                ));
                report.push(doctor_check(
                    "backend.role",
                    if role == Some("relay")
                        && upstream
                            .as_ref()
                            .and_then(|payload| payload.as_object())
                            .is_none()
                    {
                        DoctorStatus::Warn
                    } else {
                        DoctorStatus::Ok
                    },
                    match role {
                        Some("primary") => "Backend is running in primary mode.",
                        Some("relay") => "Backend is running in relay mode.",
                        _ => "Backend did not report a cluster role.",
                    },
                    match role {
                        Some("relay") => upstream.as_ref().map(|payload| payload.to_string()),
                        Some(other) => Some(other.to_string()),
                        None => None,
                    },
                    if role == Some("relay")
                        && upstream
                            .as_ref()
                            .and_then(|payload| payload.as_object())
                            .is_none()
                    {
                        Some(
                            "Start a database-connected Memory service on the local network or fix the local database connection."
                                .to_string(),
                        )
                    } else {
                        None
                    },
                    false,
                ));
                match api.project_overview(project).await {
                    Ok(overview) => {
                        report.push(doctor_check(
                            "backend.project_overview",
                            DoctorStatus::Ok,
                            "Project overview endpoint is reachable.",
                            Some(format!(
                                "{} memories / {} raw captures",
                                overview.memory_entries_total, overview.raw_captures_total
                            )),
                            None,
                            false,
                        ));
                        if overview
                            .automation
                            .as_ref()
                            .is_some_and(|automation| automation.enabled)
                        {
                            let active_watchers = overview
                                .watchers
                                .as_ref()
                                .map(|watchers| watchers.active_count);
                            report.push(doctor_check(
                                "backend.watchers",
                                if active_watchers.unwrap_or(0) > 0 {
                                    DoctorStatus::Ok
                                } else {
                                    DoctorStatus::Warn
                                },
                                if active_watchers.unwrap_or(0) > 0 {
                                    "At least one active watcher is visible to the backend."
                                } else {
                                    "Automation is enabled but no active watcher is visible."
                                },
                                active_watchers.map(|count| format!("{count} active watcher(s)")),
                                if active_watchers.unwrap_or(0) > 0 {
                                    None
                                } else {
                                    Some(format!("memory watcher enable --project {}", project))
                                },
                                false,
                            ));
                        }

                        if repo_root.join(".git").exists() {
                            match api.project_commits(project, 1, 0).await {
                                Ok(commits) => report.push(doctor_check(
                                    "history.commit_sync",
                                    if commits.total > 0 {
                                        DoctorStatus::Ok
                                    } else {
                                        DoctorStatus::Warn
                                    },
                                    if commits.total > 0 {
                                        "Commit history has been imported for this project."
                                    } else {
                                        "No commit history has been imported for this project."
                                    },
                                    Some(format!("{} stored commit(s)", commits.total)),
                                    if commits.total > 0 {
                                        None
                                    } else {
                                        Some(format!("memory commits sync --project {}", project))
                                    },
                                    false,
                                )),
                                Err(error) => report.push(doctor_check(
                                    "history.commit_sync",
                                    DoctorStatus::Warn,
                                    "Could not load project commit history.",
                                    Some(error.to_string()),
                                    Some(format!("memory commits sync --project {}", project)),
                                    false,
                                )),
                            }
                        }
                    }
                    Err(error) => report.push(doctor_check(
                        "backend.project_overview",
                        DoctorStatus::Warn,
                        "Project overview endpoint did not return data.",
                        Some(error.to_string()),
                        Some(format!("memory init --project {}", project)),
                        false,
                    )),
                }

                let (http_status, http_details) = tcp_endpoint_status(&config.service.bind_addr);
                report.push(doctor_check(
                    "backend.http_endpoint",
                    if matches!(http_status, DoctorStatus::Fail) {
                        DoctorStatus::Fail
                    } else {
                        DoctorStatus::Ok
                    },
                    "Configured HTTP endpoint is reachable.",
                    Some(http_details),
                    None,
                    false,
                ));

                let (tcp_status, tcp_details) = tcp_endpoint_status(&config.service.capnp_tcp_addr);
                report.push(doctor_check(
                    "backend.capnp_tcp_endpoint",
                    if matches!(tcp_status, DoctorStatus::Fail) {
                        DoctorStatus::Fail
                    } else {
                        DoctorStatus::Ok
                    },
                    "Configured Cap'n Proto TCP endpoint has a listener.",
                    Some(tcp_details),
                    None,
                    false,
                ));

                let (unix_status, unix_details) =
                    unix_socket_status(&config.service.capnp_unix_socket);
                report.push(doctor_check(
                    "backend.capnp_unix_socket",
                    if matches!(unix_status, DoctorStatus::Fail) {
                        DoctorStatus::Fail
                    } else {
                        DoctorStatus::Ok
                    },
                    "Configured Cap'n Proto Unix socket path is active.",
                    Some(unix_details),
                    None,
                    false,
                ));
            }
            Err(error) => {
                report.push(doctor_check(
                    "backend.health",
                    DoctorStatus::Fail,
                    "Backend health endpoint is not reachable.",
                    Some(error.to_string()),
                    Some(backend_start_hint(&config_path)),
                    false,
                ));
                report.push(doctor_check(
                    "backend.project_overview",
                    DoctorStatus::Skipped,
                    "Skipped project overview because the backend is unavailable.",
                    None,
                    None,
                    false,
                ));
                report.push(doctor_check(
                    "history.commit_sync",
                    DoctorStatus::Skipped,
                    "Skipped commit history check because the backend is unavailable.",
                    None,
                    None,
                    false,
                ));

                let (http_status, http_details) = tcp_endpoint_status(&config.service.bind_addr);
                report.push(doctor_check(
                    "backend.http_endpoint",
                    http_status,
                    "Configured HTTP endpoint is not serving Memory Layer health.",
                    Some(http_details),
                    Some(format!(
                        "Start the intended backend for {} or change [service].bind_addr",
                        project
                    )),
                    false,
                ));

                let (tcp_status, tcp_details) = tcp_endpoint_status(&config.service.capnp_tcp_addr);
                report.push(doctor_check(
                    "backend.capnp_tcp_endpoint",
                    tcp_status,
                    "Configured Cap'n Proto TCP endpoint is not confirmed healthy.",
                    Some(tcp_details),
                    Some(format!(
                        "Start the intended backend for {} or change [service].capnp_tcp_addr",
                        project
                    )),
                    false,
                ));

                let (unix_status, unix_details) =
                    unix_socket_status(&config.service.capnp_unix_socket);
                report.push(doctor_check(
                    "backend.capnp_unix_socket",
                    unix_status,
                    "Configured Cap'n Proto Unix socket is not confirmed healthy.",
                    Some(unix_details),
                    Some(format!(
                        "Start the intended backend for {} or change [service].capnp_unix_socket",
                        project
                    )),
                    false,
                ));
            }
        }

        match load_state(project, &resolved_repo_root, &config.automation).await {
            Ok(state) => report.push(doctor_check(
                "automation.state",
                if config.automation.enabled {
                    DoctorStatus::Ok
                } else {
                    DoctorStatus::Skipped
                },
                if config.automation.enabled {
                    "Automation state can be loaded."
                } else {
                    "Skipped automation state because automation is disabled."
                },
                Some(format!(
                    "enabled={} dirty_files={}",
                    state.enabled,
                    state.current_session.changed_files.len()
                )),
                None,
                false,
            )),
            Err(error) => report.push(doctor_check(
                "automation.state",
                if config.automation.enabled {
                    DoctorStatus::Warn
                } else {
                    DoctorStatus::Skipped
                },
                if config.automation.enabled {
                    "Automation state could not be loaded."
                } else {
                    "Skipped automation state because automation is disabled."
                },
                Some(error.to_string()),
                Some("memory doctor --fix".to_string()),
                false,
            )),
        }

        let remember_prereqs = detect_changed_files().is_ok();
        report.push(doctor_check(
            "workflow.remember_ready",
            if remember_prereqs {
                DoctorStatus::Ok
            } else {
                DoctorStatus::Warn
            },
            if remember_prereqs {
                "Remember workflow prerequisites look usable."
            } else {
                "Remember workflow could not inspect repo state."
            },
            None,
            if remember_prereqs {
                None
            } else {
                Some("Ensure git is available and run inside the repo".to_string())
            },
            false,
        ));

        if repo_uses_go_skill_runtime(repo_root) {
            let go_available = go_runtime_available();
            report.push(doctor_check(
                "workflow.skill_runtime_go",
                if go_available {
                    DoctorStatus::Ok
                } else {
                    DoctorStatus::Warn
                },
                if go_available {
                    "Go runtime is available for the repo-local memory skill helper."
                } else {
                    "Repo-local memory skills require `go run`, but Go is not available."
                },
                None,
                if go_available {
                    None
                } else {
                    Some(
                        "Install Go and ensure `go` is on PATH before using the repo-local memory skills."
                            .to_string(),
                    )
                },
                false,
            ));
        } else {
            report.push(doctor_check(
                "workflow.skill_runtime_go",
                DoctorStatus::Skipped,
                "Skipped Go runtime check because the repo-local memory skill helper is not installed.",
                None,
                None,
                false,
            ));
        }
    } else {
        for (id, summary) in [
            (
                "config.database_url",
                "Skipped database URL validation because config could not load.",
            ),
            (
                "config.api_token",
                "Skipped API token validation because config could not load.",
            ),
            (
                "automation.runtime_dir",
                "Skipped automation runtime checks because config could not load.",
            ),
            (
                "config.llm_model",
                "Skipped LLM model validation because config could not load.",
            ),
            (
                "config.llm_api_key",
                "Skipped LLM API key validation because config could not load.",
            ),
            (
                "automation.repo_root",
                "Skipped automation repo_root check because config could not load.",
            ),
            (
                "backend.health",
                "Skipped backend health check because config could not load.",
            ),
            (
                "backend.project_overview",
                "Skipped project overview check because config could not load.",
            ),
            (
                "automation.state",
                "Skipped automation state check because config could not load.",
            ),
            (
                "workflow.remember_ready",
                "Skipped remember readiness check because config could not load.",
            ),
            (
                "workflow.skill_runtime_go",
                "Skipped skill helper Go runtime check because config could not load.",
            ),
        ] {
            report.push(doctor_check(
                id,
                DoctorStatus::Skipped,
                summary,
                None,
                None,
                false,
            ));
        }
    }

    Ok(report)
}

fn repair_repo_bootstrap(repo_root: &Path, project: &str) -> Result<()> {
    let mem_dir = repo_root.join(".mem");
    let runtime_dir = mem_dir.join("runtime");
    let config_path = mem_dir.join("config.toml");
    let project_path = mem_dir.join("project.toml");
    let local_gitignore_path = mem_dir.join(".gitignore");
    let agent_config_path = repo_root.join(".agents").join("memory-layer.toml");
    let skill_root = repo_root.join(".agents").join("skills");

    fs::create_dir_all(&runtime_dir).context("create .mem/runtime")?;
    if !config_path.exists() {
        fs::write(&config_path, render_repo_config(repo_root)).context("write .mem/config.toml")?;
    }
    if !project_path.exists() {
        fs::write(&project_path, render_project_metadata(project, repo_root))
            .context("write .mem/project.toml")?;
    }
    if !local_gitignore_path.exists() {
        fs::write(&local_gitignore_path, "runtime/\n").context("write .mem/.gitignore")?;
    }
    if !agent_config_path.exists() {
        if let Some(parent) = agent_config_path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        fs::write(
            &agent_config_path,
            render_agent_project_config(project, repo_root),
        )
        .context("write .agents/memory-layer.toml")?;
    }
    if missing_memory_skill_dirs(&skill_root).next().is_some() {
        let skill_template_dir = discover_skill_template_dir().ok_or_else(|| {
            anyhow::anyhow!("could not locate packaged memory-layer skill template")
        })?;
        sync_memory_skill_bundle(&skill_template_dir, &skill_root, false)?;
    }
    ensure_root_gitignore_entry(&repo_root.join(".gitignore"), "/.mem\n")?;
    Ok(())
}

fn root_gitignore_contains_mem(repo_root: &Path) -> Result<bool> {
    let path = repo_root.join(".gitignore");
    if !path.exists() {
        return Ok(false);
    }
    let content = fs::read_to_string(path)?;
    Ok(content.lines().any(|line| line.trim() == "/.mem"))
}

fn is_placeholder_database_url(value: &str) -> bool {
    value.contains("<password>") || value.trim().is_empty()
}

fn mask_database_url(value: &str) -> String {
    if let Some((prefix, rest)) = value.split_once("://") {
        if let Some((creds, suffix)) = rest.split_once('@') {
            if creds.contains(':') {
                return format!("{prefix}://<redacted>@{suffix}");
            }
        }
    }
    value.to_string()
}

fn automation_runtime_dir(config: &AppConfig, repo_root: &Path) -> PathBuf {
    if let Some(path) = &config.automation.state_file_path {
        PathBuf::from(path)
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| repo_root.join(".mem").join("runtime"))
    } else if let Some(path) = &config.automation.audit_log_path {
        PathBuf::from(path)
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| repo_root.join(".mem").join("runtime"))
    } else {
        repo_root.join(".mem").join("runtime")
    }
}

#[derive(Clone, Debug, Default)]
struct LocalServiceOverrides {
    bind_addr: String,
    capnp_tcp_addr: String,
    capnp_unix_socket: String,
}

impl LocalServiceOverrides {
    fn is_enabled(&self) -> bool {
        !self.bind_addr.trim().is_empty()
            || !self.capnp_tcp_addr.trim().is_empty()
            || !self.capnp_unix_socket.trim().is_empty()
    }
}

fn default_local_service_overrides(repo_root: &Path) -> LocalServiceOverrides {
    LocalServiceOverrides {
        bind_addr: "127.0.0.1:4140".to_string(),
        capnp_tcp_addr: "127.0.0.1:4141".to_string(),
        capnp_unix_socket: repo_root
            .join(".mem")
            .join("runtime")
            .join("memory-layer.capnp.sock")
            .display()
            .to_string(),
    }
}

fn read_local_service_overrides(repo_root: &Path) -> Option<LocalServiceOverrides> {
    let config_path = repo_root.join(".mem").join("config.toml");
    let content = fs::read_to_string(config_path).ok()?;
    let mut in_service = false;
    let mut overrides = LocalServiceOverrides::default();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_service = trimmed == "[service]";
            continue;
        }
        if !in_service {
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("bind_addr = ") {
            overrides.bind_addr = value.trim_matches('"').to_string();
        } else if let Some(value) = trimmed.strip_prefix("capnp_tcp_addr = ") {
            overrides.capnp_tcp_addr = value.trim_matches('"').to_string();
        } else if let Some(value) = trimmed.strip_prefix("capnp_unix_socket = ") {
            overrides.capnp_unix_socket = value.trim_matches('"').to_string();
        }
    }

    overrides.is_enabled().then_some(overrides)
}

fn tcp_endpoint_status(addr: &str) -> (DoctorStatus, String) {
    match addr.parse::<SocketAddr>() {
        Ok(socket_addr) => {
            match TcpStream::connect_timeout(&socket_addr, Duration::from_millis(250)) {
                Ok(_) => (DoctorStatus::Warn, format!("listener detected on {addr}")),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    (DoctorStatus::Ok, format!("no listener detected on {addr}"))
                }
                Err(error) => (DoctorStatus::Warn, error.to_string()),
            }
        }
        Err(error) => (
            DoctorStatus::Fail,
            format!("invalid socket address: {error}"),
        ),
    }
}

fn unix_socket_status(path: &str) -> (DoctorStatus, String) {
    #[cfg(unix)]
    {
        let socket_path = Path::new(path);
        if !socket_path.exists() {
            return (DoctorStatus::Ok, "socket path is free".to_string());
        }

        match UnixStream::connect(socket_path) {
            Ok(_) => (
                DoctorStatus::Warn,
                format!("listener detected on {}", socket_path.display()),
            ),
            Err(error) => (
                DoctorStatus::Warn,
                format!("path exists but is not accepting connections: {error}"),
            ),
        }
    }

    #[cfg(not(unix))]
    {
        let _ = path;
        (
            DoctorStatus::Skipped,
            "unix socket checks are not available on this platform".to_string(),
        )
    }
}

fn print_doctor_report(report: &DoctorReport) {
    println!(
        "Doctor report for project {} at {}\n",
        report.project, report.repo_root
    );
    if let Some(global_config_path) = &report.global_config_path {
        println!("Merged global config: {global_config_path}");
    } else {
        println!(
            "Merged global config: <not found> (expected at {})",
            default_global_config_path_label()
        );
    }
    println!("Repo-local config: {}\n", report.config_path);
    for check in &report.checks {
        let icon = match check.status {
            DoctorStatus::Ok => "OK",
            DoctorStatus::Warn => "WARN",
            DoctorStatus::Fail => "FAIL",
            DoctorStatus::Skipped => "SKIP",
        };
        println!("[{icon}] {} - {}", check.id, check.summary);
        if let Some(details) = &check.details {
            println!("  details: {details}");
        }
        if let Some(fix) = &check.suggested_fix {
            println!("  fix: {fix}");
        }
        if check.fix_applied {
            println!("  applied: true");
        }
    }

    let ok = report
        .checks
        .iter()
        .filter(|check| check.status == DoctorStatus::Ok)
        .count();
    let warn = report
        .checks
        .iter()
        .filter(|check| check.status == DoctorStatus::Warn)
        .count();
    let fail = report
        .checks
        .iter()
        .filter(|check| check.status == DoctorStatus::Fail)
        .count();
    let skipped = report
        .checks
        .iter()
        .filter(|check| check.status == DoctorStatus::Skipped)
        .count();
    println!("\nSummary: {ok} ok, {warn} warn, {fail} fail, {skipped} skipped");
}

fn default_global_config_path_label() -> String {
    default_global_config_path().display().to_string()
}

fn repo_replacement_policy(repo_root: &Path) -> ReplacementPolicy {
    load_repo_replacement_policy(repo_root).unwrap_or_default()
}

fn initialize_repo(
    repo_root: &Path,
    project: &str,
    force: bool,
    print_only: bool,
) -> Result<String> {
    let mem_dir = repo_root.join(".mem");
    let runtime_dir = mem_dir.join("runtime");
    let config_path = mem_dir.join("config.toml");
    let project_path = mem_dir.join("project.toml");
    let local_gitignore_path = mem_dir.join(".gitignore");
    let root_gitignore_path = repo_root.join(".gitignore");
    let agent_config_path = repo_root.join(".agents").join("memory-layer.toml");
    let skill_root = repo_root.join(".agents").join("skills");
    let skill_template_dir = discover_skill_template_dir()
        .ok_or_else(|| anyhow::anyhow!("could not locate packaged memory-layer skill template"))?;

    let config_contents = render_repo_config(repo_root);
    let project_contents = render_project_metadata(project, repo_root);
    let agent_project_contents = render_agent_project_config(project, repo_root);
    let mem_gitignore_contents = "runtime/\n";
    let root_gitignore_line = "/.mem\n";

    if !print_only {
        fs::create_dir_all(&runtime_dir).context("create .mem/runtime")?;
        if force || !config_path.exists() {
            fs::write(&config_path, config_contents).context("write .mem/config.toml")?;
        }
        if force || !project_path.exists() {
            fs::write(&project_path, project_contents).context("write .mem/project.toml")?;
        }
        if let Some(parent) = agent_config_path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        if force || !agent_config_path.exists() {
            fs::write(&agent_config_path, agent_project_contents)
                .context("write .agents/memory-layer.toml")?;
        }
        if force || !local_gitignore_path.exists() {
            fs::write(&local_gitignore_path, mem_gitignore_contents)
                .context("write .mem/.gitignore")?;
        }
        sync_memory_skill_bundle(&skill_template_dir, &skill_root, force)?;
        ensure_root_gitignore_entry(&root_gitignore_path, root_gitignore_line)?;
    }

    Ok(render_init_summary(
        repo_root,
        project,
        &config_path,
        &project_path,
        &agent_config_path,
        &skill_root,
        print_only,
    ))
}

fn enable_backend_service(_config_path: &Path) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        let config_path = _config_path;
        let plist_path = backend_launch_agent_path()?;
        let _ = bootout_launch_agent(&plist_path, backend_launch_agent_label());
        if plist_path.exists() {
            let _ = fs::remove_file(&plist_path);
        }
        let pid_path = backend_pid_file_path()?;
        let stdout_path = user_memory_layer_log_dir()?.join("mem-service.stdout.log");
        let stderr_path = user_memory_layer_log_dir()?.join("mem-service.stderr.log");
        fs::create_dir_all(
            pid_path
                .parent()
                .ok_or_else(|| anyhow::anyhow!("backend pid path has no parent"))?,
        )
        .with_context(|| format!("create {}", pid_path.display()))?;
        if let Some(pid) = backend_running_pid()? {
            return Ok(format!(
                "Backend process already running.\nPID file: {}\nPID: {}\nConfig: {}",
                pid_path.display(),
                pid,
                config_path.display()
            ));
        }
        let exports = shell_export_prefix()?;
        let program_command = shell_program_invocation(&[
            mem_service_binary_path()?.display().to_string(),
            config_path.display().to_string(),
        ]);
        let shell_command = format!(
            "{exports} nohup {program_command} >>{} 2>>{} </dev/null & echo $! > {}",
            shell_quote_sh(&stdout_path.display().to_string()),
            shell_quote_sh(&stderr_path.display().to_string()),
            shell_quote_sh(&pid_path.display().to_string()),
        );
        let output = ProcessCommand::new("/bin/zsh")
            .args(["-lc", &shell_command])
            .output()
            .context("start backend process")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("start backend process failed: {}", stderr.trim());
        }
        Ok(format!(
            "Started backend process.\nPID file: {}\nConfig: {}\nLogs:\n- {}\n- {}",
            pid_path.display(),
            config_path.display(),
            stdout_path.display(),
            stderr_path.display(),
        ))
    }

    #[cfg(not(target_os = "macos"))]
    {
        run_systemctl_system(["daemon-reload"])?;
        run_systemctl_system(["enable", "--now", "memory-layer.service"])?;
        Ok("Enabled memory-layer.service".to_string())
    }
}

fn preview_enable_backend_service(config_path: &Path) -> String {
    #[cfg(target_os = "macos")]
    {
        match backend_pid_file_path() {
            Ok(pid_path) => format!(
                "Dry run: would enable/start the backend process.\nPID file: {}\nConfig: {}",
                pid_path.display(),
                config_path.display()
            ),
            Err(_) => format!(
                "Dry run: would enable/start the backend process with config {}",
                config_path.display()
            ),
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        format!(
            "Dry run: would run `systemctl enable --now memory-layer.service` using config {}",
            config_path.display()
        )
    }
}

fn disable_backend_service() -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        let plist_path = backend_launch_agent_path()?;
        let _ = bootout_launch_agent(&plist_path, backend_launch_agent_label());
        if plist_path.exists() {
            let _ = fs::remove_file(&plist_path);
        }
        let pid_path = backend_pid_file_path()?;
        if let Some(pid) = backend_running_pid()? {
            let output = ProcessCommand::new("kill")
                .arg(pid.to_string())
                .output()
                .with_context(|| format!("kill backend pid {pid}"))?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("kill {} failed: {}", pid, stderr.trim());
            }
        }
        if pid_path.exists() {
            fs::remove_file(&pid_path).with_context(|| format!("remove {}", pid_path.display()))?;
        }
        Ok(format!(
            "Stopped backend process.\nRemoved pid file: {}",
            pid_path.display()
        ))
    }

    #[cfg(not(target_os = "macos"))]
    {
        run_systemctl_system(["disable", "--now", "memory-layer.service"])?;
        Ok("Disabled memory-layer.service".to_string())
    }
}

fn preview_disable_backend_service(config_path: &Path) -> String {
    #[cfg(target_os = "macos")]
    {
        match backend_pid_file_path() {
            Ok(pid_path) => format!(
                "Dry run: would stop the backend process and remove pid file {}\nConfig: {}",
                pid_path.display(),
                config_path.display()
            ),
            Err(_) => format!(
                "Dry run: would stop the backend process configured by {}",
                config_path.display()
            ),
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        format!(
            "Dry run: would run `systemctl disable --now memory-layer.service` using config {}",
            config_path.display()
        )
    }
}

fn backend_service_status(config_path: &Path) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        let pid_path = backend_pid_file_path()?;
        let pid = backend_running_pid()?;
        Ok(format!(
            "Backend service:\n- pid file: {}\n- config: {}\n- installed: {}\n- running: {}\n- pid: {}\n\nInspect with:\n- tail -f {}",
            pid_path.display(),
            config_path.display(),
            yes_no(pid_path.exists()),
            yes_no(pid.is_some()),
            pid.map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            user_memory_layer_log_dir()?
                .join("mem-service.stderr.log")
                .display(),
        ))
    }

    #[cfg(not(target_os = "macos"))]
    {
        let is_installed = packaged_service_available();
        let is_enabled = run_systemctl_system(["is-enabled", "memory-layer.service"]).is_ok();
        let is_active = run_systemctl_system(["is-active", "memory-layer.service"]).is_ok();
        Ok(format!(
            "Backend service:\n- unit: memory-layer.service\n- config: {}\n- installed: {}\n- enabled: {}\n- active: {}\n\nInspect with:\n- systemctl status memory-layer.service",
            config_path.display(),
            yes_no(is_installed),
            yes_no(is_enabled),
            yes_no(is_active),
        ))
    }
}

fn enable_watch_service(repo_root: &Path, project: &str) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        let plist_path = watch_launch_agent_path(project)?;
        write_launch_agent(
            &plist_path,
            render_watch_launch_agent(repo_root, project)?,
            &watch_launch_agent_label(project),
        )?;
        bootstrap_launch_agent(&plist_path, &watch_launch_agent_label(project))?;
        Ok(format!(
            "Installed and started watcher LaunchAgent {}.\nPlist: {}\nRepo: {}\nProject: {}\n\nManage it with:\n- memory watcher status --project {}\n- memory watcher disable --project {}\n- launchctl kickstart -k {}/{}",
            watch_launch_agent_label(project),
            plist_path.display(),
            repo_root.display(),
            project,
            project,
            project,
            launchctl_domain_target()?,
            watch_launch_agent_label(project),
        ))
    }

    #[cfg(not(target_os = "macos"))]
    {
        let unit_name = watch_unit_name(project);
        let unit_dir = user_systemd_unit_dir()?;
        let unit_path = unit_dir.join(&unit_name);
        fs::create_dir_all(&unit_dir).with_context(|| format!("create {}", unit_dir.display()))?;
        fs::write(&unit_path, render_watch_unit(repo_root, project)?)
            .with_context(|| format!("write {}", unit_path.display()))?;
        run_systemctl_user(["daemon-reload"])?;
        run_systemctl_user(["enable", "--now", &unit_name])?;
        Ok(format!(
            "Installed and started user service {}.\nUnit: {}\nRepo: {}\nProject: {}\n\nManage it with:\n- memory watcher status --project {}\n- memory watcher disable --project {}\n- systemctl --user restart {}",
            unit_name,
            unit_path.display(),
            repo_root.display(),
            project,
            project,
            project,
            unit_name
        ))
    }
}

fn preview_enable_watch_service(repo_root: &Path, project: &str) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        Ok(format!(
            "Dry run: would install and start watcher LaunchAgent {}.\nPlist: {}\nRepo: {}\nProject: {}",
            watch_launch_agent_label(project),
            watch_launch_agent_path(project)?.display(),
            repo_root.display(),
            project,
        ))
    }

    #[cfg(not(target_os = "macos"))]
    {
        let unit_name = watch_unit_name(project);
        let unit_path = user_systemd_unit_dir()?.join(&unit_name);
        Ok(format!(
            "Dry run: would install and start user service {}.\nUnit: {}\nRepo: {}\nProject: {}",
            unit_name,
            unit_path.display(),
            repo_root.display(),
            project,
        ))
    }
}

fn disable_watch_service(project: &str) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        let plist_path = watch_launch_agent_path(project)?;
        let label = watch_launch_agent_label(project);
        let _ = bootout_launch_agent(&plist_path, &label);
        if plist_path.exists() {
            fs::remove_file(&plist_path)
                .with_context(|| format!("remove {}", plist_path.display()))?;
        }
        Ok(format!(
            "Disabled watcher LaunchAgent {}.\nRemoved plist: {}",
            label,
            plist_path.display()
        ))
    }

    #[cfg(not(target_os = "macos"))]
    {
        let unit_name = watch_unit_name(project);
        let unit_path = user_systemd_unit_dir()?.join(&unit_name);
        let _ = run_systemctl_user(["disable", "--now", &unit_name]);
        if unit_path.exists() {
            fs::remove_file(&unit_path)
                .with_context(|| format!("remove {}", unit_path.display()))?;
        }
        run_systemctl_user(["daemon-reload"])?;
        Ok(format!(
            "Disabled user service {}.\nRemoved unit: {}",
            unit_name,
            unit_path.display()
        ))
    }
}

fn preview_disable_watch_service(project: &str) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        Ok(format!(
            "Dry run: would disable watcher LaunchAgent {} and remove {}",
            watch_launch_agent_label(project),
            watch_launch_agent_path(project)?.display(),
        ))
    }

    #[cfg(not(target_os = "macos"))]
    {
        let unit_name = watch_unit_name(project);
        let unit_path = user_systemd_unit_dir()?.join(&unit_name);
        Ok(format!(
            "Dry run: would disable user service {} and remove {}",
            unit_name,
            unit_path.display(),
        ))
    }
}

fn watch_service_status(repo_root: &Path, project: &str) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        let plist_path = watch_launch_agent_path(project)?;
        let label = watch_launch_agent_label(project);
        let status = launch_agent_status(&label)?;
        Ok(format!(
            "Watcher service for project {}:\n- label: {}\n- plist: {}\n- repo: {}\n- installed: {}\n- loaded: {}\n- running: {}\n\nInspect with:\n- launchctl print {}/{}",
            project,
            label,
            plist_path.display(),
            repo_root.display(),
            yes_no(plist_path.exists()),
            yes_no(status.loaded),
            yes_no(status.running),
            launchctl_domain_target()?,
            label
        ))
    }

    #[cfg(not(target_os = "macos"))]
    {
        let unit_name = watch_unit_name(project);
        let unit_path = user_systemd_unit_dir()?.join(&unit_name);
        let is_enabled = run_systemctl_user(["is-enabled", &unit_name]).is_ok();
        let is_active = run_systemctl_user(["is-active", &unit_name]).is_ok();
        Ok(format!(
            "Watcher service for project {}:\n- unit: {}\n- repo: {}\n- installed: {}\n- enabled: {}\n- active: {}\n\nInspect with:\n- systemctl --user status {}",
            project,
            unit_path.display(),
            repo_root.display(),
            yes_no(unit_path.exists()),
            yes_no(is_enabled),
            yes_no(is_active),
            unit_name
        ))
    }
}

#[cfg(not(target_os = "macos"))]
fn render_watch_unit(repo_root: &Path, project: &str) -> Result<String> {
    let memory_binary = memory_binary_path()?;
    let env_file = user_memory_layer_env_file()?;
    let working_directory = repo_root
        .canonicalize()
        .with_context(|| format!("canonicalize {}", repo_root.display()))?;
    Ok(format!(
        "[Unit]\nDescription=Memory Layer Watcher ({project})\nAfter=default.target\n\n[Service]\nType=simple\nEnvironmentFile=-{}\nEnvironment=MEMORY_LAYER_WATCH_SERVICE_MANAGED=1\nWorkingDirectory={}\nExecStart={} --config {} watcher run --project {}\nRestart=on-failure\nRestartSec=2\n\n[Install]\nWantedBy=default.target\n",
        shell_escape_path(&env_file),
        working_directory.display(),
        shell_escape_path(&memory_binary),
        shell_escape_path(&default_global_config_path()),
        shell_escape_str(project),
    ))
}

#[cfg(not(target_os = "macos"))]
fn user_systemd_unit_dir() -> Result<PathBuf> {
    if let Ok(config_home) = env::var("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(config_home).join("systemd").join("user"));
    }
    let home = env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join("systemd")
        .join("user"))
}

fn user_memory_layer_env_file() -> Result<PathBuf> {
    platform::preferred_user_env_path().ok_or_else(|| anyhow::anyhow!("HOME is not set"))
}

fn memory_binary_path() -> Result<PathBuf> {
    Ok(platform::current_exe_sibling_binary("memory")
        .or_else(|| std::env::current_exe().ok())
        .unwrap_or_else(|| PathBuf::from("memory")))
}

#[cfg(not(target_os = "macos"))]
fn watch_unit_name(project: &str) -> String {
    platform::watch_service_unit_name(project)
}

#[cfg(target_os = "macos")]
fn sanitize_service_fragment(value: &str) -> String {
    platform::sanitize_service_fragment(value)
}

#[cfg(target_os = "macos")]
#[derive(Debug, Default)]
struct LaunchAgentStatus {
    loaded: bool,
    running: bool,
}

#[cfg(target_os = "macos")]
fn backend_launch_agent_label() -> &'static str {
    platform::backend_launch_agent_label()
}

#[cfg(target_os = "macos")]
fn watch_launch_agent_label(project: &str) -> String {
    platform::watch_launch_agent_label(project)
}

#[cfg(target_os = "macos")]
fn backend_launch_agent_path() -> Result<PathBuf> {
    platform::backend_launch_agent_path().ok_or_else(|| anyhow::anyhow!("HOME is not set"))
}

#[cfg(target_os = "macos")]
fn backend_pid_file_path() -> Result<PathBuf> {
    platform::backend_pid_file_path().ok_or_else(|| anyhow::anyhow!("HOME is not set"))
}

#[cfg(target_os = "macos")]
fn watch_launch_agent_path(project: &str) -> Result<PathBuf> {
    platform::watch_launch_agent_path(project).ok_or_else(|| anyhow::anyhow!("HOME is not set"))
}

#[cfg(target_os = "macos")]
fn user_launch_agents_dir() -> Result<PathBuf> {
    platform::user_launch_agents_dir().ok_or_else(|| anyhow::anyhow!("HOME is not set"))
}

#[cfg(target_os = "macos")]
fn macos_app_support_dir() -> Option<PathBuf> {
    platform::macos_app_support_dir()
}

#[cfg(target_os = "macos")]
fn user_memory_layer_log_dir() -> Result<PathBuf> {
    platform::user_memory_layer_log_dir().ok_or_else(|| anyhow::anyhow!("HOME is not set"))
}

#[cfg(target_os = "macos")]
fn launchctl_domain_target() -> Result<String> {
    let output = ProcessCommand::new("id")
        .arg("-u")
        .output()
        .context("run id -u")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("id -u failed: {}", stderr.trim());
    }
    let uid = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(format!("gui/{uid}"))
}

#[cfg(target_os = "macos")]
fn backend_running_pid() -> Result<Option<u32>> {
    let pid_path = backend_pid_file_path()?;
    if !pid_path.exists() {
        return Ok(None);
    }
    let content =
        fs::read_to_string(&pid_path).with_context(|| format!("read {}", pid_path.display()))?;
    let pid = match content.trim().parse::<u32>() {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    let status = ProcessCommand::new("kill")
        .args(["-0", &pid.to_string()])
        .output()
        .with_context(|| format!("check backend pid {pid}"))?;
    if status.status.success() {
        Ok(Some(pid))
    } else {
        Ok(None)
    }
}

#[cfg(target_os = "macos")]
fn write_launch_agent(path: &Path, contents: String, label: &str) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("launch agent path has no parent"))?;
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    fs::write(path, contents).with_context(|| format!("write {}", path.display()))?;
    let _ = bootout_launch_agent(path, label);
    Ok(())
}

#[cfg(target_os = "macos")]
fn bootstrap_launch_agent(path: &Path, label: &str) -> Result<()> {
    run_launchctl([
        "bootstrap",
        &launchctl_domain_target()?,
        &path.display().to_string(),
    ])?;
    run_launchctl([
        "kickstart",
        "-k",
        &format!("{}/{}", launchctl_domain_target()?, label),
    ])?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn bootout_launch_agent(path: &Path, label: &str) -> Result<()> {
    let target = format!("{}/{}", launchctl_domain_target()?, label);
    if run_launchctl(["bootout", &target]).is_ok() {
        return Ok(());
    }
    run_launchctl([
        "bootout",
        &launchctl_domain_target()?,
        &path.display().to_string(),
    ])
}

#[cfg(target_os = "macos")]
fn launch_agent_status(label: &str) -> Result<LaunchAgentStatus> {
    let target = format!("{}/{}", launchctl_domain_target()?, label);
    let output = ProcessCommand::new("launchctl")
        .args(["print", &target])
        .output()
        .with_context(|| format!("run launchctl print {target}"))?;
    if !output.status.success() {
        return Ok(LaunchAgentStatus::default());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(LaunchAgentStatus {
        loaded: true,
        running: stdout.contains("state = running") || stdout.contains("\"PID\" ="),
    })
}

#[cfg(target_os = "macos")]
fn run_launchctl<const N: usize>(args: [&str; N]) -> Result<()> {
    let output = ProcessCommand::new("launchctl")
        .args(args)
        .output()
        .with_context(|| format!("run launchctl {}", args.join(" ")))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    anyhow::bail!(
        "launchctl {} failed: {}{}{}",
        args.join(" "),
        stderr.trim(),
        if stderr.trim().is_empty() || stdout.trim().is_empty() {
            ""
        } else {
            " | "
        },
        stdout.trim()
    )
}

#[cfg(target_os = "macos")]
fn render_backend_launch_agent(config_path: &Path) -> Result<String> {
    let binary = memory_binary_path()?;
    let working_directory =
        macos_app_support_dir().ok_or_else(|| anyhow::anyhow!("HOME is not set"))?;
    let stdout_path = user_memory_layer_log_dir()?.join("mem-service.stdout.log");
    let stderr_path = user_memory_layer_log_dir()?.join("mem-service.stderr.log");
    let command = launch_agent_shell_command(&[
        binary.display().to_string(),
        "--config".to_string(),
        config_path.display().to_string(),
        "service".to_string(),
        "run".to_string(),
    ])?;
    render_launch_agent_plist(
        backend_launch_agent_label(),
        &working_directory,
        &command,
        &stdout_path,
        &stderr_path,
    )
}

#[cfg(target_os = "macos")]
fn render_watch_launch_agent(repo_root: &Path, project: &str) -> Result<String> {
    let binary = memory_binary_path()?;
    let working_directory = repo_root
        .canonicalize()
        .with_context(|| format!("canonicalize {}", repo_root.display()))?;
    let log_dir = user_memory_layer_log_dir()?;
    let sanitized = sanitize_service_fragment(project);
    let stdout_path = log_dir.join(format!("memory-watch-{sanitized}.stdout.log"));
    let stderr_path = log_dir.join(format!("memory-watch-{sanitized}.stderr.log"));
    let command = launch_agent_shell_command(&[
        binary.display().to_string(),
        "--config".to_string(),
        default_global_config_path().display().to_string(),
        "watcher".to_string(),
        "run".to_string(),
        "--project".to_string(),
        project.to_string(),
    ])?;
    let command = format!("export MEMORY_LAYER_WATCH_SERVICE_MANAGED=1; {command}");
    render_launch_agent_plist(
        &watch_launch_agent_label(project),
        &working_directory,
        &command,
        &stdout_path,
        &stderr_path,
    )
}

#[cfg(target_os = "macos")]
fn shell_export_prefix() -> Result<String> {
    let env_vars = launch_agent_environment_variables()?;
    let mut command = String::new();
    for (key, value) in env_vars {
        command.push_str("export ");
        command.push_str(&key);
        command.push('=');
        command.push_str(&shell_quote_sh(&value));
        command.push_str("; ");
    }
    Ok(command)
}

#[cfg(target_os = "macos")]
fn shell_program_invocation(program_arguments: &[String]) -> String {
    let mut command = String::new();
    let mut first = true;
    for arg in program_arguments {
        if !first {
            command.push(' ');
        }
        first = false;
        command.push_str(&shell_quote_sh(arg));
    }
    command
}

#[cfg(target_os = "macos")]
fn shell_command_for_program(program_arguments: &[String], exec_program: bool) -> Result<String> {
    let mut command = shell_export_prefix()?;
    if exec_program {
        command.push_str("exec");
        command.push(' ');
    }
    command.push_str(&shell_program_invocation(program_arguments));
    Ok(command)
}

#[cfg(target_os = "macos")]
fn launch_agent_shell_command(program_arguments: &[String]) -> Result<String> {
    shell_command_for_program(program_arguments, true)
}

#[cfg(target_os = "macos")]
fn launch_agent_environment_variables() -> Result<BTreeMap<String, String>> {
    let mut values = BTreeMap::new();
    values.insert(
        "HOME".to_string(),
        env::var("HOME").context("HOME is not set")?,
    );
    values.insert(
        "PATH".to_string(),
        env::var("PATH")
            .unwrap_or_else(|_| "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin".to_string()),
    );
    if let Ok(user) = env::var("USER") {
        values.insert("USER".to_string(), user.clone());
        values.insert("LOGNAME".to_string(), user);
    }
    let env_file = user_memory_layer_env_file()?;
    if !env_file.exists() {
        return Ok(values);
    }
    let content =
        fs::read_to_string(&env_file).with_context(|| format!("read {}", env_file.display()))?;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            values.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    Ok(values)
}

#[cfg(target_os = "macos")]
fn render_launch_agent_plist(
    label: &str,
    working_directory: &Path,
    shell_command: &str,
    stdout_path: &Path,
    stderr_path: &Path,
) -> Result<String> {
    let log_dir = stdout_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("stdout log path has no parent"))?;
    fs::create_dir_all(log_dir).with_context(|| format!("create {}", log_dir.display()))?;
    let env_vars = launch_agent_environment_variables()?;
    let program_arguments = [
        "/bin/zsh".to_string(),
        "-lc".to_string(),
        shell_command.to_string(),
    ];
    let args_xml = program_arguments
        .iter()
        .map(|arg| format!("    <string>{}</string>", xml_escape(arg)))
        .collect::<Vec<_>>()
        .join("\n");
    let env_xml = env_vars
        .iter()
        .map(|(key, value)| {
            format!(
                "    <key>{}</key>\n    <string>{}</string>",
                xml_escape(key),
                xml_escape(value)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
{args_xml}
  </array>
  <key>WorkingDirectory</key>
  <string>{working_directory}</string>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>{stdout_path}</string>
  <key>StandardErrorPath</key>
  <string>{stderr_path}</string>
  <key>EnvironmentVariables</key>
  <dict>
{env_xml}
  </dict>
</dict>
</plist>
"#,
        label = xml_escape(label),
        args_xml = args_xml,
        working_directory = xml_escape(&working_directory.display().to_string()),
        stdout_path = xml_escape(&stdout_path.display().to_string()),
        stderr_path = xml_escape(&stderr_path.display().to_string()),
        env_xml = env_xml,
    ))
}

#[cfg(target_os = "macos")]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(target_os = "macos")]
fn shell_quote_sh(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

#[cfg(not(target_os = "macos"))]
fn run_systemctl_user<const N: usize>(args: [&str; N]) -> Result<()> {
    let output = ProcessCommand::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .with_context(|| format!("run systemctl --user {}", args.join(" ")))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    anyhow::bail!(
        "systemctl --user {} failed: {}{}{}",
        args.join(" "),
        stderr.trim(),
        if stderr.trim().is_empty() || stdout.trim().is_empty() {
            ""
        } else {
            " | "
        },
        stdout.trim()
    )
}

#[cfg(not(target_os = "macos"))]
fn shell_escape_path(value: &Path) -> String {
    shell_escape_str(&value.display().to_string())
}

#[cfg(not(target_os = "macos"))]
fn shell_escape_str(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-'))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

const MEMORY_SKILL_NAMES: &[&str] = &[
    "memory-layer",
    "memory-query-resume",
    "memory-plan-execution",
    "memory-remember",
];

fn missing_memory_skill_dirs<'a>(skill_root: &'a Path) -> impl Iterator<Item = PathBuf> + 'a {
    MEMORY_SKILL_NAMES
        .iter()
        .map(|name| skill_root.join(name))
        .filter(|path| !path.is_dir())
}

fn discover_skill_template_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join(".agents")
            .join("skills"),
    );
    if let Ok(exe) = env::current_exe() {
        if let Some(bin_dir) = exe.parent() {
            if let Some(prefix) = bin_dir.parent() {
                candidates.push(
                    prefix
                        .join("share")
                        .join("memory-layer")
                        .join("skill-template"),
                );
            }
        }
    }
    if let Ok(data_home) = env::var("XDG_DATA_HOME") {
        candidates.push(
            PathBuf::from(data_home)
                .join("memory-layer")
                .join("skill-template"),
        );
    }
    if let Ok(home) = env::var("HOME") {
        candidates.push(
            PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("memory-layer")
                .join("skill-template"),
        );
    }
    candidates.push(PathBuf::from("/usr/share/memory-layer/skill-template"));

    candidates.into_iter().find(|path| path.is_dir())
}

fn sync_memory_skill_bundle(src_root: &Path, dest_root: &Path, force: bool) -> Result<()> {
    fs::create_dir_all(dest_root).with_context(|| format!("create {}", dest_root.display()))?;
    for skill_name in MEMORY_SKILL_NAMES {
        let src = src_root.join(skill_name);
        if !src.is_dir() {
            anyhow::bail!("skill template is missing {}", src.display());
        }
        let dest = dest_root.join(skill_name);
        if dest.exists() {
            if force {
                fs::remove_dir_all(&dest).with_context(|| format!("remove {}", dest.display()))?;
            } else {
                continue;
            }
        }
        copy_directory_tree(&src, &dest)?;
    }
    Ok(())
}

fn copy_directory_tree(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest).with_context(|| format!("create {}", dest.display()))?;
    for entry in fs::read_dir(src).with_context(|| format!("read {}", src.display()))? {
        let entry = entry.with_context(|| format!("read entry in {}", src.display()))?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        let file_type = entry
            .file_type()
            .with_context(|| format!("read type for {}", src_path.display()))?;
        if file_type.is_dir() {
            copy_directory_tree(&src_path, &dest_path)?;
        } else if file_type.is_file() {
            fs::copy(&src_path, &dest_path).with_context(|| {
                format!("copy {} -> {}", src_path.display(), dest_path.display())
            })?;
            let mode = if src_path.extension().and_then(|ext| ext.to_str()) == Some("sh") {
                0o755
            } else {
                0o644
            };
            set_copied_file_permissions(&dest_path, mode)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path) -> Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("chmod {}", path.display()))
}

#[cfg(not(unix))]
fn set_private_file_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_copied_file_permissions(path: &Path, mode: u32) -> Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .with_context(|| format!("chmod {}", path.display()))
}

#[cfg(not(unix))]
fn set_copied_file_permissions(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
}

fn render_repo_config(repo_root: &Path) -> String {
    let repo_root = repo_root.display();
    format!(
        r#"# Repo-local overrides for this project.
# Put shared defaults and secrets in the global config:
#   {}
# Shared LLM settings for `memory scan` should also live there under [llm].

# Uncomment [service] to run a repo-local dev backend alongside the shared one.
# Example dev endpoints:
# [service]
# bind_addr = "127.0.0.1:4140"
# capnp_unix_socket = "{repo_root}/.mem/runtime/memory-layer.capnp.sock"
# capnp_tcp_addr = "127.0.0.1:4141"

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
"#,
        default_global_config_path_label()
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

pub(crate) fn render_agent_project_config(project: &str, repo_root: &Path) -> String {
    format!(
        r#"# Project-owned memory behavior.
# Less technical users should customize Memory Layer here.

[project]
slug = "{project}"
repo_root = "{}"

[capture]
include_paths = ["README.md", "docs/", "src/", "crates/", "scripts/", "packaging/"]
ignore_paths = [".git/", "target/", ".mem/", "node_modules/"]

[analysis]
analyzers = ["rust", "typescript", "python"]

[retrieval]
graph_enabled = false

[curation]
replacement_policy = "balanced"
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

    if !content
        .lines()
        .any(|existing| existing.trim() == line.trim())
    {
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
    agent_config_path: &Path,
    skills_root: &Path,
    print_only: bool,
) -> String {
    let action = if print_only {
        "Would prepare"
    } else {
        "Prepared"
    };
    format!(
        "{action} repo-local memory bootstrap for project `{project}` at {}.\n\nFiles:\n- {}\n- {}\n- {}\n- {}/runtime/\n- {} (bundled memory skills)\n\nNext steps:\n1. Set shared values like `database.url`, `service.api_token`, and `[llm]` config in {}\n2. Use {} for repo-specific runtime overrides\n3. Use {} to customize project memory behavior\n4. Start the shared backend if it is not already running:\n   memory service run --config {}\n5. Optional: configure repo-local [service] overrides if you want a parallel dev backend for this repo\n6. Optional: run a project scan:\n   memory scan --project {}\n7. Optional: enable the per-repo watcher user service:\n   memory watcher enable --project {}\n8. Open the TUI:\n   memory tui --project {}\n9. Use the repo-local memory skill bundle from {} (umbrella skill at {}/memory-layer)",
        repo_root.display(),
        config_path.display(),
        project_path.display(),
        agent_config_path.display(),
        config_path.parent().unwrap_or(repo_root).display(),
        skills_root.display(),
        default_global_config_path_label(),
        config_path.display(),
        agent_config_path.display(),
        default_global_config_path().display(),
        project,
        project,
        project,
        skills_root.display(),
        skills_root.display()
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

    pub(crate) async fn replacement_proposals(
        &self,
        project: &str,
    ) -> Result<mem_api::ReplacementProposalListResponse> {
        get_json(
            self.client
                .get(service_url(
                    &self.config,
                    &format!("/v1/projects/{project}/replacement-proposals"),
                ))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn approve_replacement_proposal(
        &self,
        project: &str,
        proposal_id: Uuid,
    ) -> Result<mem_api::ReplacementProposalResolutionResponse> {
        get_json(
            self.client
                .post(service_url(
                    &self.config,
                    &format!("/v1/projects/{project}/replacement-proposals/{proposal_id}/approve"),
                ))
                .headers(write_headers(&self.config)?)
                .json(&serde_json::json!({}))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn reject_replacement_proposal(
        &self,
        project: &str,
        proposal_id: Uuid,
    ) -> Result<mem_api::ReplacementProposalResolutionResponse> {
        get_json(
            self.client
                .post(service_url(
                    &self.config,
                    &format!("/v1/projects/{project}/replacement-proposals/{proposal_id}/reject"),
                ))
                .headers(write_headers(&self.config)?)
                .json(&serde_json::json!({}))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn resume(&self, request: &ResumeRequest) -> Result<ResumeResponse> {
        get_json(
            self.client
                .post(service_url(
                    &self.config,
                    &format!("/v1/projects/{}/resume", request.project),
                ))
                .json(request)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn project_commits(
        &self,
        project: &str,
        limit: i64,
        offset: i64,
    ) -> Result<ProjectCommitsResponse> {
        get_json(
            self.client
                .get(service_url(
                    &self.config,
                    &format!("/v1/projects/{project}/commits?limit={limit}&offset={offset}"),
                ))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn project_commit(
        &self,
        project: &str,
        commit: &str,
    ) -> Result<CommitDetailResponse> {
        get_json(
            self.client
                .get(service_url(
                    &self.config,
                    &format!("/v1/projects/{project}/commits/{commit}"),
                ))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn export_bundle_preview(
        &self,
        project: &str,
        options: &ProjectMemoryExportOptions,
    ) -> Result<ProjectMemoryBundlePreview> {
        get_json(
            self.client
                .post(service_url(
                    &self.config,
                    &format!("/v1/projects/{project}/bundle/export/preview"),
                ))
                .headers(write_headers(&self.config)?)
                .json(options)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn export_bundle(
        &self,
        project: &str,
        options: &ProjectMemoryExportOptions,
    ) -> Result<Vec<u8>> {
        let response = self
            .client
            .post(service_url(
                &self.config,
                &format!("/v1/projects/{project}/bundle/export"),
            ))
            .headers(write_headers(&self.config)?)
            .json(options)
            .send()
            .await?;
        let status = response.status();
        let bytes = response.bytes().await?;
        if !status.is_success() {
            anyhow::bail!("{status} {}", String::from_utf8_lossy(&bytes));
        }
        Ok(bytes.to_vec())
    }

    pub(crate) async fn import_bundle_preview(
        &self,
        project: &str,
        bytes: Vec<u8>,
    ) -> Result<ProjectMemoryImportPreview> {
        get_json(
            self.client
                .post(service_url(
                    &self.config,
                    &format!("/v1/projects/{project}/bundle/import/preview"),
                ))
                .headers(write_headers(&self.config)?)
                .header("content-type", "application/octet-stream")
                .body(bytes)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn import_bundle(
        &self,
        project: &str,
        bytes: Vec<u8>,
    ) -> Result<ProjectMemoryImportResponse> {
        get_json(
            self.client
                .post(service_url(
                    &self.config,
                    &format!("/v1/projects/{project}/bundle/import"),
                ))
                .headers(write_headers(&self.config)?)
                .header("content-type", "application/octet-stream")
                .body(bytes)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn query(&self, request: &QueryRequest) -> Result<QueryResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/query"))
                .json(request)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn log_scan_activity(&self, request: &ScanActivityRequest) -> Result<()> {
        let response = self
            .client
            .post(service_url(&self.config, "/v1/scan/activity"))
            .headers(write_headers(&self.config)?)
            .json(request)
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            anyhow::bail!("{status} {body}");
        }
        Ok(())
    }

    pub(crate) async fn log_checkpoint_activity(
        &self,
        request: &CheckpointActivityRequest,
    ) -> Result<()> {
        let response = self
            .client
            .post(service_url(&self.config, "/v1/checkpoint/activity"))
            .headers(write_headers(&self.config)?)
            .json(request)
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            anyhow::bail!("{status} {body}");
        }
        Ok(())
    }

    pub(crate) async fn log_plan_activity(&self, request: &PlanActivityRequest) -> Result<()> {
        let response = self
            .client
            .post(service_url(&self.config, "/v1/plan/activity"))
            .headers(write_headers(&self.config)?)
            .json(request)
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            anyhow::bail!("{status} {body}");
        }
        Ok(())
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

    pub(crate) async fn sync_commits(
        &self,
        request: &CommitSyncRequest,
    ) -> Result<CommitSyncResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/commits/sync"))
                .headers(write_headers(&self.config)?)
                .json(request)
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
                .headers(write_headers(&self.config)?)
                .json(request)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn curate(
        &self,
        project: &str,
        replacement_policy: ReplacementPolicy,
        dry_run: bool,
    ) -> Result<CurateResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/curate"))
                .headers(write_headers(&self.config)?)
                .json(&CurateRequest {
                    project: project.to_string(),
                    batch_size: None,
                    replacement_policy: Some(replacement_policy),
                    dry_run,
                })
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn reindex(&self, project: &str, dry_run: bool) -> Result<ReindexResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/reindex"))
                .headers(write_headers(&self.config)?)
                .json(&ReindexRequest {
                    project: project.to_string(),
                    dry_run,
                })
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn reembed(&self, project: &str, dry_run: bool) -> Result<ReembedResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/reembed"))
                .headers(write_headers(&self.config)?)
                .json(&ReembedRequest {
                    project: project.to_string(),
                    dry_run,
                })
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn prune_embeddings(
        &self,
        project: &str,
        dry_run: bool,
    ) -> Result<PruneEmbeddingsResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/prune-embeddings"))
                .headers(write_headers(&self.config)?)
                .json(&PruneEmbeddingsRequest {
                    project: project.to_string(),
                    dry_run,
                })
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn archive_low_value(
        &self,
        project: &str,
        dry_run: bool,
    ) -> Result<ArchiveResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/archive"))
                .headers(write_headers(&self.config)?)
                .json(&ArchiveRequest {
                    project: project.to_string(),
                    max_confidence: 0.3,
                    max_importance: 1,
                    dry_run,
                })
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn delete_memory(&self, memory_id: Uuid) -> Result<DeleteMemoryResponse> {
        get_json(
            self.client
                .delete(service_url(&self.config, "/v1/memory"))
                .headers(write_headers(&self.config)?)
                .json(&DeleteMemoryRequest { memory_id })
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
    println!(
        "Confidence: {:.2} | Evidence: {}\n",
        payload.confidence,
        if payload.insufficient_evidence {
            "insufficient"
        } else {
            "sufficient"
        }
    );
    println!(
        "Diagnostics: lexical {} ({} ms) | semantic {} ({} ms) | merged {} | returned {} | rerank {} ms | total {} ms\n",
        payload.diagnostics.lexical_candidates,
        payload.diagnostics.lexical_duration_ms,
        payload.diagnostics.semantic_candidates,
        payload.diagnostics.semantic_duration_ms,
        payload.diagnostics.merged_candidates,
        payload.diagnostics.returned_results,
        payload.diagnostics.rerank_duration_ms,
        payload.diagnostics.total_duration_ms,
    );
    for result in payload.results {
        println!(
            "- {} [{} / {}] score={:.2}",
            result.summary, result.memory_type, result.match_kind, result.score
        );
        println!("  {}", result.snippet);
        println!(
            "  debug: chunk {:.2} | entry {:.2} | semantic {:.2} | relation {:.2}",
            result.debug.chunk_fts,
            result.debug.entry_fts,
            result.debug.semantic_similarity,
            result.debug.relation_boost,
        );
        if !result.score_explanation.is_empty() {
            println!("  why: {}", result.score_explanation.join(" | "));
        }
        if !result.tags.is_empty() {
            println!("  tags: {}", result.tags.join(", "));
        }
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

fn print_bundle_import_preview(preview: &ProjectMemoryImportPreview) {
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

fn print_bundle_import_response(response: &ProjectMemoryImportResponse) {
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

fn print_resume_response(response: &ResumeResponse) {
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

fn print_plan_execution_finish_report(report: &PlanExecutionFinishReport) {
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

fn print_scan_report(report: &scan::ScanReport) {
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

fn print_index_report(report: &scan::RepoIndexReport) {
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

fn print_index_status(status: &Option<scan::RepoIndexStatus>, project: &str) {
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

fn print_commit_sync_response(response: &CommitSyncResponse) {
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

fn print_project_commits(response: &ProjectCommitsResponse) {
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

fn print_commit_detail(response: &CommitDetailResponse) {
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

fn parse_memory_type(input: String) -> Result<mem_api::MemoryType> {
    match input.as_str() {
        "architecture" => Ok(mem_api::MemoryType::Architecture),
        "convention" => Ok(mem_api::MemoryType::Convention),
        "decision" => Ok(mem_api::MemoryType::Decision),
        "incident" => Ok(mem_api::MemoryType::Incident),
        "debugging" => Ok(mem_api::MemoryType::Debugging),
        "environment" => Ok(mem_api::MemoryType::Environment),
        "domain_fact" => Ok(mem_api::MemoryType::DomainFact),
        "plan" => Ok(mem_api::MemoryType::Plan),
        _ => anyhow::bail!("unknown memory type: {input}"),
    }
}

fn write_headers(config: &AppConfig) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    if let Some(origin) = trusted_local_origin(&config.service.bind_addr) {
        headers.insert(ORIGIN, origin.parse()?);
    } else {
        headers.insert("x-api-token", config.service.api_token.parse()?);
    }
    Ok(headers)
}

fn trusted_local_origin(bind_addr: &str) -> Option<&'static str> {
    let host = bind_addr
        .rsplit_once(':')
        .map(|(host, _)| host.trim_matches('[').trim_matches(']'))
        .unwrap_or(bind_addr);
    match host {
        "127.0.0.1" | "localhost" | "::1" => Some("http://127.0.0.1"),
        _ => None,
    }
}

fn service_url(config: &AppConfig, path: &str) -> String {
    format!("http://{}{}", config.service.bind_addr, path)
}

fn resolve_project_slug(project: Option<String>, cwd: &Path) -> Result<String> {
    if let Some(project) = project {
        return Ok(project);
    }
    let repo_root = resolve_repo_root(cwd)?;
    if let Some(project) = read_repo_project_slug(&repo_root) {
        return Ok(project);
    }
    let Some(name) = repo_root.file_name().and_then(|value| value.to_str()) else {
        anyhow::bail!("could not determine project slug from current directory");
    };
    Ok(name.to_string())
}

fn build_remember_request(
    args: RememberArgs,
    project: &str,
    writer_id: &str,
    writer_name: Option<&str>,
) -> Result<CaptureTaskRequest> {
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
        writer_id: writer_id.to_string(),
        writer_name: writer_name.map(|value| value.to_string()),
        agent_summary: summary,
        files_changed,
        git_diff_summary: None,
        tests,
        notes: args.notes,
        structured_candidates: Vec::new(),
        command_output,
        idempotency_key: None,
        dry_run: false,
    })
}

async fn save_checkpoint_with_activity(
    api: &ApiClient,
    project: &str,
    repo_root: &Path,
    note: Option<String>,
) -> Result<(mem_api::ResumeCheckpoint, PathBuf)> {
    let (checkpoint, path) = resume::save_checkpoint(project, repo_root, note)?;
    let request = CheckpointActivityRequest {
        project: project.to_string(),
        checkpoint: checkpoint.clone(),
    };
    if let Err(error) = api.log_checkpoint_activity(&request).await {
        eprintln!("warning: failed to log checkpoint activity for `{project}`: {error}");
    }
    Ok((checkpoint, path))
}

fn preview_checkpoint(
    project: &str,
    repo_root: &Path,
    note: Option<String>,
) -> Result<(mem_api::ResumeCheckpoint, PathBuf)> {
    Ok((
        resume::build_checkpoint(project, repo_root, note),
        resume::checkpoint_store_location()?,
    ))
}

async fn preview_automation_flush(
    config: &AppConfig,
    client: &Client,
    project: &str,
    repo_root: &Path,
    force_curate: bool,
    writer_id: &str,
    writer_name: Option<&str>,
) -> Result<serde_json::Value> {
    let mut state = load_state(project, repo_root, &config.automation).await?;
    let changed = watch_detect_changed_files(repo_root, &config.automation.ignored_paths)?;
    update_session_from_repo(&mut state, changed, &config.automation);
    let (capture, capture_reason) = should_capture(&state, &config.automation, true);
    let overview = fetch_automation_overview(client, config, project).await?;
    let (curate, curate_reason) = should_curate(
        &config.automation,
        overview.uncurated_raw_captures,
        true,
        force_curate,
    );
    let capture_request =
        capture.then(|| build_automation_capture_request(&state, writer_id, writer_name));
    Ok(serde_json::json!({
        "project": project,
        "dry_run": true,
        "capture": {
            "would_run": capture,
            "reason": capture_reason,
            "request": capture_request,
        },
        "curate": {
            "would_run": curate,
            "reason": curate_reason,
            "force": force_curate,
            "uncurated_raw_captures": overview.uncurated_raw_captures,
        }
    }))
}

fn build_plan_activity_request(
    project: &str,
    action: PlanActivityAction,
    title: &str,
    thread_key: &str,
    total_items: usize,
    completed_items: usize,
    remaining_items: Vec<String>,
    source_path: Option<String>,
) -> PlanActivityRequest {
    PlanActivityRequest {
        project: project.to_string(),
        action,
        title: title.to_string(),
        thread_key: thread_key.to_string(),
        total_items,
        completed_items,
        remaining_items,
        source_path,
    }
}

#[derive(Debug, Clone)]
struct ActivePlanSelection {
    memory_id: Uuid,
    title: String,
    thread_key: String,
}

#[derive(Debug, Clone, Serialize)]
struct PlanChecklistItem {
    text: String,
    checked: bool,
}

#[derive(Debug, Clone, Serialize)]
struct PlanExecutionFinishReport {
    project: String,
    thread_key: String,
    plan_title: String,
    total_items: usize,
    completed_items: usize,
    remaining_items: Vec<String>,
    verified_complete: bool,
}

async fn resolve_active_plan_selection(
    api: &ApiClient,
    project: &str,
    thread_key: Option<&str>,
) -> Result<ActivePlanSelection> {
    let memories = api.project_memories(project).await?;
    let mut plans = memories
        .items
        .into_iter()
        .filter(|item| item.status == mem_api::MemoryStatus::Active)
        .filter(|item| item.memory_type == mem_api::MemoryType::Plan)
        .filter_map(|item| {
            extract_plan_thread_key(&item.tags).map(|key| ActivePlanSelection {
                memory_id: item.id,
                title: item.summary,
                thread_key: key.to_string(),
            })
        })
        .collect::<Vec<_>>();

    if let Some(thread_key) = thread_key {
        plans.retain(|plan| plan.thread_key == thread_key);
        return match plans.as_slice() {
            [] => anyhow::bail!("no active plan found for thread `{thread_key}`"),
            [plan] => Ok(plan.clone()),
            _ => anyhow::bail!(
                "multiple active plans found for thread `{thread_key}`; review plan memories first"
            ),
        };
    }

    match plans.as_slice() {
        [] => anyhow::bail!("no active plan memory found for `{project}`"),
        [plan] => Ok(plan.clone()),
        _ => {
            let available = plans
                .iter()
                .map(|plan| format!("{} ({})", plan.title, plan.thread_key))
                .collect::<Vec<_>>()
                .join(", ");
            anyhow::bail!(
                "multiple active plan memories found; rerun with --thread-key. Available threads: {available}"
            );
        }
    }
}

fn extract_plan_thread_key(tags: &[String]) -> Option<&str> {
    tags.iter()
        .find_map(|tag| tag.strip_prefix("plan-thread:"))
        .filter(|value| !value.trim().is_empty())
}

fn load_plan_content(
    plan_file: Option<&Path>,
    plan_stdin: bool,
) -> Result<(String, Option<PathBuf>)> {
    match (plan_file, plan_stdin) {
        (Some(_), true) => anyhow::bail!("use either --plan-file or --plan-stdin, not both"),
        (None, false) => anyhow::bail!("provide --plan-file <path> or --plan-stdin"),
        (Some(path), false) => Ok((
            fs::read_to_string(path)
                .with_context(|| format!("read plan file {}", path.display()))?,
            Some(path.to_path_buf()),
        )),
        (None, true) => {
            let mut buffer = String::new();
            std::io::stdin()
                .read_to_string(&mut buffer)
                .context("read plan content from stdin")?;
            Ok((buffer, None))
        }
    }
}

fn derive_plan_title(explicit_title: Option<&str>, plan_markdown: &str, project: &str) -> String {
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

fn derive_plan_thread_key(explicit_key: Option<&str>, title: &str, project: &str) -> String {
    let candidate = explicit_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(title);
    let sanitized = platform::sanitize_service_fragment(candidate)
        .trim_matches('-')
        .to_ascii_lowercase();
    if sanitized.is_empty() {
        format!(
            "approved-plan-{}",
            platform::sanitize_service_fragment(project)
                .trim_matches('-')
                .to_ascii_lowercase()
        )
    } else {
        sanitized
    }
}

fn parse_plan_checkboxes(markdown: &str) -> Vec<PlanChecklistItem> {
    markdown
        .lines()
        .filter_map(|line| parse_plan_checkbox_line(line))
        .collect()
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

fn ensure_checkbox_plan(items: &[PlanChecklistItem]) -> Result<()> {
    if items.is_empty() {
        anyhow::bail!(
            "approved plans must contain Markdown checkbox items like `- [ ] task` before execution starts"
        );
    }
    Ok(())
}

fn normalize_plan_markdown_for_hash(input: &str) -> String {
    input
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_string()
}

fn build_plan_execution_idempotency_key(
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

fn build_plan_execution_request(
    project: &str,
    writer: &WriterIdentity,
    title: &str,
    thread_key: &str,
    plan_markdown: &str,
    source_path: Option<&Path>,
    git_head: Option<&str>,
) -> CaptureTaskRequest {
    let normalized_plan = normalize_plan_markdown_for_hash(plan_markdown);
    let mut sources = vec![
        mem_api::CaptureCandidateSourceInput {
            file_path: None,
            source_kind: mem_api::SourceKind::TaskPrompt,
            excerpt: Some("Approved execution plan entered implementation.".to_string()),
        },
        mem_api::CaptureCandidateSourceInput {
            file_path: None,
            source_kind: mem_api::SourceKind::Note,
            excerpt: Some(normalized_plan.clone()),
        },
    ];
    if let Some(source_path) = source_path {
        sources.insert(
            0,
            mem_api::CaptureCandidateSourceInput {
                file_path: Some(source_path.display().to_string()),
                source_kind: mem_api::SourceKind::File,
                excerpt: Some(format!(
                    "Approved plan source file: {}",
                    source_path.display()
                )),
            },
        );
    }

    CaptureTaskRequest {
        project: project.to_string(),
        task_title: format!("Approved plan: {title}"),
        user_prompt: format!("Approved execution plan for project {project}."),
        writer_id: writer.id.clone(),
        writer_name: writer.name.clone(),
        agent_summary: title.to_string(),
        files_changed: Vec::new(),
        git_diff_summary: git_head.map(|head| format!("Execution started from git HEAD {head}")),
        tests: Vec::new(),
        notes: Vec::new(),
        structured_candidates: vec![mem_api::CaptureCandidateInput {
            canonical_text: normalized_plan.clone(),
            summary: title.to_string(),
            memory_type: mem_api::MemoryType::Plan,
            confidence: 0.95,
            importance: 4,
            tags: vec![
                "plan".to_string(),
                format!("plan-thread:{thread_key}"),
                "execution-started".to_string(),
            ],
            sources,
        }],
        command_output: None,
        idempotency_key: Some(build_plan_execution_idempotency_key(
            project,
            thread_key,
            &normalized_plan,
            git_head,
        )),
        dry_run: false,
    }
}

fn build_plan_execution_finish_report(
    project: &str,
    detail: &mem_api::MemoryEntryResponse,
) -> Result<PlanExecutionFinishReport> {
    let items = parse_plan_checkboxes(&detail.canonical_text);
    ensure_checkbox_plan(&items)?;
    let completed_items = items.iter().filter(|item| item.checked).count();
    let remaining_items = items
        .iter()
        .filter(|item| !item.checked)
        .map(|item| item.text.clone())
        .collect::<Vec<_>>();
    let thread_key = extract_plan_thread_key(&detail.tags)
        .ok_or_else(|| anyhow::anyhow!("active plan is missing a `plan-thread:` tag"))?
        .to_string();

    Ok(PlanExecutionFinishReport {
        project: project.to_string(),
        thread_key,
        plan_title: detail.summary.clone(),
        total_items: items.len(),
        completed_items,
        verified_complete: remaining_items.is_empty(),
        remaining_items,
    })
}

fn plan_detail_from_markdown(
    selection: &ActivePlanSelection,
    markdown: &str,
    memory_id: Uuid,
) -> Result<mem_api::MemoryEntryResponse> {
    let items = parse_plan_checkboxes(markdown);
    ensure_checkbox_plan(&items)?;
    Ok(mem_api::MemoryEntryResponse {
        id: memory_id,
        canonical_text: normalize_plan_markdown_for_hash(markdown),
        summary: selection.title.clone(),
        memory_type: mem_api::MemoryType::Plan,
        importance: 4,
        confidence: 0.95,
        status: mem_api::MemoryStatus::Active,
        tags: vec![
            "plan".to_string(),
            format!("plan-thread:{}", selection.thread_key),
        ],
        sources: Vec::new(),
        related_memories: Vec::new(),
        project: String::new(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    })
}

#[derive(Debug, Clone)]
struct WriterIdentity {
    id: String,
    name: Option<String>,
}

fn resolve_writer_identity(
    config: &AppConfig,
    cli_writer_id: Option<&str>,
) -> Result<WriterIdentity> {
    resolve_writer_identity_for_tool(config, cli_writer_id, "memory")
}

fn resolve_writer_identity_for_tool(
    config: &AppConfig,
    cli_writer_id: Option<&str>,
    tool_name: &str,
) -> Result<WriterIdentity> {
    if let Some(writer_id) = cli_writer_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(WriterIdentity {
            id: writer_id.to_string(),
            name: config.writer.name.clone(),
        });
    }
    if let Ok(writer_id) = env::var("MEMORY_LAYER_WRITER_ID") {
        let trimmed = writer_id.trim();
        if !trimmed.is_empty() {
            return Ok(WriterIdentity {
                id: trimmed.to_string(),
                name: config.writer.name.clone(),
            });
        }
    }
    if let Ok(writer_id) = env::var("MEMORY_LAYER_AGENT_ID") {
        let trimmed = writer_id.trim();
        if !trimmed.is_empty() {
            return Ok(WriterIdentity {
                id: trimmed.to_string(),
                name: config.writer.name.clone(),
            });
        }
    }
    let trimmed = config.writer.id.trim();
    if !trimmed.is_empty() {
        return Ok(WriterIdentity {
            id: trimmed.to_string(),
            name: config.writer.name.clone(),
        });
    }
    Ok(WriterIdentity {
        id: platform::derive_default_writer_id(tool_name),
        name: config.writer.name.clone(),
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

fn repo_git_head(repo_root: &Path) -> Option<String> {
    let output = ProcessCommand::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;
    let head = stdout.trim();
    if head.is_empty() {
        None
    } else {
        Some(head.to_string())
    }
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
    use std::{fs, path::PathBuf, sync::Mutex, time::Duration};

    use clap::{Command, CommandFactory, Parser, error::ErrorKind};
    use uuid::Uuid;

    use super::{
        Cli, DEV_API_TOKEN, RememberArgs, SERVICE_API_TOKEN_KEY, ServiceApiTokenAction,
        build_plan_execution_finish_report, build_plan_execution_request, build_remember_request,
        derive_plan_thread_key, derive_plan_title, ensure_checkbox_plan,
        ensure_shared_service_api_token, initialize_repo, is_placeholder_database_url,
        mask_database_url, parse_plan_checkboxes, render_agent_project_config,
        repair_repo_bootstrap, resolve_project_slug, resolve_repo_root, resolve_writer_identity,
        root_gitignore_contains_mem, shared_env_lookup, write_headers,
    };
    use mem_api::AppConfig;

    #[cfg(target_os = "macos")]
    use super::{
        backend_launch_agent_label, backend_service_available, default_global_config_path,
        render_backend_launch_agent, render_watch_launch_agent, sanitize_service_fragment,
        watch_launch_agent_label,
    };

    #[cfg(not(target_os = "macos"))]
    use super::{render_watch_unit, watch_unit_name};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn restore_env_var(key: &str, value: Option<String>) {
        unsafe {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }

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
    fn project_defaults_to_repo_metadata_when_present() {
        let repo_root = unique_temp_dir("mem-cli-project-slug");
        fs::create_dir_all(repo_root.join(".mem")).unwrap();
        fs::write(
            repo_root.join(".mem").join("project.toml"),
            "slug = \"sctp\"\nrepo_root = \"/tmp/sctp-conformance\"\n",
        )
        .unwrap();

        assert_eq!(resolve_project_slug(None, &repo_root).unwrap(), "sctp");

        let _ = fs::remove_dir_all(repo_root);
    }

    #[test]
    fn bundle_import_rejects_preview_flag() {
        let result = Cli::try_parse_from([
            "memory",
            "bundle",
            "import",
            "--project",
            "memory",
            "bundle.zip",
            "--preview",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn init_rejects_print_flag() {
        let result = Cli::try_parse_from(["memory", "init", "--print"]);
        assert!(result.is_err());
    }

    fn rendered_help(args: &[&str]) -> String {
        let err = Cli::try_parse_from(args).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::DisplayHelp);
        err.to_string()
    }

    fn assert_command_metadata(cmd: &Command, path: &str) {
        if cmd.get_name() != "help" {
            let about = cmd
                .get_about()
                .map(|value| value.to_string())
                .or_else(|| cmd.get_long_about().map(|value| value.to_string()));
            assert!(
                about
                    .as_deref()
                    .map(|value| !value.trim().is_empty())
                    .unwrap_or(false),
                "command {path} is missing help metadata"
            );
        }
        for subcommand in cmd.get_subcommands() {
            let sub_path = if path.is_empty() {
                subcommand.get_name().to_string()
            } else {
                format!("{path} {}", subcommand.get_name())
            };
            assert_command_metadata(subcommand, &sub_path);
        }
    }

    #[test]
    fn all_public_commands_have_help_metadata() {
        let command = Cli::command();
        assert_command_metadata(&command, "memory");
    }

    #[test]
    fn root_help_includes_examples_and_docs_hint() {
        let output = rendered_help(&["memory", "--help"]);
        assert!(output.contains("Project memory CLI"));
        assert!(output.contains("Examples:"));
        assert!(output.contains("docs/user/README.md"));
        assert!(output.contains("Ask a project-specific question against curated memory"));
    }

    #[test]
    fn grouped_help_includes_subcommand_descriptions() {
        let output = rendered_help(&["memory", "service", "--help"]);
        assert!(output.contains("Manage the Memory Layer backend service"));
        assert!(output.contains("Run the backend service in the foreground"));
        assert!(output.contains("Examples:"));
        assert!(output.contains("docs/user/cli/service.md"));
    }

    #[test]
    fn leaf_help_includes_flag_help_examples_and_docs_hint() {
        let output = rendered_help(&["memory", "checkpoint", "start-execution", "--help"]);
        assert!(output.contains("Save a checkpoint and record the approved execution plan"));
        assert!(output.contains("Read the approved plan markdown from a file"));
        assert!(output.contains("Examples:"));
        assert!(output.contains("docs/user/cli/checkpoint.md"));
    }

    #[test]
    fn query_help_includes_argument_descriptions() {
        let output = rendered_help(&["memory", "query", "--help"]);
        assert!(output.contains("Natural-language question to answer from project memory"));
        assert!(output.contains("Restrict results to one or more memory types"));
        assert!(output.contains("docs/user/cli/query.md"));
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
                dry_run: false,
            },
            "memory",
            "codex-writer",
            Some("Codex"),
        )
        .unwrap();

        assert_eq!(request.task_title, "Memory update for memory");
        assert_eq!(request.writer_id, "codex-writer");
        assert!(request.user_prompt.contains("Auto-captured"));
        assert!(request.agent_summary.contains("src/main.rs"));
    }

    #[test]
    fn derive_plan_title_prefers_markdown_heading() {
        let title = derive_plan_title(None, "# Resume Redesign\n\n- step", "memory");
        assert_eq!(title, "Resume Redesign");
    }

    #[test]
    fn derive_plan_thread_key_sanitizes_title() {
        let thread_key = derive_plan_thread_key(None, "Resume Redesign!", "memory");
        assert_eq!(thread_key, "resume-redesign");
    }

    #[test]
    fn plan_execution_request_uses_plan_type_and_thread_tag() {
        let writer = super::WriterIdentity {
            id: "writer".to_string(),
            name: Some("Writer".to_string()),
        };
        let request = build_plan_execution_request(
            "memory",
            &writer,
            "Resume Redesign",
            "resume-redesign",
            "# Resume Redesign\n\n- step",
            None,
            Some("abc123"),
        );

        assert_eq!(request.task_title, "Approved plan: Resume Redesign");
        assert_eq!(request.structured_candidates.len(), 1);
        let candidate = &request.structured_candidates[0];
        assert_eq!(candidate.memory_type, mem_api::MemoryType::Plan);
        assert!(candidate.tags.contains(&"plan".to_string()));
        assert!(
            candidate
                .tags
                .contains(&"plan-thread:resume-redesign".to_string())
        );
    }

    #[test]
    fn parse_plan_checkboxes_extracts_checked_and_unchecked_items() {
        let items = parse_plan_checkboxes(
            "# Plan\n\n- [ ] first task\n* [x] second task\n+ [X] third task\nplain bullet",
        );
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].text, "first task");
        assert!(!items[0].checked);
        assert_eq!(items[1].text, "second task");
        assert!(items[1].checked);
        assert_eq!(items[2].text, "third task");
        assert!(items[2].checked);
    }

    #[test]
    fn ensure_checkbox_plan_rejects_plans_without_checkboxes() {
        let items = parse_plan_checkboxes("# Plan\n\n- regular bullet");
        let error = ensure_checkbox_plan(&items).expect_err("should reject non-checkbox plan");
        assert!(
            error
                .to_string()
                .contains("approved plans must contain Markdown checkbox items")
        );
    }

    #[test]
    fn finish_report_lists_remaining_items() {
        let report = build_plan_execution_finish_report(
            "memory",
            &mem_api::MemoryEntryResponse {
                id: Uuid::new_v4(),
                project: "memory".to_string(),
                canonical_text: "# Plan\n\n- [x] done\n- [ ] remaining".to_string(),
                summary: "Execution plan".to_string(),
                memory_type: mem_api::MemoryType::Plan,
                importance: 4,
                confidence: 0.95,
                status: mem_api::MemoryStatus::Active,
                tags: vec![
                    "plan".to_string(),
                    "plan-thread:resume-redesign".to_string(),
                ],
                sources: Vec::new(),
                related_memories: Vec::new(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
        )
        .expect("build finish report");

        assert_eq!(report.total_items, 2);
        assert_eq!(report.completed_items, 1);
        assert!(!report.verified_complete);
        assert_eq!(report.remaining_items, vec!["remaining".to_string()]);
    }

    #[test]
    fn writer_identity_falls_back_to_derived_value() {
        let _guard = ENV_LOCK.lock().unwrap();
        let old_user = std::env::var("MEMORY_LAYER_WRITER_IDENTITY_USER").ok();
        let old_host = std::env::var("MEMORY_LAYER_WRITER_IDENTITY_HOST").ok();
        let old_writer = std::env::var("MEMORY_LAYER_WRITER_ID").ok();
        let old_agent = std::env::var("MEMORY_LAYER_AGENT_ID").ok();

        unsafe {
            std::env::set_var("MEMORY_LAYER_WRITER_IDENTITY_USER", "cli-user");
            std::env::set_var("MEMORY_LAYER_WRITER_IDENTITY_HOST", "cli-host");
            std::env::remove_var("MEMORY_LAYER_WRITER_ID");
            std::env::remove_var("MEMORY_LAYER_AGENT_ID");
        }

        let config: AppConfig =
            toml::from_str("[service]\n\n[database]\nurl = \"postgres://example\"\n")
                .expect("parse minimal config");
        let writer = resolve_writer_identity(&config, None).expect("resolve writer identity");

        restore_env_var("MEMORY_LAYER_WRITER_IDENTITY_USER", old_user);
        restore_env_var("MEMORY_LAYER_WRITER_IDENTITY_HOST", old_host);
        restore_env_var("MEMORY_LAYER_WRITER_ID", old_writer);
        restore_env_var("MEMORY_LAYER_AGENT_ID", old_agent);

        assert_eq!(writer.id, "memory-cli-user-cli-host");
        assert_eq!(writer.name, None);
    }

    #[test]
    fn init_print_describes_repo_layout() {
        let repo_root = PathBuf::from("/tmp/memory");
        let summary = initialize_repo(&repo_root, "memory", false, true).unwrap();

        assert!(summary.contains(".mem/config.toml"));
        assert!(summary.contains(".agents/memory-layer.toml"));
        assert!(summary.contains(".agents/skills"));
        assert!(summary.contains("bundled memory skills"));
        assert!(summary.contains("memory watcher enable --project memory"));
        assert!(summary.contains("memory service run"));
    }

    #[test]
    fn init_creates_repo_files_and_gitignore_entry() {
        let repo_root = unique_temp_dir("mem-init");
        fs::create_dir_all(&repo_root).unwrap();

        initialize_repo(&repo_root, "memory", false, false).unwrap();

        assert!(repo_root.join(".mem/config.toml").is_file());
        assert!(repo_root.join(".mem/project.toml").is_file());
        assert!(repo_root.join(".agents/memory-layer.toml").is_file());
        assert!(repo_root.join(".mem/runtime").is_dir());
        assert!(
            repo_root
                .join(".agents/skills/memory-layer/SKILL.md")
                .is_file()
        );
        assert!(
            repo_root
                .join(".agents/skills/memory-query-resume/SKILL.md")
                .is_file()
        );
        assert!(
            repo_root
                .join(".agents/skills/memory-plan-execution/SKILL.md")
                .is_file()
        );
        assert!(
            repo_root
                .join(".agents/skills/memory-remember/SKILL.md")
                .is_file()
        );
        assert!(
            repo_root
                .join(".agents/skills/memory-layer/scripts/go.mod")
                .is_file()
        );
        assert!(
            repo_root
                .join(".agents/skills/memory-layer/scripts/main.go")
                .is_file()
        );
        assert!(
            fs::read_to_string(repo_root.join(".mem/config.toml"))
                .unwrap()
                .contains("[automation]")
        );
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

    #[test]
    fn placeholder_database_url_is_detected() {
        assert!(is_placeholder_database_url(
            "postgresql://memory:<password>@localhost:5432/memory"
        ));
        assert!(!is_placeholder_database_url(
            "postgresql://memory:secret@localhost:5432/memory"
        ));
    }

    #[test]
    fn database_url_is_masked_for_output() {
        assert_eq!(
            mask_database_url("postgresql://memory:secret@localhost:5432/memory"),
            "postgresql://<redacted>@localhost:5432/memory"
        );
    }

    #[test]
    fn repair_repo_bootstrap_creates_missing_files() {
        let repo_root = unique_temp_dir("mem-doctor-fix");
        fs::create_dir_all(&repo_root).unwrap();

        repair_repo_bootstrap(&repo_root, "memory").unwrap();

        assert!(repo_root.join(".mem/config.toml").is_file());
        assert!(repo_root.join(".mem/project.toml").is_file());
        assert!(repo_root.join(".agents/memory-layer.toml").is_file());
        assert!(repo_root.join(".mem/runtime").is_dir());
        assert!(
            repo_root
                .join(".agents/skills/memory-layer/SKILL.md")
                .is_file()
        );
        assert!(
            repo_root
                .join(".agents/skills/memory-query-resume/SKILL.md")
                .is_file()
        );
        assert!(root_gitignore_contains_mem(&repo_root).unwrap());

        let _ = fs::remove_dir_all(repo_root);
    }

    #[test]
    fn init_preserves_existing_memory_skills_without_force() {
        let repo_root = unique_temp_dir("mem-init-skill-bundle");
        fs::create_dir_all(repo_root.join(".agents/skills/memory-layer")).unwrap();
        fs::write(
            repo_root.join(".agents/skills/memory-layer/SKILL.md"),
            "custom umbrella skill\n",
        )
        .unwrap();

        initialize_repo(&repo_root, "memory", false, false).unwrap();

        assert_eq!(
            fs::read_to_string(repo_root.join(".agents/skills/memory-layer/SKILL.md")).unwrap(),
            "custom umbrella skill\n"
        );
        assert!(
            repo_root
                .join(".agents/skills/memory-query-resume/SKILL.md")
                .is_file()
        );
        assert!(
            repo_root
                .join(".agents/skills/memory-plan-execution/SKILL.md")
                .is_file()
        );
        assert!(
            repo_root
                .join(".agents/skills/memory-remember/SKILL.md")
                .is_file()
        );

        let _ = fs::remove_dir_all(repo_root);
    }

    #[test]
    fn agent_project_config_mentions_project_customization() {
        let repo_root = PathBuf::from("/tmp/memory");
        let content = render_agent_project_config("memory", &repo_root);

        assert!(content.contains("[capture]"));
        assert!(content.contains("include_paths"));
        assert!(content.contains("graph_enabled = false"));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn watch_unit_name_is_project_scoped() {
        assert_eq!(watch_unit_name("homelab"), "memory-watch-homelab.service");
        assert_eq!(
            watch_unit_name("customer portal"),
            "memory-watch-customer-portal.service"
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn watch_unit_uses_repo_root_and_project() {
        let repo_root = unique_temp_dir("mem-watch-unit");
        fs::create_dir_all(&repo_root).unwrap();
        let unit = render_watch_unit(&repo_root, "homelab").unwrap();

        assert!(unit.contains("Description=Memory Layer Watcher (homelab)"));
        assert!(unit.contains(&format!("WorkingDirectory={}", repo_root.display())));
        assert!(unit.contains("EnvironmentFile=-"));
        assert!(unit.contains("run --project homelab"));

        let _ = fs::remove_dir_all(repo_root);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn launch_agent_labels_are_project_scoped() {
        assert_eq!(backend_launch_agent_label(), "com.memory-layer.mem-service");
        assert_eq!(
            watch_launch_agent_label("customer portal"),
            "com.memory-layer.memory-watch.customer-portal"
        );
        assert_eq!(
            sanitize_service_fragment("customer portal"),
            "customer-portal"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn backend_launch_agent_uses_global_config_path() {
        let plist = render_backend_launch_agent(&default_global_config_path()).unwrap();

        assert!(backend_service_available());
        assert!(plist.contains("<string>com.memory-layer.mem-service</string>"));
        assert!(plist.contains("<string>/bin/zsh</string>"));
        assert!(plist.contains(&default_global_config_path().display().to_string()));
        assert!(plist.contains("mem-service.stdout.log"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn watch_launch_agent_uses_repo_root_and_project() {
        let repo_root = unique_temp_dir("mem-watch-launch-agent");
        fs::create_dir_all(&repo_root).unwrap();
        let plist = render_watch_launch_agent(&repo_root, "homelab").unwrap();

        assert!(plist.contains("<string>com.memory-layer.memory-watch.homelab</string>"));
        assert!(plist.contains(&repo_root.display().to_string()));
        assert!(plist.contains("<string>/bin/zsh</string>"));
        assert!(plist.contains("memory-watch"));
        assert!(plist.contains("--project"));
        assert!(plist.contains("homelab"));

        let _ = fs::remove_dir_all(repo_root);
    }

    #[test]
    fn shared_env_lookup_reads_key() {
        let dir = unique_temp_dir("mem-shared-env");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("memory-layer.env");
        fs::write(&path, "OPENAI_API_KEY=test-key\n").unwrap();

        assert_eq!(
            super::shared_env_lookup(&path, "OPENAI_API_KEY").as_deref(),
            Some("test-key")
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn ensure_shared_service_api_token_creates_missing_token() {
        let dir = unique_temp_dir("mem-token-create");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("memory-layer.env");

        let result = ensure_shared_service_api_token(&path, None, true).unwrap();
        let token = shared_env_lookup(&path, SERVICE_API_TOKEN_KEY).unwrap();

        assert!(result.changed);
        assert!(matches!(result.action, ServiceApiTokenAction::Created));
        assert!(token.starts_with("ml_"));
        assert_ne!(token, DEV_API_TOKEN);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn ensure_shared_service_api_token_rotates_placeholder() {
        let dir = unique_temp_dir("mem-token-rotate");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("memory-layer.env");
        fs::write(
            &path,
            format!("{SERVICE_API_TOKEN_KEY}={DEV_API_TOKEN}\nOPENAI_API_KEY=test-key\n"),
        )
        .unwrap();

        let result = ensure_shared_service_api_token(&path, None, true).unwrap();
        let token = shared_env_lookup(&path, SERVICE_API_TOKEN_KEY).unwrap();
        let content = fs::read_to_string(&path).unwrap();

        assert!(result.changed);
        assert!(matches!(result.action, ServiceApiTokenAction::Rotated));
        assert!(token.starts_with("ml_"));
        assert!(content.contains("OPENAI_API_KEY=test-key"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn ensure_shared_service_api_token_preserves_existing_non_placeholder() {
        let dir = unique_temp_dir("mem-token-preserve");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("memory-layer.env");
        fs::write(&path, format!("{SERVICE_API_TOKEN_KEY}=ml_existingtoken\n")).unwrap();

        let result = ensure_shared_service_api_token(&path, None, true).unwrap();
        let token = shared_env_lookup(&path, SERVICE_API_TOKEN_KEY).unwrap();

        assert!(!result.changed);
        assert!(matches!(result.action, ServiceApiTokenAction::Preserved));
        assert_eq!(token, "ml_existingtoken");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn write_headers_adds_local_origin_for_loopback_service() {
        let mut config = test_app_config();
        config.service.bind_addr = "127.0.0.1:4040".to_string();
        config.service.api_token = "ml_testtoken".to_string();

        let headers = write_headers(&config).unwrap();

        assert!(headers.get("x-api-token").is_none());
        assert_eq!(
            headers.get("origin").and_then(|value| value.to_str().ok()),
            Some("http://127.0.0.1")
        );
    }

    #[test]
    fn write_headers_omits_local_origin_for_non_loopback_service() {
        let mut config = test_app_config();
        config.service.bind_addr = "10.22.6.42:4140".to_string();
        config.service.api_token = "ml_testtoken".to_string();

        let headers = write_headers(&config).unwrap();

        assert_eq!(
            headers
                .get("x-api-token")
                .and_then(|value| value.to_str().ok()),
            Some("ml_testtoken")
        );
        assert!(headers.get("origin").is_none());
    }

    fn test_app_config() -> AppConfig {
        AppConfig {
            service: mem_api::ServiceConfig {
                bind_addr: "127.0.0.1:4040".to_string(),
                capnp_unix_socket: "/tmp/memory-layer.capnp.sock".to_string(),
                capnp_tcp_addr: "127.0.0.1:4041".to_string(),
                web_root: None,
                api_token: "ml_testtoken".to_string(),
                request_timeout: Duration::from_secs(30),
            },
            database: mem_api::DatabaseConfig {
                url: "postgresql://memory:test@localhost:5432/memory".to_string(),
            },
            features: mem_api::FeatureFlags::default(),
            llm: mem_api::LlmConfig::default(),
            embeddings: mem_api::EmbeddingConfig::default(),
            cluster: mem_api::ClusterConfig::default(),
            writer: mem_api::WriterConfig::default(),
            automation: mem_api::AutomationConfig::default(),
        }
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
