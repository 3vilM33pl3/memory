mod commits;
mod plan_execution;
mod resume;
mod scan;
mod tui;
mod wizard;
mod writer_identity;

use std::collections::BTreeMap;
#[cfg(unix)]
use std::os::unix::{fs::PermissionsExt, net::UnixStream};
use std::{
    env, fs,
    io::{self, IsTerminal, Read, Write},
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, Stdio},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::{Args, Parser, Subcommand};
use mem_agenttop::LightweightAgentSession;
use mem_api::{
    ActivityListResponse, AppConfig, ArchiveRequest, ArchiveResponse, CaptureTaskRequest,
    CheckpointActivityRequest, CommitDetailResponse, CommitSyncRequest, CommitSyncResponse,
    CurateRequest, CurateResponse, DeleteMemoryRequest, DeleteMemoryResponse, GraphActivityRequest,
    MemoryEntryResponse, MemoryType, PlanActivityAction, PlanActivityRequest, Profile,
    ProjectCommitsResponse, ProjectMemoriesResponse, ProjectMemoryBundlePreview,
    ProjectMemoryExportOptions, ProjectMemoryImportPreview, ProjectMemoryImportResponse,
    ProjectOverviewResponse, PruneEmbeddingsRequest, PruneEmbeddingsResponse, QueryFilters,
    QueryRequest, QueryResponse, ReembedRequest, ReembedResponse, ReindexRequest, ReindexResponse,
    ReplacementPolicy, ResumeRequest, ResumeResponse, ScanActivityRequest, TestResult, TokenUsage,
    UpToSpeedRequest, UpToSpeedResponse, discover_global_config_path, discover_repo_env_path,
    effective_llm_base_url, is_ollama_provider, is_supported_llm_provider,
    llm_max_output_tokens_field, llm_requires_api_key, load_repo_replacement_policy,
    read_repo_project_slug, resolve_llm_api_key,
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
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Row, postgres::PgPoolOptions};
use uuid::Uuid;

use crate::plan_execution::{
    derive_plan_thread_key, derive_plan_title, durable_plan_source_path, ensure_checkbox_plan,
    normalize_plan_markdown_for_hash, parse_plan_checkboxes,
};
use crate::writer_identity::{
    WriterIdentity, resolve_writer_identity, resolve_writer_identity_for_tool,
};

const ROOT_AFTER_HELP: &str = "\
Agent contract:
  Use this CLI from Codex, Claude, or scripts to read and write durable project memory.
  Prefer --json for commands that support it when another tool will parse the output.
  Prefer --dry-run before mutating repository, service, memory, history, or embedding state.
  Before answering repo-specific questions, run query; after interruptions, run resume.
  When an approved plan moves into implementation, run checkpoint start-execution.
  When direct no-plan work starts, run checkpoint start-task.
  Before claiming plan-backed work is complete, run checkpoint finish-execution.
  After meaningful completed work, run remember with concrete notes and changed files.

Examples:
  memory wizard --global
  memory query --project memory --question \"What changed recently?\" --json
  memory resume --project memory --json
  memory checkpoint start-task --project memory --title \"Task title\" --prompt \"Original user request\"
  memory remember --project memory --title \"Task title\" --summary \"What changed\" --note \"Durable fact\"

See also:
  docs/user/README.md";

const WIZARD_AFTER_HELP: &str = "\
Agent notes:
  Use for first-time interactive setup. Prefer init or service commands for noninteractive runs.
  Mutates config/service files unless --dry-run is passed.

Examples:
  memory wizard --global
  memory wizard
  memory wizard --project memory --dry-run

See also:
  docs/user/cli/wizard.md";

const INIT_AFTER_HELP: &str = "\
Agent notes:
  Use in a repository before dev init, TUI, web UI, watcher, scan, query, or remember workflows.
  Mutates repo-local .mem bootstrap files and agent skill files unless --dry-run is passed.

Examples:
  memory init
  memory init --project memory --dry-run
  memory init --force

See also:
  docs/user/cli/init.md";

const DEV_GROUP_AFTER_HELP: &str = "\
Agent notes:
  Use from a cargo checkout to manage the isolated dev profile.
  Dev binaries use 127.0.0.1:4250 HTTP and 127.0.0.1:4251 Cap'n Proto by default.

Examples:
  memory dev init --copy-from-global
  memory dev init --dry-run

See also:
  docs/developer/dev-stack.md";

const DEV_INIT_AFTER_HELP: &str = "\
Agent notes:
  Run after memory init when developing Memory Layer from source.
  Use --copy-from-global to copy database, LLM, embedding, feature, and writer settings into the dev overlay.
  Mutates .mem/config.dev.toml and .mem/runtime/dev unless --dry-run is passed.

Examples:
  memory init
  memory dev init --copy-from-global
  memory dev init --dry-run

See also:
  docs/developer/dev-stack.md";

const SERVICE_GROUP_AFTER_HELP: &str = "\
Agent notes:
  Use service run for foreground backend work and service status for packaged install checks.
  The backend serves the HTTP API and browser web UI from the same bind address.
  Dev cargo runs use 127.0.0.1:4250; packaged installs normally use 127.0.0.1:4040.

Examples:
  memory service run
  memory service enable --dry-run
  memory service status

See also:
  docs/user/cli/service.md";

const SERVICE_RUN_AFTER_HELP: &str = "\
Agent notes:
  Starts the backend in the foreground. In dev mode it also serves the web UI at http://127.0.0.1:4250/.
  This process keeps running until interrupted; use it when a TUI, web UI, or CLI call needs a backend.

Examples:
  memory service run

See also:
  docs/user/cli/service.md";

const SERVICE_ENABLE_AFTER_HELP: &str = "\
Agent notes:
  Enables and starts the packaged backend service. Use --dry-run first from an agent.
  Prefer service run for foreground dev work from a cargo checkout.

Examples:
  memory service enable
  memory service enable --dry-run

See also:
  docs/user/cli/service.md";

const SERVICE_DISABLE_AFTER_HELP: &str = "\
Agent notes:
  Stops and disables the packaged backend service. This can interrupt TUI, web UI, watcher, and CLI clients.
  Use --dry-run first from an agent.

Examples:
  memory service disable
  memory service disable --dry-run

See also:
  docs/user/cli/service.md";

const SERVICE_STATUS_AFTER_HELP: &str = "\
Agent notes:
  Read-only packaged service inspection. Use doctor for broader config and connectivity checks.

Examples:
  memory service status

See also:
  docs/user/cli/service.md";

const SERVICE_RESTART_ALL_AFTER_HELP: &str = "\
Agent notes:
  Restarts Memory Layer services that are already active or loaded after a package install or upgrade.
  Does not start intentionally stopped services. Use --dry-run --json before changing service state from automation.
  Use --mark-tui-restart from installers so running TUIs can show a red restart status.

Examples:
  memory service restart-all --dry-run --json
  memory service restart-all --mark-tui-restart --json

See also:
  docs/user/cli/service.md";

const SERVICE_TOKEN_AFTER_HELP: &str = "\
Agent notes:
  Provisions or rotates the shared API token used by local clients. Use --dry-run and --json when scripting.
  Mutates env files unless --dry-run is passed.

Examples:
  memory service ensure-api-token --shared
  memory service ensure-api-token --rotate-placeholder --dry-run --json

See also:
  docs/user/cli/service.md";

const EVAL_GROUP_AFTER_HELP: &str = "\
Agent notes:
  Use eval commands to produce reproducible evidence that memory changes retrieval, grounding, cost, or task success.
  Prefer --dry-run before runs that would call the backend or execute task commands.
  JSON is the default output for automation; use --text for human summaries.

Examples:
  memory eval scaffold --project memory --out evals/suites/memory-smoke
  memory eval doctor --suite evals/examples/memory-smoke
  memory eval run --suite evals/examples/app-build-smoke --condition no-memory --condition full-memory --profile offline --text
  memory eval run --suite evals/examples/memory-smoke --condition full-memory --repeat 5
  memory eval compare --baseline target/memory-evals/run-a.json --candidate target/memory-evals/run-b.json --text
  memory eval compare --baseline 'target/memory-evals/*no-memory*.json' --candidate 'target/memory-evals/*full-memory*.json' --out target/memory-evals/comparison.json
  memory eval gate --comparison target/memory-evals/comparison.json --policy evals/gates/research-v1.toml
  memory eval report --comparison target/memory-evals/comparison.json --text

See also:
  docs/user/cli/eval.md";

const EVAL_SCAFFOLD_AFTER_HELP: &str = "\
Agent notes:
  Creates starter retrieval QA fixtures from current memories. Review generated labels before using the suite for claims.
  Mutates the output directory unless --dry-run is passed.

Examples:
  memory eval scaffold --project memory --out evals/suites/memory-smoke
  memory eval scaffold --project memory --out /tmp/eval --limit 5 --dry-run

See also:
  docs/user/cli/eval.md";

const EVAL_RUN_AFTER_HELP: &str = "\
Agent notes:
  Runs a suite under one or more conditions and writes immutable JSON artifacts under target/memory-evals by default.
  Default profile is llm. Use --profile offline for deterministic CI-safe checks.
  Use --dry-run to validate suite parsing without LLM calls or shell command execution.
  agent_build_task items copy fixtures to target/memory-evals/build-runs and capture prompts, stdout, stderr, and scoring summaries.

Examples:
  memory eval run --suite evals/examples/memory-smoke --condition full-memory --dry-run
  memory eval run --suite evals/examples/app-build-smoke --condition no-memory --condition full-memory --profile offline --text
  memory eval run --suite evals/examples/memory-smoke --condition no-memory --condition full-memory --repeat 5
  memory eval run --suite evals/suites/memory-improvement-v1 --condition no-memory --condition full-memory --llm-judge --repeat 5

See also:
  docs/user/cli/eval.md";

const EVAL_COMPARE_AFTER_HELP: &str = "\
Agent notes:
  Compares run JSON files pairwise by item id, repeat, and sequence step, then reports success deltas plus statistical summaries.
  Use --out to preserve comparison JSON for reports or releases.

Examples:
  memory eval compare --baseline target/memory-evals/no-memory.json --candidate target/memory-evals/full-memory.json --text
  memory eval compare --baseline 'target/memory-evals/*no-memory*.json' --candidate 'target/memory-evals/*full-memory*.json' --out target/memory-evals/comparison.json

See also:
  docs/user/cli/eval.md";

const EVAL_REPORT_AFTER_HELP: &str = "\
Agent notes:
  Renders an existing comparison artifact. This is read-only and suitable for release notes.

Examples:
  memory eval report --comparison target/memory-evals/comparison.json --text
  memory eval report --comparison target/memory-evals/comparison.json --markdown --out target/memory-evals/report.md
  memory eval report --comparison target/memory-evals/comparison.json

See also:
  docs/user/cli/eval.md";

const EVAL_DOCTOR_AFTER_HELP: &str = "\
Agent notes:
  Read-only eval prerequisite check. Use before expensive LLM-backed research runs.

Examples:
  memory eval doctor --suite evals/examples/memory-smoke
  memory eval doctor --suite evals/suites/research-v1 --text

See also:
  docs/user/cli/eval.md";

const EVAL_GATE_AFTER_HELP: &str = "\
Agent notes:
  Read-only comparison policy check. Exits with a failure when the gate does not pass.

Examples:
  memory eval gate --comparison target/memory-evals/comparison.json --policy evals/gates/research-v1.toml
  memory eval gate --comparison target/memory-evals/comparison.json --policy evals/gates/research-v1.toml --text

See also:
  docs/user/cli/eval.md";

const DOCTOR_AFTER_HELP: &str = "\
Agent notes:
  Use as the first diagnostic command when setup, service connectivity, watcher, skill, LLM, or embedding behavior is unclear.
  Read-only unless --fix is passed. Prefer --json for automated diagnosis.

Examples:
  memory doctor
  memory doctor --project memory
  memory doctor --json

See also:
  docs/user/cli/doctor.md";

const WATCHER_GROUP_AFTER_HELP: &str = "\
Agent notes:
  The watcher manager is the preferred local automation path for Codex-linked sessions.
  Legacy per-project watcher services and manual watcher run remain compatibility paths.
  Enable/disable commands mutate service state; use --dry-run where supported.

Examples:
  memory watcher manager enable
  memory watcher run --project memory
  memory watcher enable --project memory
  memory watcher status --project memory

See also:
  docs/user/cli/watchers.md";

const WATCHER_RUN_AFTER_HELP: &str = "\
Agent notes:
  Runs one watcher in the foreground for a project or repo root.
  Agent ownership flags are for watcher-manager handoff; avoid inventing them manually unless integrating another agent runtime.

Examples:
  memory watcher run --project memory
  memory watcher run --repo-root /path/to/repo

See also:
  docs/user/cli/watchers.md";

const WATCHER_ENABLE_AFTER_HELP: &str = "\
Agent notes:
  Enables a legacy per-project watcher service. Prefer watcher manager enable for Codex-linked local automation.
  Mutates service state unless --dry-run is passed.

Examples:
  memory watcher enable --project memory
  memory watcher enable --project memory --dry-run

See also:
  docs/user/cli/watchers.md";

const WATCHER_DISABLE_AFTER_HELP: &str = "\
Agent notes:
  Disables a legacy per-project watcher service. This can stop background capture for the project.
  Mutates service state unless --dry-run is passed.

Examples:
  memory watcher disable --project memory
  memory watcher disable --project memory --dry-run

See also:
  docs/user/cli/watchers.md";

const WATCHER_STATUS_AFTER_HELP: &str = "\
Agent notes:
  Read-only watcher health check. Use manager status for Codex-linked watcher-manager state.

Examples:
  memory watcher status --project memory

See also:
  docs/user/cli/watchers.md";

const WATCHER_MANAGER_GROUP_AFTER_HELP: &str = "\
Agent notes:
  Preferred automation surface for Codex-linked sessions. The manager detects live sessions and starts one watcher per repo/session.
  Enable/disable commands mutate the user service. Run/status are foreground/read-only service checks.

Examples:
  memory watcher manager status
  memory watcher manager enable --dry-run
  memory watcher manager run

See also:
  docs/user/cli/watchers.md";

const WATCHER_MANAGER_RUN_AFTER_HELP: &str = "\
Agent notes:
  Runs the Codex-linked watcher manager in the foreground for development or debugging.
  The process keeps running until interrupted and may start watcher processes for detected sessions.

Examples:
  memory watcher manager run

See also:
  docs/user/cli/watchers.md";

const WATCHER_MANAGER_ENABLE_AFTER_HELP: &str = "\
Agent notes:
  Enables the persistent user watcher-manager service. Use --dry-run first from an agent.
  Prefer this over legacy per-project watcher services for normal Codex automation.

Examples:
  memory watcher manager enable
  memory watcher manager enable --dry-run

See also:
  docs/user/cli/watchers.md";

const WATCHER_MANAGER_DISABLE_AFTER_HELP: &str = "\
Agent notes:
  Disables the persistent user watcher-manager service. This can stop agent-linked background capture.
  Use --dry-run first from an agent.

Examples:
  memory watcher manager disable
  memory watcher manager disable --dry-run

See also:
  docs/user/cli/watchers.md";

const WATCHER_MANAGER_STATUS_AFTER_HELP: &str = "\
Agent notes:
  Read-only status for the Codex-linked watcher manager and its user service.

Examples:
  memory watcher manager status

See also:
  docs/user/cli/watchers.md";

const QUERY_AFTER_HELP: &str = "\
Agent notes:
  Use before answering project-specific questions about architecture, workflows, history, or prior decisions.
  Prefer --json for Codex/Claude tools so citations, confidence, and insufficient_evidence are machine-readable.
  Do not treat low-confidence or insufficient-evidence answers as facts.

Examples:
  memory query --project memory --question \"How does resume work?\" --json
  memory query --project memory --question \"What changed?\" --type plan --tag plan --json

See also:
  docs/user/cli/query.md";

const HISTORY_AFTER_HELP: &str = "\
Agent notes:
  Read-only inspection of all versions for one memory chain, including tombstones.
  Use --json when comparing canonical history programmatically.

Examples:
  memory history 00000000-0000-0000-0000-000000000000 --json

See also:
  docs/developer/architecture/embeddings-and-search.md";

const PRUNE_HISTORY_AFTER_HELP: &str = "\
Agent notes:
  Permanently prunes tombstoned and superseded memory versions by retention threshold.
  Always use --dry-run and --json first from an agent. Limit with --project unless a global sweep is intended.

Examples:
  memory prune-history --project memory --dry-run --json
  memory prune-history --project memory --tombstone-after 90d --superseded-after 180d --dry-run

See also:
  docs/developer/architecture/memory-types.md";

const COMMITS_GROUP_AFTER_HELP: &str = "\
Agent notes:
  Use to import or inspect git history as evidence without turning every commit into canonical memory.
  Sync mutates backend commit history unless --dry-run is passed.

Examples:
  memory commits sync --project memory
  memory commits list --project memory
  memory commits show <commit> --project memory

See also:
  docs/user/cli/commits.md";

const COMMITS_SYNC_AFTER_HELP: &str = "\
Agent notes:
  Imports local git commits into backend evidence. Use --dry-run and --json before a large sync.

Examples:
  memory commits sync --project memory --dry-run --json
  memory commits sync --project memory --since 2026-04-01 --dry-run --json

See also:
  docs/user/cli/commits.md";

const COMMITS_LIST_AFTER_HELP: &str = "\
Agent notes:
  Read-only commit evidence listing. Prefer --json for scripts.

Examples:
  memory commits list --project memory
  memory commits list --project memory --limit 50 --json

See also:
  docs/user/cli/commits.md";

const COMMITS_SHOW_AFTER_HELP: &str = "\
Agent notes:
  Read-only detail for one imported commit. Use --json when extracting evidence.

Examples:
  memory commits show abc123 --project memory --json

See also:
  docs/user/cli/commits.md";

const REPO_GROUP_AFTER_HELP: &str = "\
Agent notes:
  The repository index feeds scan and analysis flows. Status is read-only; index mutates local index state unless --dry-run is passed.

Examples:
  memory repo index --project memory
  memory repo status --project memory

See also:
  docs/user/cli/repo.md";

const REPO_INDEX_AFTER_HELP: &str = "\
Agent notes:
  Builds or refreshes the local repository index. Use --dry-run and --json before broad indexing from an agent.

Examples:
  memory repo index --project memory --dry-run --json
  memory repo index --project memory --since 2026-04-01 --dry-run --json

See also:
  docs/user/cli/repo.md";

const REPO_STATUS_AFTER_HELP: &str = "\
Agent notes:
  Read-only repository index status and analyzer coverage. Prefer --json for scripts.

Examples:
  memory repo status --project memory
  memory repo status --project memory --json

See also:
  docs/user/cli/repo.md";

const GRAPH_GROUP_AFTER_HELP: &str = "\
Agent notes:
  Extract parser-backed code graph facts so query and the TUI can improve retrieval ranking and explanations.
  Extract mutates graph tables and may rebuild the local repo index unless --dry-run is passed. Status is read-only.
  JSON is the default output; use --text only for human-readable summaries.

Examples:
  memory graph extract --project memory --dry-run
  memory graph extract --project memory --force --text
  memory graph status --project memory

See also:
  docs/user/cli/graph.md";

const GRAPH_EXTRACT_AFTER_HELP: &str = "\
Agent notes:
  Builds code symbols, references, graph nodes, graph edges, and evidence from the local repository index.
  Use --dry-run before writing. Use --rebuild-index when the repo index may be stale.
  JSON is the default output; use --text only for human-readable summaries.

Examples:
  memory graph extract --project memory
  memory graph extract --project memory --rebuild-index --dry-run
  memory graph extract --project memory --force --text

See also:
  docs/user/cli/graph.md";

const GRAPH_STATUS_AFTER_HELP: &str = "\
Agent notes:
  Read-only status for the latest completed graph extraction, including analyzer versions, graph counts, and unresolved references.
  JSON is the default output; use --text only for human-readable summaries.

Examples:
  memory graph status --project memory
  memory graph status --project memory --text

See also:
  docs/user/cli/graph.md";

const BUNDLE_GROUP_AFTER_HELP: &str = "\
Agent notes:
  Export/import portable project memory bundles. Import mutates memory state unless --dry-run is passed.

Examples:
  memory bundle export --project memory --out /tmp/memory.mlbundle.zip
  memory bundle import --project memory /tmp/memory.mlbundle.zip --dry-run

See also:
  docs/user/cli/bundles.md";

const BUNDLE_EXPORT_AFTER_HELP: &str = "\
Agent notes:
  Writes a portable bundle file unless --dry-run is passed. Include provenance flags intentionally.

Examples:
  memory bundle export --project memory --out /tmp/memory.mlbundle.zip
  memory bundle export --project memory --out /tmp/memory.mlbundle.zip --dry-run

See also:
  docs/user/cli/bundles.md";

const BUNDLE_IMPORT_AFTER_HELP: &str = "\
Agent notes:
  Imports memories from a bundle. Always run --dry-run --json first from an agent.

Examples:
  memory bundle import --project memory /tmp/memory.mlbundle.zip
  memory bundle import --project memory /tmp/memory.mlbundle.zip --dry-run --json

See also:
  docs/user/cli/bundles.md";

const RESUME_AFTER_HELP: &str = "\
Agent notes:
  Use after interruptions or context compaction to rebuild project state before continuing.
  Prefer --json for tools; include_llm_summary defaults on and falls back to deterministic output when unavailable.

Examples:
  memory resume --project memory
  memory resume --project memory --json

See also:
  docs/user/cli/resume.md";

const ACTIVITIES_AFTER_HELP: &str = "\
Agent notes:
  Read-only timeline of persisted project activity events, including query, resume, graph, watcher, and checkpoint activity.
  JSON is the default output; use --text only for human-readable summaries.
  Use kind filters when an agent needs specific operational evidence.

Examples:
  memory activities --project memory
  memory activities --project memory --limit 50 --text
  memory activities --project memory --kind query

See also:
  docs/user/cli/activities.md";

const UP_TO_SPEED_AFTER_HELP: &str = "\
Agent notes:
  Use when a new agent joins an active project and needs a concise operational briefing.
  JSON is the default output; use --text only for human-readable summaries.
  Use --llm when configured synthesis is desired; deterministic evidence remains available without it.

Examples:
  memory up-to-speed --project memory
  memory up-to-speed --project memory --text
  memory up-to-speed --project memory --llm

See also:
  docs/user/cli/up-to-speed.md";

const CHECKPOINT_GROUP_AFTER_HELP: &str = "\
Agent notes:
  Use for execution state. start-execution records approved plans; start-task records direct no-plan tasks.
  finish-execution verifies plan completion before the final response. Prefer --dry-run for previews.

Examples:
  memory checkpoint save --project memory
  memory checkpoint start-execution --project memory --plan-file /tmp/plan.md
  memory checkpoint start-task --project memory --title \"Fix query input\" --prompt \"Improve query input UX\"
  memory checkpoint finish-execution --project memory

See also:
  docs/user/cli/checkpoint.md";

const CHECKPOINT_START_TASK_AFTER_HELP: &str = "\
Agent notes:
  Run when an actionable user instruction starts execution without an approved plan.
  This records a task memory as the start marker; use remember after completion for the implemented outcome.
  Prefer --dry-run --json before wiring this into an agent workflow.

Examples:
  memory checkpoint start-task --project memory --title \"Fix query input\" --prompt \"Improve query input UX\"
  memory checkpoint start-task --project memory --title \"Update README\" --prompt \"Highlight the benchmark\" --dry-run --json

See also:
  docs/user/cli/checkpoint.md";

const CHECKPOINT_SAVE_AFTER_HELP: &str = "\
Agent notes:
  Saves a resumable project checkpoint. Use --dry-run for previews and --json for scripts.

Examples:
  memory checkpoint save --project memory
  memory checkpoint save --project memory --note \"Waiting on review\" --dry-run

See also:
  docs/user/cli/checkpoint.md";

const CHECKPOINT_SHOW_AFTER_HELP: &str = "\
Agent notes:
  Read-only inspection of the current saved checkpoint for resume workflows.

Examples:
  memory checkpoint show --project memory

See also:
  docs/user/cli/checkpoint.md";

const CHECKPOINT_START_AFTER_HELP: &str = "\
Agent notes:
  Run when an approved plan moves into implementation. It stores the plan and records a checkpoint.
  Use --plan-file for saved plans or --plan-stdin when piping generated markdown.

Examples:
  memory checkpoint start-execution --project memory --plan-file /tmp/plan.md
  memory checkpoint start-execution --project memory --plan-stdin --thread-key task-123

See also:
  docs/user/cli/checkpoint.md";

const CHECKPOINT_FINISH_AFTER_HELP: &str = "\
Agent notes:
  Run before claiming plan-backed work is complete. It verifies every approved plan item is done.
  Use --dry-run --json to inspect failures without recording implementation memory.

Examples:
  memory checkpoint finish-execution --project memory --dry-run --json
  memory checkpoint finish-execution --project memory --plan-file /tmp/plan.md --json

See also:
  docs/user/cli/checkpoint.md";

const CAPTURE_GROUP_AFTER_HELP: &str = "\
Agent notes:
  Low-level capture ingestion for structured task payloads. Prefer remember for normal completed-work capture.
  Mutates raw capture state unless --dry-run is passed on the leaf command.

Examples:
  memory capture task --file /tmp/task.json

See also:
  docs/user/cli/capture.md";

const CAPTURE_TASK_AFTER_HELP: &str = "\
Agent notes:
  Sends one JSON task payload to the backend. Use --dry-run to validate without writing.

Examples:
  memory capture task --file /tmp/task.json
  memory capture task --file /tmp/task.json --dry-run

See also:
  docs/user/cli/capture.md";

const SCAN_AFTER_HELP: &str = "\
Agent notes:
  Use when onboarding a repository or refreshing durable-memory candidates from source and git history.
  Requires LLM configuration for extraction. Use --dry-run and --json before writing candidates.

Examples:
  memory scan --project memory --dry-run --json
  memory scan --project memory --rebuild-index

See also:
  docs/user/cli/scan.md";

const REMEMBER_AFTER_HELP: &str = "\
Agent notes:
  Use after meaningful completed work to preserve durable facts with provenance.
  Include concrete notes, changed files, tests, and command output where useful.
  Use --type user, feedback, project, reference, or implementation when classification should be explicit.

Examples:
  memory remember --project memory --note \"Durable fact\"
  memory remember --project memory --title \"Task title\" --summary \"What changed\" --file-changed crates/mem-cli/src/main.rs

See also:
  docs/user/cli/remember.md";

const CURATE_AFTER_HELP: &str = "\
Agent notes:
  Converts raw captures into canonical memory. Use --dry-run before accepting curation decisions.

Examples:
  memory curate --project memory
  memory curate --project memory --batch-size 10 --dry-run

See also:
  docs/user/cli/curate.md";

const EMBEDDINGS_GROUP_AFTER_HELP: &str = "\
Agent notes:
  Manage semantic retrieval spaces. list/activate are global backend operations; reindex/reembed/prune operate per project.
  Reindex/reembed/prune mutate embedding rows unless --dry-run is passed.

Examples:
  memory embeddings list
  memory embeddings activate voyage-code
  memory embeddings reindex --project memory
  memory embeddings reembed --project memory
  memory embeddings prune --project memory --dry-run

See also:
  docs/user/cli/embeddings.md";

const EMBEDDINGS_LIST_AFTER_HELP: &str = "\
Agent notes:
  Read-only list of configured embedding backends. Active backends are marked with *; unresolved backends are marked with !.
  Use before activate, reindex, or reembed to avoid guessing backend names.

Examples:
  memory embeddings list

See also:
  docs/user/cli/embeddings.md";

const EMBEDDINGS_ACTIVATE_AFTER_HELP: &str = "\
Agent notes:
  Switches which configured backend query uses for semantic retrieval. It does not recompute embeddings.
  Run embeddings list first and backfill the target space with reembed or reindex if needed.

Examples:
  memory embeddings list
  memory embeddings activate voyage-code

See also:
  docs/user/cli/embeddings.md";

const EMBEDDINGS_REINDEX_AFTER_HELP: &str = "\
Agent notes:
  Rebuilds chunks and embeddings for a project. By default, covers every configured backend.
  Use --backend to restrict and --dry-run before writing.

Examples:
  memory embeddings reindex --project memory
  memory embeddings reindex --project memory --dry-run
  memory embeddings reindex --project memory --backend voyage-code

See also:
  docs/user/cli/embeddings.md";

const EMBEDDINGS_REEMBED_AFTER_HELP: &str = "\
Agent notes:
  Regenerates embeddings for eligible chunks without rebuilding chunks.
  Use after adding a backend or when coverage is partial. Use --dry-run before writing.

Examples:
  memory embeddings reembed --project memory
  memory embeddings reembed --project memory --dry-run
  memory embeddings reembed --project memory --backend voyage-code

See also:
  docs/user/cli/embeddings.md";

const EMBEDDINGS_PRUNE_AFTER_HELP: &str = "\
Agent notes:
  Deletes stale or orphaned embedding rows relative to configured backends.
  Always run --dry-run first from an agent.

Examples:
  memory embeddings prune --project memory
  memory embeddings prune --project memory --dry-run

See also:
  docs/user/cli/embeddings.md";

const HEALTH_AFTER_HELP: &str = "\
Agent notes:
  Read-only backend health check. Use doctor for full environment diagnosis.

Examples:
  memory health
  memory stats

See also:
  docs/user/cli/health.md";

const STATS_AFTER_HELP: &str = "\
Agent notes:
  Read-only memory and project summary. Use health for service reachability.

Examples:
  memory stats
  memory health

See also:
  docs/user/cli/health.md";

const ARCHIVE_AFTER_HELP: &str = "\
Agent notes:
  Archives low-signal memories by confidence and importance. Always use --dry-run first from an agent.

Examples:
  memory archive --project memory
  memory archive --project memory --max-confidence 0.2 --dry-run

See also:
  docs/user/cli/archive.md";

const AUTOMATION_GROUP_AFTER_HELP: &str = "\
Agent notes:
  Inspect or flush pending automation capture state. Status is read-only; flush mutates capture state unless --dry-run is passed.

Examples:
  memory automation status --project memory
  memory automation flush --project memory --curate --dry-run

See also:
  docs/user/cli/automation.md";

const AUTOMATION_STATUS_AFTER_HELP: &str = "\
Agent notes:
  Read-only view of pending automation state for a project.

Examples:
  memory automation status --project memory

See also:
  docs/user/cli/automation.md";

const AUTOMATION_FLUSH_AFTER_HELP: &str = "\
Agent notes:
  Converts pending automation state into capture records and can optionally curate them.
  Use --dry-run before writing; add --curate only when canonical memory should be produced immediately.

Examples:
  memory automation flush --project memory
  memory automation flush --project memory --curate --dry-run

See also:
  docs/user/cli/automation.md";

const TUI_AFTER_HELP: &str = "\
Agent notes:
  Opens the terminal UI for interactive browsing, querying, activity, watcher, and embedding views.
  Use CLI commands with --json for agent automation; use the TUI for human inspection.

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
    #[command(about = "Extract and inspect the project code graph.", after_help = GRAPH_GROUP_AFTER_HELP)]
    Graph(GraphArgs),
    #[command(about = "Export and import shareable project memory bundles.", after_help = BUNDLE_GROUP_AFTER_HELP)]
    Bundle(BundleArgs),
    #[command(about = "Save, inspect, and verify execution checkpoints.", after_help = CHECKPOINT_GROUP_AFTER_HELP)]
    Checkpoint(CheckpointArgs),
    #[command(about = "Generate a resume briefing for a project.", after_help = RESUME_AFTER_HELP)]
    Resume(ResumeArgs),
    #[command(about = "List persisted project activity events.", after_help = ACTIVITIES_AFTER_HELP)]
    Activities(ActivitiesArgs),
    #[command(about = "Generate a new-agent get-up-to-speed briefing.", after_help = UP_TO_SPEED_AFTER_HELP)]
    UpToSpeed(UpToSpeedArgs),
    #[command(about = "Run automated Memory quality evaluations.", after_help = EVAL_GROUP_AFTER_HELP)]
    Eval(EvalArgs),
    #[command(about = "Ask a project-specific question against curated memory.", after_help = QUERY_AFTER_HELP)]
    Query(QueryArgs),
    #[command(about = "Show the full version history for a memory, including tombstones.", after_help = HISTORY_AFTER_HELP)]
    History(HistoryArgs),
    #[command(about = "Prune old memory versions and tombstoned canonicals.", after_help = PRUNE_HISTORY_AFTER_HELP)]
    PruneHistory(PruneHistoryArgs),
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
    #[command(about = "Scaffold and inspect the dev-profile overlay.", after_help = DEV_GROUP_AFTER_HELP)]
    Dev(DevArgs),
}

#[derive(Debug, Args)]
struct DevArgs {
    #[command(subcommand)]
    command: DevCommand,
}

#[derive(Debug, Subcommand)]
enum DevCommand {
    /// Create `.mem/config.dev.toml` and the dev runtime directory.
    #[command(after_help = DEV_INIT_AFTER_HELP)]
    Init(DevInitArgs),
}

#[derive(Debug, Args)]
struct DevInitArgs {
    /// Overwrite an existing `.mem/config.dev.toml` instead of preserving it.
    #[arg(long)]
    force: bool,
    /// Print what would be written without touching the filesystem.
    #[arg(long)]
    dry_run: bool,
    /// Address the dev service should bind. Defaults to `127.0.0.1:4250`.
    #[arg(long, default_value = "127.0.0.1:4250")]
    bind_addr: String,
    /// Cap'n Proto TCP address for the dev service. Defaults to `127.0.0.1:4251`.
    #[arg(long, default_value = "127.0.0.1:4251")]
    capnp_tcp_addr: String,
    /// Copy database URL and LLM/embedding endpoints from the global config
    /// into the dev overlay. Without this flag and without a TTY, nothing is
    /// copied. With a TTY, the command asks interactively.
    #[arg(long)]
    copy_from_global: bool,
    /// Skip the interactive prompt and leave shared settings out of the overlay.
    #[arg(long, conflicts_with = "copy_from_global")]
    no_copy_from_global: bool,
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
    #[command(about = "Restart active Memory Layer services after an install or upgrade.", after_help = SERVICE_RESTART_ALL_AFTER_HELP)]
    RestartAll(ServiceRestartAllArgs),
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
struct ServiceRestartAllArgs {
    /// Preview active service discovery and restart actions without changing service state.
    #[arg(long)]
    dry_run: bool,
    /// Emit the restart report as JSON.
    #[arg(long)]
    json: bool,
    /// Write a TUI restart marker after restart planning/execution.
    #[arg(long)]
    mark_tui_restart: bool,
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
    #[command(about = "Run or manage the Codex-linked watcher manager.", after_help = WATCHER_MANAGER_GROUP_AFTER_HELP)]
    Manager(WatcherManagerArgs),
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
    /// Owning agent CLI name for agent-linked watcher mode.
    #[arg(long)]
    agent_cli: Option<String>,
    /// Owning agent session id for agent-linked watcher mode.
    #[arg(long)]
    agent_session_id: Option<String>,
    /// Owning agent pid for agent-linked watcher mode.
    #[arg(long)]
    agent_pid: Option<u32>,
    /// Owning agent started-at timestamp for agent-linked watcher mode.
    #[arg(long)]
    agent_started_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Args)]
struct WatcherManagerArgs {
    #[command(subcommand)]
    command: WatcherManagerCommand,
}

#[derive(Debug, Subcommand)]
enum WatcherManagerCommand {
    #[command(about = "Run the watcher manager in the foreground.", after_help = WATCHER_MANAGER_RUN_AFTER_HELP)]
    Run,
    #[command(about = "Enable the persistent user watcher manager service.", after_help = WATCHER_MANAGER_ENABLE_AFTER_HELP)]
    Enable(ServiceLifecycleArgs),
    #[command(about = "Disable the persistent user watcher manager service.", after_help = WATCHER_MANAGER_DISABLE_AFTER_HELP)]
    Disable(ServiceLifecycleArgs),
    #[command(about = "Show watcher manager status.", after_help = WATCHER_MANAGER_STATUS_AFTER_HELP)]
    Status,
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
    /// Include every historical version of each memory (including
    /// tombstones from deleted memories) in the search space. Default is
    /// latest-version-only.
    #[arg(long)]
    history: bool,
    /// Emit the query result as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Show the full version history for a memory, including tombstones.",
    after_help = HISTORY_AFTER_HELP
)]
struct HistoryArgs {
    /// Any version's id (including a tombstone). The chain resolves via
    /// canonical_id so passing any version id returns the same history.
    memory_id: Uuid,
    /// Emit the result as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Prune tombstoned canonical memories and superseded versions older than the configured thresholds.",
    after_help = PRUNE_HISTORY_AFTER_HELP
)]
struct PruneHistoryArgs {
    /// Limit the sweep to one project. Defaults to every project in the DB.
    #[arg(long)]
    project: Option<String>,
    /// Duration (e.g. 30d, 12h) after which a tombstoned canonical's rows
    /// are deleted entirely. Overrides config.retention.tombstone_after.
    #[arg(long, value_parser = humantime::parse_duration)]
    tombstone_after: Option<std::time::Duration>,
    /// Duration after which non-latest, non-tombstone versions are
    /// deleted. Overrides config.retention.superseded_after.
    #[arg(long, value_parser = humantime::parse_duration)]
    superseded_after: Option<std::time::Duration>,
    /// Preview counts without touching the database.
    #[arg(long)]
    dry_run: bool,
    /// Emit the result as JSON.
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
    about = "Extract and inspect the code graph produced from parser-backed repository analysis.",
    after_help = GRAPH_GROUP_AFTER_HELP
)]
struct GraphArgs {
    #[command(subcommand)]
    command: GraphCommand,
}

#[derive(Debug, Subcommand)]
enum GraphCommand {
    #[command(about = "Extract code graph facts from the local repository index.", after_help = GRAPH_EXTRACT_AFTER_HELP)]
    Extract(GraphExtractArgs),
    #[command(about = "Show the latest persisted code graph extraction status.", after_help = GRAPH_STATUS_AFTER_HELP)]
    Status(GraphStatusArgs),
}

#[derive(Debug, Args)]
struct GraphExtractArgs {
    /// Project slug to extract; defaults to the current repo when available.
    #[arg(long)]
    project: Option<String>,
    /// Limit the repository index context to changes after this timestamp or revision marker.
    #[arg(long)]
    since: Option<String>,
    /// Rebuild the local repository index before extracting graph facts.
    #[arg(long)]
    rebuild_index: bool,
    /// Create a fresh extraction run even when an identical completed run exists.
    #[arg(long)]
    force: bool,
    /// Preview extraction without writing database rows or the local index.
    #[arg(long)]
    dry_run: bool,
    /// Print a human-readable summary instead of the default JSON output.
    #[arg(long)]
    text: bool,
}

#[derive(Debug, Args)]
struct GraphStatusArgs {
    /// Project slug to inspect; defaults to the current repo when available.
    #[arg(long)]
    project: Option<String>,
    /// Print a human-readable summary instead of the default JSON output.
    #[arg(long)]
    text: bool,
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
    about = "List persisted project activity events.",
    after_help = ACTIVITIES_AFTER_HELP
)]
struct ActivitiesArgs {
    /// Project slug; defaults to the current repo when available.
    #[arg(long)]
    project: Option<String>,
    /// Maximum number of activities to return.
    #[arg(long, default_value_t = 50)]
    limit: usize,
    /// Filter by activity kind, for example query, plan, curate, or briefing.
    #[arg(long)]
    kind: Option<String>,
    /// Emit a human-readable text view instead of JSON.
    #[arg(long)]
    text: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Generate a new-agent get-up-to-speed briefing.",
    after_help = UP_TO_SPEED_AFTER_HELP
)]
struct UpToSpeedArgs {
    /// Project slug; defaults to the current repo when available.
    #[arg(long)]
    project: Option<String>,
    /// Maximum number of recent activities to use.
    #[arg(long, default_value_t = 20)]
    limit: usize,
    /// Ask the configured LLM to synthesize the briefing.
    #[arg(long)]
    llm: bool,
    /// Emit a human-readable text view instead of JSON.
    #[arg(long)]
    text: bool,
}

#[derive(Debug, Args)]
#[command(about = "Run automated Memory quality evaluations.", after_help = EVAL_GROUP_AFTER_HELP)]
struct EvalArgs {
    #[command(subcommand)]
    command: EvalCommand,
}

#[derive(Debug, Subcommand)]
enum EvalCommand {
    #[command(
        about = "Check whether an eval suite and environment are ready.",
        after_help = EVAL_DOCTOR_AFTER_HELP
    )]
    Doctor(EvalDoctorArgs),
    #[command(
        about = "Create a starter eval suite from recent project memories.",
        after_help = EVAL_SCAFFOLD_AFTER_HELP
    )]
    Scaffold(EvalScaffoldArgs),
    #[command(
        about = "Run one suite under one or more memory conditions.",
        after_help = EVAL_RUN_AFTER_HELP
    )]
    Run(EvalRunArgs),
    #[command(
        about = "Compare two eval run JSON files.",
        after_help = EVAL_COMPARE_AFTER_HELP
    )]
    Compare(EvalCompareArgs),
    #[command(
        about = "Render an eval comparison JSON file.",
        after_help = EVAL_REPORT_AFTER_HELP
    )]
    Report(EvalReportArgs),
    #[command(
        about = "Check an eval comparison against a gate policy.",
        after_help = EVAL_GATE_AFTER_HELP
    )]
    Gate(EvalGateArgs),
}

#[derive(Debug, Args)]
struct EvalDoctorArgs {
    /// Suite directory or suite.toml path to validate.
    #[arg(long)]
    suite: PathBuf,
    /// Emit a human-readable text view instead of JSON.
    #[arg(long)]
    text: bool,
}

#[derive(Debug, Args)]
struct EvalScaffoldArgs {
    /// Project slug; defaults to the current repo when available.
    #[arg(long)]
    project: Option<String>,
    /// Output directory for suite.toml and items.jsonl.
    #[arg(long)]
    out: PathBuf,
    /// Maximum number of starter items to generate.
    #[arg(long, default_value_t = 12)]
    limit: usize,
    /// Preview files without writing them.
    #[arg(long)]
    dry_run: bool,
    /// Emit a human-readable text view instead of JSON.
    #[arg(long)]
    text: bool,
}

#[derive(Debug, Args)]
struct EvalRunArgs {
    /// Suite directory or suite.toml path.
    #[arg(long)]
    suite: PathBuf,
    /// Condition to run. Repeat for paired runs.
    #[arg(long = "condition", default_value = "full-memory")]
    conditions: Vec<String>,
    /// Output directory for run JSON files.
    #[arg(long, default_value = "target/memory-evals")]
    out: PathBuf,
    /// Execution profile: llm for official provider-backed runs, offline for CI-safe dry scoring.
    #[arg(long, default_value = "llm")]
    profile: String,
    /// Number of repeated runs per condition.
    #[arg(long, default_value_t = 1)]
    repeat: usize,
    /// Optional token budget guard for one run group.
    #[arg(long)]
    max_cost: Option<u64>,
    /// Preserve raw answers/transcripts in artifacts. Currently metadata-only; answers are always kept.
    #[arg(long)]
    write_transcripts: bool,
    /// Add LLM judge scores for answer-like items. Deterministic checks still decide success.
    #[arg(long)]
    llm_judge: bool,
    /// Fail when the suite manifest is not marked reviewed.
    #[arg(long)]
    fail_on_unreviewed_labels: bool,
    /// Preview work without LLM calls or command execution.
    #[arg(long)]
    dry_run: bool,
    /// Emit a human-readable text view instead of JSON.
    #[arg(long)]
    text: bool,
}

#[derive(Debug, Args)]
struct EvalCompareArgs {
    /// Baseline run JSON file or glob. Repeat for multiple run artifacts.
    #[arg(long)]
    baseline: Vec<PathBuf>,
    /// Candidate run JSON file or glob. Repeat for multiple run artifacts.
    #[arg(long)]
    candidate: Vec<PathBuf>,
    /// Optional path to write comparison JSON.
    #[arg(long)]
    out: Option<PathBuf>,
    /// Emit a human-readable text view instead of JSON.
    #[arg(long)]
    text: bool,
}

#[derive(Debug, Args)]
struct EvalReportArgs {
    /// Comparison JSON file from memory eval compare.
    #[arg(long)]
    comparison: PathBuf,
    /// Optional file to write the rendered report.
    #[arg(long)]
    out: Option<PathBuf>,
    /// Emit a Markdown report instead of comparison JSON.
    #[arg(long)]
    markdown: bool,
    /// Emit a human-readable text view instead of JSON.
    #[arg(long)]
    text: bool,
}

#[derive(Debug, Args)]
struct EvalGateArgs {
    /// Comparison JSON file from memory eval compare.
    #[arg(long)]
    comparison: PathBuf,
    /// Gate policy TOML file.
    #[arg(long)]
    policy: PathBuf,
    /// Emit a human-readable text view instead of JSON.
    #[arg(long)]
    text: bool,
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
    #[command(about = "Record a direct no-plan task at execution start.", after_help = CHECKPOINT_START_TASK_AFTER_HELP)]
    StartTask(CheckpointStartTaskArgs),
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
struct CheckpointStartTaskArgs {
    /// Project slug to update; defaults to the current repo when available.
    #[arg(long)]
    project: Option<String>,
    /// Short title for the direct task.
    #[arg(long)]
    title: String,
    /// Original user instruction or task framing.
    #[arg(long)]
    prompt: String,
    /// Stable task thread key; derived from title/project when omitted.
    #[arg(long)]
    thread_key: Option<String>,
    /// Validate and preview the task-start flow without writing checkpoint or memory state.
    #[arg(long)]
    dry_run: bool,
    /// Emit the task-start report as JSON.
    #[arg(long)]
    json: bool,
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
    /// Optional explicit summary for the implementation memory recorded after verification.
    #[arg(long)]
    implementation_summary: Option<String>,
    /// Durable implementation detail to include in the recorded implementation memory.
    #[arg(long = "implementation-note")]
    implementation_notes: Vec<String>,
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
    /// Memory type to assign (e.g., user, feedback, project, reference, implementation).
    /// When set, overrides the automatic type classification.
    #[arg(long = "type")]
    memory_type: Option<String>,
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
    #[arg(long = "file-changed", visible_alias = "file")]
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
    #[command(about = "List configured embedding backends and show which is active.", after_help = EMBEDDINGS_LIST_AFTER_HELP)]
    List,
    #[command(about = "Switch which configured embedding backend is used for search.", after_help = EMBEDDINGS_ACTIVATE_AFTER_HELP)]
    Activate(EmbeddingsActivateArgs),
    #[command(about = "Build or refresh the active embedding index.", after_help = EMBEDDINGS_REINDEX_AFTER_HELP)]
    Reindex(EmbeddingsProjectArgs),
    #[command(about = "Generate embeddings for eligible chunks.", after_help = EMBEDDINGS_REEMBED_AFTER_HELP)]
    Reembed(EmbeddingsProjectArgs),
    #[command(about = "Delete stale or orphaned embedding rows.", after_help = EMBEDDINGS_PRUNE_AFTER_HELP)]
    Prune(EmbeddingsProjectArgs),
}

#[derive(Debug, Args)]
struct EmbeddingsActivateArgs {
    /// Name of the configured backend to activate.
    name: String,
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
    /// Restrict to a single configured backend by name. Omit to
    /// operate on every configured backend so every space stays
    /// covered.
    #[arg(long)]
    backend: Option<String>,
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
    about = "Open the terminal UI for browsing memories, querying memory, and inspecting project state.",
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
    if env::args()
        .nth(1)
        .is_some_and(|arg| arg == "--version" || arg == "-V")
    {
        println!(
            "memory {}",
            Profile::detect().display_version(env!("CARGO_PKG_VERSION"))
        );
        return Ok(());
    }

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
        Command::Dev(args) => {
            match &args.command {
                DevCommand::Init(init_args) => {
                    let cwd = env::current_dir().context("read current directory")?;
                    let repo_root = resolve_repo_root(&cwd)?;
                    let output = initialize_dev_overlay(&repo_root, init_args)?;
                    println!("{output}");
                }
            }
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
                        println!("{}", enable_backend_service(&config_path).await?);
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
                ServiceCommand::RestartAll(args) => {
                    let report = restart_all_memory_services(args.dry_run, args.mark_tui_restart)?;
                    if args.json {
                        println!("{}", serde_json::to_string_pretty(&report)?);
                    } else {
                        println!("{}", report.summary());
                    }
                }
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
                WatcherCommand::Manager(args) => match &args.command {
                    WatcherManagerCommand::Run => {}
                    WatcherManagerCommand::Enable(args) => {
                        let output = if args.dry_run {
                            preview_enable_watch_manager_service()?
                        } else {
                            enable_watch_manager_service(
                                &cli_config
                                    .clone()
                                    .unwrap_or_else(default_global_config_path),
                            )?
                        };
                        println!("{output}");
                        return Ok(());
                    }
                    WatcherManagerCommand::Disable(args) => {
                        let output = if args.dry_run {
                            preview_disable_watch_manager_service()?
                        } else {
                            disable_watch_manager_service(Profile::detect())?
                        };
                        println!("{output}");
                        return Ok(());
                    }
                    WatcherManagerCommand::Status => {
                        println!("{}", watch_manager_service_status(Profile::detect())?);
                        return Ok(());
                    }
                },
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
            if !watcher_command_requires_config_load(&args.command) {
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

    let cli_config_path = cli_config.clone();
    let config = AppConfig::load_from_path(cli_config).context("load config")?;
    let client = Client::builder()
        .timeout(config.service.request_timeout)
        .build()
        .context("build http client")?;

    match command {
        Command::Wizard(_) => unreachable!("wizard is handled before config loading"),
        Command::Init(_) => unreachable!("init is handled before config loading"),
        Command::Dev(_) => unreachable!("dev subcommands are handled before config loading"),
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
                history: args.history,
                retrieval_mode: None,
                answer_mode: None,
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
        Command::History(args) => {
            let payload: mem_api::MemoryHistoryResponse = get_json(
                client
                    .get(service_url(
                        &config,
                        &format!("/v1/memory/{}/history", args.memory_id),
                    ))
                    .send()
                    .await
                    .context("history request failed")?,
            )
            .await?;
            if args.json {
                println!("{}", serde_json::to_string(&payload)?);
            } else {
                print_memory_history(&payload);
            }
        }
        Command::PruneHistory(args) => {
            let request = mem_api::PruneHistoryRequest {
                project: args.project,
                tombstone_after: args.tombstone_after,
                superseded_after: args.superseded_after,
                dry_run: args.dry_run,
            };
            let payload: mem_api::PruneHistoryResponse = get_json(
                client
                    .post(service_url(&config, "/v1/prune-history"))
                    .headers(write_headers(&config)?)
                    .json(&request)
                    .send()
                    .await
                    .context("prune-history request failed")?,
            )
            .await?;
            if args.json {
                println!("{}", serde_json::to_string(&payload)?);
            } else {
                let verb = if payload.dry_run {
                    "Would prune"
                } else {
                    "Pruned"
                };
                let scope = payload
                    .project
                    .as_deref()
                    .map(|p| format!(" for project \"{p}\""))
                    .unwrap_or_default();
                println!(
                    "{verb} {} canonical tombstone(s) and {} superseded version(s){scope}.",
                    payload.canonicals_tombstoned_deleted, payload.superseded_versions_pruned
                );
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
        Command::Graph(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            let repo_root = resolve_repo_root(&cwd)?;
            match args.command {
                GraphCommand::Extract(args) => {
                    let project = resolve_project_slug(args.project, &cwd)?;
                    let index = scan::load_graph_index(
                        &repo_root,
                        &project,
                        args.since.as_deref(),
                        &config,
                        args.rebuild_index,
                        args.dry_run,
                    )?;
                    let request = mem_graph::GraphExtractionRequest {
                        project: index.project,
                        repo_root: index.repo_root,
                        git_head: index.head,
                        since: index.since,
                        force: args.force,
                        dry_run: args.dry_run,
                        index_reused: index.index_reused,
                        analysis: index.analysis,
                    };
                    let report = if args.dry_run {
                        mem_graph::build_extraction_preview(&request)
                    } else {
                        let pool = connect_graph_database(&config).await?;
                        mem_graph::run_migrations(&pool).await?;
                        mem_graph::PostgresGraphRepository::new(pool)
                            .extract(request)
                            .await?
                    };
                    if !report.dry_run {
                        let api = ApiClient::new(client.clone(), config.clone());
                        let activity_request = build_graph_activity_request(&report);
                        if let Err(error) = api.log_graph_activity(&activity_request).await {
                            eprintln!(
                                "warning: failed to log graph extraction activity for `{}`: {error}",
                                report.project
                            );
                        }
                    }
                    if args.text {
                        print_graph_extract_report(&report, &index.index_path);
                    } else {
                        println!("{}", serde_json::to_string_pretty(&report)?);
                    }
                }
                GraphCommand::Status(args) => {
                    let project = resolve_project_slug(args.project, &cwd)?;
                    let pool = connect_graph_database(&config).await?;
                    mem_graph::run_migrations(&pool).await?;
                    let status = mem_graph::PostgresGraphRepository::new(pool)
                        .latest_status(&project)
                        .await?;
                    if args.text {
                        print_graph_status(&status, &project);
                    } else {
                        println!("{}", serde_json::to_string_pretty(&status)?);
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
                    let thread_key =
                        derive_plan_thread_key(args.thread_key.as_deref(), title, &project);
                    let note = format!("Direct task started: {title}");
                    let (checkpoint, path) = if args.dry_run {
                        preview_checkpoint(&project, &repo_root, Some(note))?
                    } else {
                        save_checkpoint_with_activity(&api, &project, &repo_root, Some(note))
                            .await?
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
                            eprintln!(
                                "warning: failed to log plan activity for `{project}`: {error}"
                            );
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
                    repo_root: Some(repo_root.display().to_string()),
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
        Command::Activities(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            let project = resolve_project_slug(args.project, &cwd)?;
            let api = ApiClient::new(client, config);
            let payload = api
                .project_activities(&project, args.limit.clamp(1, 500), args.kind.as_deref())
                .await?;
            if args.text {
                print_activities_response(&payload);
            } else {
                println!("{}", serde_json::to_string_pretty(&payload)?);
            }
        }
        Command::UpToSpeed(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            let project = resolve_project_slug(args.project, &cwd)?;
            let api = ApiClient::new(client, config);
            let payload = api
                .up_to_speed(&UpToSpeedRequest {
                    project,
                    include_llm_summary: args.llm,
                    limit: args.limit.clamp(1, 50),
                })
                .await?;
            if args.text {
                print_up_to_speed_response(&payload);
            } else {
                println!("{}", serde_json::to_string_pretty(&payload)?);
            }
        }
        Command::Eval(args) => {
            let cwd = env::current_dir().context("read current directory")?;
            let api = ApiClient::new(client, config);
            handle_eval_command(args, &cwd, &api).await?;
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
            let curate = if dry_run {
                api.curate(&project, repo_replacement_policy(&repo_root), true)
                    .await?
            } else {
                api.curate_capture(
                    &project,
                    capture.raw_capture_id,
                    repo_replacement_policy(&repo_root),
                    false,
                )
                .await?
            };
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
                    raw_capture_id: None,
                    replacement_policy: Some(replacement_policy),
                    dry_run: args.dry_run,
                })
                .send()
                .await?;
            print_json_response(response).await?;
        }
        Command::Embeddings(args) => match args.command {
            EmbeddingsCommand::List => {
                let api = ApiClient::new(client.clone(), config.clone());
                let payload = api.list_embedding_backends(None).await?;
                print_embedding_backends(&payload);
            }
            EmbeddingsCommand::Activate(args) => {
                let api = ApiClient::new(client.clone(), config.clone());
                let payload = api.activate_embedding_backend(&args.name).await?;
                print_embedding_backends(&payload);
            }
            EmbeddingsCommand::Reindex(args) => {
                let response = client
                    .post(service_url(&config, "/v1/reindex"))
                    .headers(write_headers(&config)?)
                    .json(&ReindexRequest {
                        project: args.project,
                        dry_run: args.dry_run,
                        backend: args.backend,
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
                        backend: args.backend,
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
                        agent_cli: args.agent_cli,
                        agent_session_id: args.agent_session_id,
                        agent_pid: args.agent_pid,
                        agent_started_at: args.agent_started_at,
                    },
                    writer.id,
                    writer.name,
                )
                .await?;
            }
            WatcherCommand::Manager(args) => match args.command {
                WatcherManagerCommand::Run => run_watcher_manager(config, cli_config_path).await?,
                WatcherManagerCommand::Enable(_)
                | WatcherManagerCommand::Disable(_)
                | WatcherManagerCommand::Status => {
                    unreachable!(
                        "watcher manager lifecycle commands are handled before config loading"
                    )
                }
            },
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
        if let Some((name, value)) = trimmed.split_once('=')
            && name.trim() == key
        {
            return Some(value.trim().to_string());
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

        let mut database_connect_error = None;
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
                    database_connect_error = Some(error.to_string());
                    report.push(doctor_check(
                        "database.connect",
                        DoctorStatus::Fail,
                        "Could not connect to the configured database directly.",
                        Some(error.to_string()),
                        Some(if config.cluster.enabled {
                            "Fix the database URL or credentials first, or start another database-connected Memory Layer backend on the local network for relay discovery.".to_string()
                        } else {
                            format!(
                                "Fix the database URL or credentials first, or enable relay discovery by setting [cluster].enabled = true in {}.",
                                global_config_path
                                    .as_ref()
                                    .unwrap_or(&config_path)
                                    .display()
                            )
                        }),
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
            if config.service.api_token.trim().is_empty()
                || config.service.api_token == DEV_API_TOKEN
            {
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
                config.llm.provider,
                effective_llm_base_url(&config.llm)
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
        let llm_api_key_value = resolve_llm_api_key(&config.llm).unwrap_or_default();
        let llm_api_key_required = llm_requires_api_key(&config.llm);
        report.push(doctor_check(
            "config.llm_api_key",
            if !llm_api_key_required {
                DoctorStatus::Skipped
            } else if llm_api_key_value.trim().is_empty() {
                DoctorStatus::Fail
            } else {
                DoctorStatus::Ok
            },
            if !llm_api_key_required {
                "LLM API key is optional for this provider."
            } else if llm_api_key_value.trim().is_empty() {
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

        if is_ollama_provider(&config.llm.provider) {
            let models_url = format!("{}/models", effective_llm_base_url(&config.llm));
            let ollama_check = match Client::new().get(&models_url).send().await {
                Ok(response) if response.status().is_success() => {
                    match response.json::<serde_json::Value>().await {
                        Ok(body) => {
                            let model = config.llm.model.trim();
                            let found = body
                                .get("data")
                                .and_then(|value| value.as_array())
                                .is_some_and(|models| {
                                    models.iter().any(|entry| {
                                        entry
                                            .get("id")
                                            .and_then(|value| value.as_str())
                                            .is_some_and(|id| id == model)
                                    })
                                });
                            doctor_check(
                                "config.ollama",
                                if found {
                                    DoctorStatus::Ok
                                } else {
                                    DoctorStatus::Warn
                                },
                                if found {
                                    "Ollama is reachable and the configured model is available."
                                } else {
                                    "Ollama is reachable but the configured model was not listed."
                                },
                                Some(models_url),
                                (!found).then(|| format!("Run `ollama pull {model}`")),
                                false,
                            )
                        }
                        Err(error) => doctor_check(
                            "config.ollama",
                            DoctorStatus::Warn,
                            "Ollama responded but the model list could not be parsed.",
                            Some(models_url),
                            Some(error.to_string()),
                            false,
                        ),
                    }
                }
                Ok(response) => doctor_check(
                    "config.ollama",
                    DoctorStatus::Fail,
                    "Ollama model endpoint returned an error.",
                    Some(models_url),
                    Some(format!("HTTP {}", response.status())),
                    false,
                ),
                Err(error) => doctor_check(
                    "config.ollama",
                    DoctorStatus::Fail,
                    "Ollama is not reachable at the configured base URL.",
                    Some(models_url),
                    Some(format!("Start Ollama with `ollama serve`: {error}")),
                    false,
                ),
            };
            report.push(ollama_check);
        }

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
        report.push(doctor_check(
            "config.relay_discovery",
            if config.cluster.enabled {
                DoctorStatus::Ok
            } else if database_connect_error.is_some() {
                DoctorStatus::Warn
            } else {
                DoctorStatus::Ok
            },
            if config.cluster.enabled {
                "Relay discovery is enabled for backend failover."
            } else {
                "Relay discovery is disabled."
            },
            Some(format!(
                "enabled={} multicast={} priority={}",
                config.cluster.enabled,
                config.cluster.discovery_multicast_addr,
                config.cluster.priority
            )),
            if config.cluster.enabled {
                None
            } else {
                Some(format!(
                    "Set [cluster].enabled = true in {} to allow this backend to discover and proxy to another Memory Layer backend when PostgreSQL is unavailable.",
                    global_config_path
                        .as_ref()
                        .unwrap_or(&config_path)
                        .display()
                ))
            },
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

        #[cfg(target_os = "macos")]
        {
            let manager_plist_path = watch_manager_launch_agent_path()?;
            let manager_status = launch_agent_status(watch_manager_launch_agent_label())?;
            let manager_installed = manager_plist_path.exists();
            report.push(doctor_check(
                "watcher.manager_service",
                if manager_status.running {
                    DoctorStatus::Ok
                } else {
                    DoctorStatus::Warn
                },
                if manager_status.running {
                    "Agent-linked watcher manager service is active."
                } else if manager_installed || manager_status.loaded {
                    "Agent-linked watcher manager service is installed but not active."
                } else {
                    "Agent-linked watcher manager service is not installed."
                },
                Some(format!(
                    "installed={} loaded={} active={} plist={}",
                    yes_no(manager_installed),
                    yes_no(manager_status.loaded),
                    yes_no(manager_status.running),
                    manager_plist_path.display()
                )),
                if manager_status.running {
                    None
                } else {
                    Some("memory watcher manager enable".to_string())
                },
                false,
            ));
        }

        #[cfg(not(target_os = "macos"))]
        {
            let manager_unit_path = user_systemd_unit_dir()?.join(WATCH_MANAGER_UNIT_NAME);
            let manager_installed = manager_unit_path.exists();
            let manager_enabled =
                run_systemctl_user(["is-enabled", WATCH_MANAGER_UNIT_NAME]).is_ok();
            let manager_active = run_systemctl_user(["is-active", WATCH_MANAGER_UNIT_NAME]).is_ok();
            report.push(doctor_check(
                "watcher.manager_service",
                if manager_active {
                    DoctorStatus::Ok
                } else {
                    DoctorStatus::Warn
                },
                if manager_active {
                    "Agent-linked watcher manager service is active."
                } else if manager_installed {
                    "Agent-linked watcher manager service is installed but not active."
                } else {
                    "Agent-linked watcher manager service is not installed."
                },
                Some(format!(
                    "installed={} enabled={} active={} unit={}",
                    yes_no(manager_installed),
                    yes_no(manager_enabled),
                    yes_no(manager_active),
                    manager_unit_path.display()
                )),
                if manager_active {
                    None
                } else {
                    Some("memory watcher manager enable".to_string())
                },
                false,
            ));
        }

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
                                    Some(if cfg!(target_os = "macos") {
                                        format!("memory watcher enable --project {}", project)
                                    } else {
                                        "memory watcher manager enable".to_string()
                                    })
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
                    Some(if database_connect_error.is_some() && !config.cluster.enabled {
                        format!(
                            "{} or enable relay discovery in {} and rerun `memory service enable`",
                            backend_start_hint(&config_path),
                            global_config_path
                                .as_ref()
                                .unwrap_or(&config_path)
                                .display()
                        )
                    } else {
                        backend_start_hint(&config_path)
                    }),
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
    ensure_claude_md_memory_section(repo_root, project)?;
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
    if let Some((prefix, rest)) = value.split_once("://")
        && let Some((creds, suffix)) = rest.split_once('@')
        && creds.contains(':')
    {
        return format!("{prefix}://<redacted>@{suffix}");
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
        ensure_claude_md_memory_section(repo_root, project)?;
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

fn initialize_dev_overlay(repo_root: &Path, args: &DevInitArgs) -> Result<String> {
    // Prefer the .mem/ that the config loader would find (ancestor walk from
    // cwd), so running `memory dev init` inside a git worktree lands the
    // overlay next to the existing base config in the main repo.
    let mem_dir = mem_api::discover_repo_config_path()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| repo_root.join(".mem"));
    if !mem_dir.is_dir() {
        anyhow::bail!(
            "no .mem/ directory found at {}. Run `memory init` first to bootstrap the \
             base config before layering the dev overlay on top.",
            mem_dir.display()
        );
    }
    let overlay_path = mem_dir.join("config.dev.toml");
    let runtime_dev_dir = mem_dir.join("runtime").join("dev");
    let capnp_unix_socket = runtime_dev_dir.join("memory-layer.capnp.sock");
    let state_file_path = runtime_dev_dir.join("automation-state.json");
    let audit_log_path = runtime_dev_dir.join("automation.log");

    let shared_snippet = resolve_shared_global_snippet(args)?;

    let mut contents = format!(
        "# Overlay on top of .mem/config.toml for the dev profile.\n\
         # Active when MEMORY_LAYER_PROFILE=dev or the binary runs from a cargo target/ directory.\n\
         # The dev profile does NOT read the global config — anything shared (database URL,\n\
         # LLM endpoints) lives here. Re-run `memory dev init --copy-from-global` to refresh.\n\
         \n\
         [service]\n\
         bind_addr = \"{bind_addr}\"\n\
         capnp_tcp_addr = \"{capnp_tcp_addr}\"\n\
         capnp_unix_socket = \"{capnp_unix_socket}\"\n\
         \n\
         [automation]\n\
         state_file_path = \"{state_file_path}\"\n\
         audit_log_path = \"{audit_log_path}\"\n\
         \n\
         [cluster]\n\
         service_id = \"memory-layer-dev\"\n",
        bind_addr = args.bind_addr,
        capnp_tcp_addr = args.capnp_tcp_addr,
        capnp_unix_socket = capnp_unix_socket.display(),
        state_file_path = state_file_path.display(),
        audit_log_path = audit_log_path.display(),
    );
    if !shared_snippet.is_empty() {
        contents.push('\n');
        contents.push_str(&shared_snippet);
    }

    if args.dry_run {
        return Ok(format!(
            "[dry run] would write {} ({} bytes) and create {}\n\n{}",
            overlay_path.display(),
            contents.len(),
            runtime_dev_dir.display(),
            contents
        ));
    }

    fs::create_dir_all(&runtime_dev_dir)
        .with_context(|| format!("create {}", runtime_dev_dir.display()))?;
    if overlay_path.exists() && !args.force {
        return Ok(format!(
            "dev overlay already present at {} (use --force to overwrite)",
            overlay_path.display()
        ));
    }
    fs::write(&overlay_path, &contents)
        .with_context(|| format!("write {}", overlay_path.display()))?;
    Ok(format!(
        "wrote {} and ensured {}\nnext: run `cargo run --bin memory -- service run` in another \
         shell, then `cargo run --bin memory -- tui`.",
        overlay_path.display(),
        runtime_dev_dir.display()
    ))
}

/// Tables we willingly copy from the global config into the dev overlay. The
/// service endpoint + automation paths + cluster id are intentionally
/// excluded so the dev stack always diverges where it matters.
const SHARED_GLOBAL_SECTIONS: &[&str] = &["database", "llm", "embeddings", "features", "writer"];

fn resolve_shared_global_snippet(args: &DevInitArgs) -> Result<String> {
    let Some(global_path) = mem_api::discover_global_config_path() else {
        if args.copy_from_global {
            anyhow::bail!(
                "--copy-from-global was set but no global config was found \
                 (expected one of the paths reported by `memory doctor`)"
            );
        }
        return Ok(String::new());
    };
    let should_copy = if args.copy_from_global {
        true
    } else if args.no_copy_from_global {
        false
    } else if io::stdin().is_terminal() && io::stdout().is_terminal() {
        prompt_yes_no(&format!(
            "Copy shared settings (database URL, LLM/embedding endpoints) from {} into the dev overlay?",
            global_path.display()
        ))?
    } else {
        false
    };
    if !should_copy {
        return Ok(String::new());
    }
    let raw = fs::read_to_string(&global_path)
        .with_context(|| format!("read {}", global_path.display()))?;
    let value: toml::Value =
        toml::from_str(&raw).with_context(|| format!("parse {}", global_path.display()))?;
    let Some(table) = value.as_table() else {
        return Ok(String::new());
    };
    let mut copied = toml::value::Table::new();
    for section in SHARED_GLOBAL_SECTIONS {
        if let Some(value) = table.get(*section) {
            copied.insert((*section).to_string(), value.clone());
        }
    }
    if copied.is_empty() {
        return Ok(String::new());
    }
    let rendered =
        toml::to_string(&toml::Value::Table(copied)).context("serialize shared sections")?;
    Ok(format!(
        "# Copied from {} — re-run `memory dev init --copy-from-global --force` to refresh.\n{}",
        global_path.display(),
        rendered
    ))
}

async fn enable_backend_service(config_path: &Path) -> Result<String> {
    let config = AppConfig::load_from_path(Some(config_path.to_path_buf()))
        .context("load config for backend service enable")?;
    let startup_output = start_backend_service_once(config_path)?;
    match wait_for_backend_health(config_path).await {
        Ok(health) => Ok(format!(
            "{startup_output}\n\n{}",
            format_backend_health_summary(&health)
        )),
        Err(start_error) => {
            let database_error = check_database_connectivity(&config).await.err();
            if !config.cluster.enabled
                && let Some(database_error) = database_error
            {
                if io::stdin().is_terminal()
                    && io::stdout().is_terminal()
                    && prompt_yes_no(&format!(
                        "Backend could not reach PostgreSQL ({database_error}). Enable relay discovery in {} and retry?",
                        config_path.display()
                    ))?
                {
                    set_cluster_enabled_in_shared_config(config_path, true)?;
                    let _ = disable_backend_service();
                    let retry_output = start_backend_service_once(config_path)?;
                    let health = wait_for_backend_health(config_path).await?;
                    return Ok(format!(
                        "Enabled relay discovery in {}.\n{}\n\n{}",
                        config_path.display(),
                        retry_output,
                        format_backend_health_summary(&health)
                    ));
                }
                anyhow::bail!(
                    "Backend did not become healthy after startup.\nLikely cause: {database_error}\nRecovery: enable relay discovery by setting [cluster].enabled = true in {} and rerun `memory service enable`.",
                    config_path.display()
                );
            }
            Err(start_error)
        }
    }
}

fn start_backend_service_once(config_path: &Path) -> Result<String> {
    #[cfg(not(target_os = "macos"))]
    let _ = config_path;

    #[cfg(target_os = "macos")]
    {
        let plist_path = backend_launch_agent_path()?;
        let label = backend_launch_agent_label();
        let stdout_path = user_memory_layer_log_dir()?.join("mem-service.stdout.log");
        let stderr_path = user_memory_layer_log_dir()?.join("mem-service.stderr.log");
        write_launch_agent(
            &plist_path,
            render_backend_launch_agent(config_path)?,
            label,
        )?;
        bootstrap_launch_agent(&plist_path, label)?;
        Ok(format!(
            "Installed and started backend LaunchAgent {}.\nPlist: {}\nConfig: {}\nLogs:\n- {}\n- {}\n\nManage it with:\n- memory service status\n- memory service disable\n- launchctl kickstart -k {}/{}",
            label,
            plist_path.display(),
            config_path.display(),
            stdout_path.display(),
            stderr_path.display(),
            launchctl_domain_target()?,
            label,
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
        match backend_launch_agent_path() {
            Ok(plist_path) => format!(
                "Dry run: would install and start backend LaunchAgent {}.\nPlist: {}\nConfig: {}",
                backend_launch_agent_label(),
                plist_path.display(),
                config_path.display()
            ),
            Err(_) => format!(
                "Dry run: would install and start the backend LaunchAgent with config {}",
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

pub(crate) async fn enable_relay_discovery_and_restart_backend() -> Result<String> {
    let config_path = discover_global_config_path().unwrap_or_else(default_global_config_path);
    set_cluster_enabled_in_shared_config(&config_path, true)?;
    let _ = disable_backend_service();
    enable_backend_service(&config_path).await
}

fn disable_backend_service() -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        let plist_path = backend_launch_agent_path()?;
        let label = backend_launch_agent_label();
        let _ = bootout_launch_agent(&plist_path, label);
        if plist_path.exists() {
            fs::remove_file(&plist_path)
                .with_context(|| format!("remove {}", plist_path.display()))?;
        }
        Ok(format!(
            "Disabled backend LaunchAgent {}.\nRemoved plist: {}",
            label,
            plist_path.display()
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
        match backend_launch_agent_path() {
            Ok(plist_path) => format!(
                "Dry run: would disable backend LaunchAgent {} and remove {}\nConfig: {}",
                backend_launch_agent_label(),
                plist_path.display(),
                config_path.display()
            ),
            Err(_) => format!(
                "Dry run: would disable the backend LaunchAgent configured by {}",
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
        let plist_path = backend_launch_agent_path()?;
        let label = backend_launch_agent_label();
        let status = launch_agent_status(label)?;
        Ok(format!(
            "Backend service:\n- label: {}\n- plist: {}\n- config: {}\n- installed: {}\n- running: {}\n\nInspect with:\n- launchctl print {}/{}\n- tail -f {}",
            label,
            plist_path.display(),
            config_path.display(),
            yes_no(plist_path.exists() || status.loaded),
            yes_no(status.running),
            launchctl_domain_target()?,
            label,
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

const TUI_RESTART_MARKER_FILE: &str = "tui-restart-required.json";
#[cfg(not(target_os = "macos"))]
const LINUX_GLOBAL_TUI_RESTART_MARKER: &str = "/var/lib/memory-layer/tui-restart-required.json";
#[cfg(target_os = "macos")]
const MACOS_GLOBAL_TUI_RESTART_MARKER: &str =
    "/usr/local/var/memory-layer/tui-restart-required.json";

#[derive(Debug, Clone, Serialize)]
struct ServiceRestartReport {
    dry_run: bool,
    marked_tui_restart: bool,
    marker_paths: Vec<String>,
    operations: Vec<ServiceRestartOperation>,
}

impl ServiceRestartReport {
    fn summary(&self) -> String {
        let mut lines = vec![format!(
            "Memory Layer service restart{}:",
            if self.dry_run { " dry run" } else { "" }
        )];
        for operation in &self.operations {
            lines.push(format!(
                "- {} [{}]: {}{}",
                operation.name,
                operation.manager,
                operation.action,
                operation
                    .message
                    .as_deref()
                    .filter(|value| !value.is_empty())
                    .map(|value| format!(" ({value})"))
                    .unwrap_or_default()
            ));
        }
        if self.marked_tui_restart {
            lines.push(format!(
                "TUI restart marker written: {}",
                self.marker_paths.join(", ")
            ));
        }
        lines.join("\n")
    }
}

#[derive(Debug, Clone, Serialize)]
struct ServiceRestartOperation {
    name: String,
    manager: String,
    active: bool,
    action: String,
    success: bool,
    message: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct TuiRestartMarker {
    pub(crate) version: String,
    pub(crate) marked_at: DateTime<Utc>,
    pub(crate) reason: String,
    pub(crate) binary_path: String,
    pub(crate) restarted_services: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TuiRestartNotice {
    pub(crate) marker_path: PathBuf,
    pub(crate) version: String,
    pub(crate) reason: String,
}

fn restart_all_memory_services(
    dry_run: bool,
    mark_tui_restart: bool,
) -> Result<ServiceRestartReport> {
    let mut operations = Vec::new();
    restart_platform_services(dry_run, &mut operations)?;
    let restarted_services = operations
        .iter()
        .filter(|operation| {
            operation.active
                && (operation.success || dry_run)
                && (operation.action == "restart" || operation.action == "would-restart")
        })
        .map(|operation| operation.name.clone())
        .collect::<Vec<_>>();
    let marker_paths = if mark_tui_restart && !dry_run {
        write_tui_restart_marker("install-or-upgrade", restarted_services)?
    } else {
        Vec::new()
    };
    Ok(ServiceRestartReport {
        dry_run,
        marked_tui_restart: mark_tui_restart && !dry_run && !marker_paths.is_empty(),
        marker_paths: marker_paths
            .into_iter()
            .map(|path| path.display().to_string())
            .collect(),
        operations,
    })
}

#[cfg(not(target_os = "macos"))]
fn restart_platform_services(
    dry_run: bool,
    operations: &mut Vec<ServiceRestartOperation>,
) -> Result<()> {
    for unit in ["memory-layer.service", "memory-watch.service"] {
        restart_systemd_system_unit_if_active(unit, dry_run, operations);
    }
    for scope in active_memory_user_unit_scopes() {
        for unit in &scope.units {
            restart_systemd_user_unit_if_active(&scope, unit, dry_run, operations);
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn restart_platform_services(
    dry_run: bool,
    operations: &mut Vec<ServiceRestartOperation>,
) -> Result<()> {
    for label in active_launch_agent_labels()? {
        restart_launch_agent_if_loaded(&label, dry_run, operations);
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn restart_systemd_system_unit_if_active(
    unit: &str,
    dry_run: bool,
    operations: &mut Vec<ServiceRestartOperation>,
) {
    let active = run_systemctl_system(["is-active", "--quiet", unit]).is_ok();
    if !active {
        operations.push(ServiceRestartOperation {
            name: unit.to_string(),
            manager: "systemd-system".to_string(),
            active,
            action: "skip-inactive".to_string(),
            success: true,
            message: None,
        });
        return;
    }
    if dry_run {
        operations.push(ServiceRestartOperation {
            name: unit.to_string(),
            manager: "systemd-system".to_string(),
            active,
            action: "would-restart".to_string(),
            success: true,
            message: None,
        });
        return;
    }
    let result = run_systemctl_system(["restart", unit]);
    operations.push(ServiceRestartOperation {
        name: unit.to_string(),
        manager: "systemd-system".to_string(),
        active,
        action: "restart".to_string(),
        success: result.is_ok(),
        message: result.err().map(|error| error.to_string()),
    });
}

#[cfg(not(target_os = "macos"))]
#[derive(Debug, Clone)]
struct SystemdUserScope {
    manager_label: String,
    username: Option<String>,
    runtime_dir: Option<PathBuf>,
    units: Vec<String>,
}

#[cfg(not(target_os = "macos"))]
fn active_memory_user_unit_scopes() -> Vec<SystemdUserScope> {
    if running_as_root() {
        let scopes = active_logged_in_user_memory_unit_scopes();
        if !scopes.is_empty() {
            return scopes;
        }
    }
    active_current_user_memory_units()
        .into_iter()
        .next()
        .map(|units| SystemdUserScope {
            manager_label: "systemd-user".to_string(),
            username: None,
            runtime_dir: None,
            units,
        })
        .into_iter()
        .collect()
}

#[cfg(not(target_os = "macos"))]
fn running_as_root() -> bool {
    ProcessCommand::new("id")
        .arg("-u")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim() == "0")
        .unwrap_or(false)
}

#[cfg(not(target_os = "macos"))]
fn active_logged_in_user_memory_unit_scopes() -> Vec<SystemdUserScope> {
    let Ok(entries) = fs::read_dir("/run/user") else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let runtime_dir = entry.path();
            let uid = runtime_dir.file_name()?.to_str()?.to_string();
            let username = username_for_uid(&uid)?;
            let units = active_user_memory_units_for(&username, Some(&runtime_dir));
            (!units.is_empty()).then_some(SystemdUserScope {
                manager_label: format!("systemd-user:{username}"),
                username: Some(username),
                runtime_dir: Some(runtime_dir),
                units,
            })
        })
        .collect()
}

#[cfg(not(target_os = "macos"))]
fn username_for_uid(uid: &str) -> Option<String> {
    let output = ProcessCommand::new("getent")
        .args(["passwd", uid])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .split(':')
        .next()
        .map(str::to_string)
        .filter(|value| !value.is_empty())
}

#[cfg(not(target_os = "macos"))]
fn active_current_user_memory_units() -> Option<Vec<String>> {
    let units = active_user_memory_units_for("", None);
    (!units.is_empty()).then_some(units)
}

#[cfg(not(target_os = "macos"))]
fn active_user_memory_units_for(username: &str, runtime_dir: Option<&Path>) -> Vec<String> {
    let mut command = if let Some(runtime_dir) = runtime_dir {
        let mut command = ProcessCommand::new("runuser");
        command
            .args(["-u", username, "--", "env"])
            .arg(format!("XDG_RUNTIME_DIR={}", runtime_dir.display()))
            .args([
                "systemctl",
                "--user",
                "list-units",
                "--type=service",
                "--state=active",
                "--no-legend",
                "memory-watch*.service",
            ]);
        command
    } else {
        let mut command = ProcessCommand::new("systemctl");
        command.args([
            "--user",
            "list-units",
            "--type=service",
            "--state=active",
            "--no-legend",
            "memory-watch*.service",
        ]);
        command
    };
    let output = command.output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    parse_systemd_unit_names(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(not(target_os = "macos"))]
fn parse_systemd_unit_names(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|unit| unit.starts_with("memory-watch") && unit.ends_with(".service"))
        .map(ToString::to_string)
        .collect()
}

#[cfg(not(target_os = "macos"))]
fn restart_systemd_user_unit_if_active(
    scope: &SystemdUserScope,
    unit: &str,
    dry_run: bool,
    operations: &mut Vec<ServiceRestartOperation>,
) {
    if dry_run {
        operations.push(ServiceRestartOperation {
            name: unit.to_string(),
            manager: scope.manager_label.clone(),
            active: true,
            action: "would-restart".to_string(),
            success: true,
            message: None,
        });
        return;
    }
    let result = if let (Some(username), Some(runtime_dir)) = (&scope.username, &scope.runtime_dir)
    {
        run_systemctl_user_for(username, runtime_dir, ["restart", unit])
    } else {
        run_systemctl_user(["restart", unit])
    };
    operations.push(ServiceRestartOperation {
        name: unit.to_string(),
        manager: scope.manager_label.clone(),
        active: true,
        action: "restart".to_string(),
        success: result.is_ok(),
        message: result.err().map(|error| error.to_string()),
    });
}

#[cfg(target_os = "macos")]
fn active_launch_agent_labels() -> Result<Vec<String>> {
    let mut labels = vec![
        backend_launch_agent_label().to_string(),
        watch_manager_launch_agent_label().to_string(),
    ];
    if let Some(dir) = platform::user_launch_agents_dir() {
        if dir.is_dir() {
            for entry in fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
                let path = entry?.path();
                let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
                    continue;
                };
                if file_name.starts_with("com.memory-layer.memory-watch")
                    && file_name.ends_with(".plist")
                {
                    labels.push(file_name.trim_end_matches(".plist").to_string());
                }
            }
        }
    }
    labels.sort();
    labels.dedup();
    Ok(labels)
}

#[cfg(target_os = "macos")]
fn restart_launch_agent_if_loaded(
    label: &str,
    dry_run: bool,
    operations: &mut Vec<ServiceRestartOperation>,
) {
    let status = launch_agent_status(label).unwrap_or_default();
    if !status.loaded {
        operations.push(ServiceRestartOperation {
            name: label.to_string(),
            manager: "launchctl".to_string(),
            active: false,
            action: "skip-unloaded".to_string(),
            success: true,
            message: None,
        });
        return;
    }
    if dry_run {
        operations.push(ServiceRestartOperation {
            name: label.to_string(),
            manager: "launchctl".to_string(),
            active: true,
            action: "would-restart".to_string(),
            success: true,
            message: None,
        });
        return;
    }
    let target = format!(
        "{}/{}",
        launchctl_domain_target().unwrap_or_else(|_| "gui/unknown".to_string()),
        label
    );
    let result = run_launchctl(["kickstart", "-k", &target]);
    operations.push(ServiceRestartOperation {
        name: label.to_string(),
        manager: "launchctl".to_string(),
        active: true,
        action: "restart".to_string(),
        success: result.is_ok(),
        message: result.err().map(|error| error.to_string()),
    });
}

pub(crate) fn tui_restart_marker_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(dir) = platform::preferred_user_state_dir() {
        paths.push(dir.join(TUI_RESTART_MARKER_FILE));
    }
    #[cfg(not(target_os = "macos"))]
    paths.push(PathBuf::from(LINUX_GLOBAL_TUI_RESTART_MARKER));
    #[cfg(target_os = "macos")]
    paths.push(PathBuf::from(MACOS_GLOBAL_TUI_RESTART_MARKER));
    paths.sort();
    paths.dedup();
    paths
}

fn write_tui_restart_marker(reason: &str, restarted_services: Vec<String>) -> Result<Vec<PathBuf>> {
    let marker = TuiRestartMarker {
        version: Profile::detect().display_version(env!("CARGO_PKG_VERSION")),
        marked_at: Utc::now(),
        reason: reason.to_string(),
        binary_path: memory_binary_path()
            .unwrap_or_else(|_| PathBuf::from("memory"))
            .display()
            .to_string(),
        restarted_services,
    };
    let contents = serde_json::to_vec_pretty(&marker)?;
    let mut written = Vec::new();
    let mut last_error: Option<anyhow::Error> = None;
    for path in tui_restart_marker_paths() {
        if let Some(parent) = path.parent()
            && let Err(error) = fs::create_dir_all(parent)
        {
            last_error = Some(error.into());
            continue;
        }
        match fs::write(&path, &contents) {
            Ok(()) => written.push(path),
            Err(error) => last_error = Some(error.into()),
        }
    }
    if written.is_empty() {
        if let Some(error) = last_error {
            return Err(error).context("write TUI restart marker");
        }
    }
    Ok(written)
}

pub(crate) fn load_tui_restart_notice(
    startup_at: DateTime<Utc>,
    running_version: &str,
) -> Option<TuiRestartNotice> {
    newest_tui_restart_notice(startup_at, running_version, tui_restart_marker_paths())
}

pub(crate) fn newest_tui_restart_notice(
    startup_at: DateTime<Utc>,
    running_version: &str,
    marker_paths: Vec<PathBuf>,
) -> Option<TuiRestartNotice> {
    marker_paths
        .into_iter()
        .filter_map(|path| {
            let contents = fs::read_to_string(&path).ok()?;
            let marker: TuiRestartMarker = serde_json::from_str(&contents).ok()?;
            if !restart_marker_applies_to_running_version(&marker.version, running_version) {
                return None;
            }
            let newer_than_tui = marker.marked_at > startup_at;
            let different_version = marker.version.trim() != running_version.trim();
            (newer_than_tui || different_version).then_some(TuiRestartNotice {
                marker_path: path,
                version: marker.version,
                reason: marker.reason,
            })
        })
        .max_by_key(|notice| {
            fs::read_to_string(&notice.marker_path)
                .ok()
                .and_then(|contents| serde_json::from_str::<TuiRestartMarker>(&contents).ok())
                .map(|marker| marker.marked_at)
        })
}

fn restart_marker_applies_to_running_version(marker_version: &str, running_version: &str) -> bool {
    version_profile_suffix(marker_version) == version_profile_suffix(running_version)
}

fn version_profile_suffix(version: &str) -> &'static str {
    if version.trim().ends_with("-dev") {
        "dev"
    } else {
        "prod"
    }
}

async fn wait_for_backend_health(config_path: &Path) -> Result<serde_json::Value> {
    let config = AppConfig::load_from_path(Some(config_path.to_path_buf()))
        .context("reload config after backend startup")?;
    let client = Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .context("build backend startup http client")?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut last_error = None;
    while tokio::time::Instant::now() < deadline {
        match client.get(service_url(&config, "/healthz")).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    return response
                        .json()
                        .await
                        .context("parse backend health response");
                }
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                last_error = Some(anyhow::anyhow!("health endpoint returned {status} {body}"));
            }
            Err(error) => last_error = Some(error.into()),
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("backend health endpoint did not respond")))
}

async fn check_database_connectivity(config: &AppConfig) -> Result<()> {
    PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(3))
        .connect(&config.database.url)
        .await
        .map(drop)
        .context("connect postgres")
}

fn format_backend_health_summary(health: &serde_json::Value) -> String {
    let role = health
        .get("role")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    let status = health
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    let database = health
        .get("database")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    let mut lines = vec![
        "Backend health:".to_string(),
        format!("- role: {role}"),
        format!("- status: {status}"),
        format!("- database: {database}"),
    ];
    if let Some(upstream) = health.get("upstream") {
        lines.push(format!("- upstream: {upstream}"));
    }
    lines.join("\n")
}

fn prompt_yes_no(prompt: &str) -> Result<bool> {
    print!("{prompt} [y/N]: ");
    io::stdout().flush().context("flush prompt")?;
    let mut input = String::new();
    io::stdin().read_line(&mut input).context("read prompt")?;
    Ok(matches!(
        input.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn set_cluster_enabled_in_shared_config(path: &Path, enabled: bool) -> Result<()> {
    let mut content = if path.exists() {
        fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?
    } else {
        String::new()
    };
    let mut lines = content.lines().map(ToOwned::to_owned).collect::<Vec<_>>();
    let mut cluster_header = None;
    let mut enabled_line = None;
    let mut in_cluster = false;

    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_cluster = trimmed == "[cluster]";
            if in_cluster {
                cluster_header = Some(index);
            }
            continue;
        }
        if in_cluster && trimmed.starts_with("enabled = ") {
            enabled_line = Some(index);
            break;
        }
    }

    let enabled_value = format!("enabled = {enabled}");
    if let Some(index) = enabled_line {
        lines[index] = enabled_value;
    } else if let Some(index) = cluster_header {
        lines.insert(index + 1, enabled_value);
    } else {
        if !lines.is_empty() && !lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push("[cluster]".to_string());
        lines.push(enabled_value);
        lines.push("# advertise_addr = \"192.168.1.50:4040\"".to_string());
        lines.push("# discovery_multicast_addr = \"239.255.42.99:4042\"".to_string());
        lines.push("# announce_interval = \"5s\"".to_string());
        lines.push("# peer_ttl = \"15s\"".to_string());
        lines.push("# priority = 100".to_string());
    }

    content = lines.join("\n");
    if !content.ends_with('\n') {
        content.push('\n');
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(path, content).with_context(|| format!("write {}", path.display()))
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
const WATCH_MANAGER_UNIT_NAME: &str = "memory-watch-manager.service";
const WATCH_MANAGER_EVENT_DEBOUNCE_MS: u64 = 500;
const WATCH_MANAGER_FALLBACK_SCAN_SECONDS: u64 = 30;
const WATCH_MANAGER_HEALTH_SCAN_SECONDS: u64 = 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
struct WatcherManagerState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    updated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    mode: String,
    #[serde(default)]
    last_reconcile_reason: String,
    #[serde(default)]
    last_reconcile_duration_ms: u128,
    #[serde(default)]
    event_count: u64,
    #[serde(default)]
    fallback_scan_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lock_owner_pid: Option<u32>,
    #[serde(default)]
    sessions: std::collections::BTreeMap<String, ManagedWatcherSession>,
    #[serde(default)]
    warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ManagedWatcherSession {
    unit_name: String,
    project: String,
    repo_root: String,
    agent_cli: String,
    agent_session_id: String,
    agent_pid: u32,
    agent_started_at: DateTime<Utc>,
}

async fn run_watcher_manager(config: AppConfig, config_path: Option<PathBuf>) -> Result<()> {
    let _lock = WatcherManagerLock::acquire(config.profile)?;
    let version = config.profile.display_version(env!("CARGO_PKG_VERSION"));
    eprintln!(
        "watcher manager v{version} starting (profile={profile}, service={service_addr}, mode=event-driven, fallback={fallback}s)",
        profile = config.profile,
        service_addr = config.service.bind_addr,
        fallback = WATCH_MANAGER_FALLBACK_SCAN_SECONDS,
    );
    if let Some(path) = config.resolved_config_path.as_deref() {
        eprintln!("  config: {}", path.display());
    }
    if let Some(path) = config.resolved_dev_overlay_path.as_deref() {
        eprintln!("  dev overlay: {}", path.display());
    }
    eprintln!(
        "  state: {}",
        watcher_manager_state_path(config.profile)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "unknown".to_string())
    );
    let mut event_rx = match start_watcher_manager_event_source() {
        Ok(rx) => Some(rx),
        Err(error) => {
            eprintln!(
                "watcher manager session file events unavailable; using fallback scans only: {error}"
            );
            None
        }
    };
    reconcile_watcher_manager(&config, config_path.as_deref(), "startup", true, 0, 0).await?;
    let mut debounce: Option<std::pin::Pin<Box<tokio::time::Sleep>>> = None;
    let mut fallback = tokio::time::interval(std::time::Duration::from_secs(
        WATCH_MANAGER_FALLBACK_SCAN_SECONDS,
    ));
    fallback.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    fallback.tick().await;
    let mut health = tokio::time::interval(std::time::Duration::from_secs(
        WATCH_MANAGER_HEALTH_SCAN_SECONDS,
    ));
    health.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    health.tick().await;
    let mut event_count = 0u64;
    let mut fallback_scan_count = 0u64;
    loop {
        tokio::select! {
            Some(_) = async {
                match event_rx.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            }, if event_rx.is_some() => {
                event_count = event_count.saturating_add(1);
                debounce = Some(Box::pin(tokio::time::sleep(std::time::Duration::from_millis(
                    WATCH_MANAGER_EVENT_DEBOUNCE_MS,
                ))));
            }
            _ = async {
                if let Some(delay) = debounce.as_mut() {
                    delay.as_mut().await;
                }
            }, if debounce.is_some() => {
                debounce = None;
                if let Err(error) = reconcile_watcher_manager(
                    &config,
                    config_path.as_deref(),
                    "session-file-event",
                    false,
                    event_count,
                    fallback_scan_count,
                ).await {
                    eprintln!("watcher manager reconcile failed: {error}");
                }
            }
            _ = fallback.tick() => {
                fallback_scan_count = fallback_scan_count.saturating_add(1);
                if let Err(error) = reconcile_watcher_manager(
                    &config,
                    config_path.as_deref(),
                    "fallback-scan",
                    false,
                    event_count,
                    fallback_scan_count,
                ).await {
                    eprintln!("watcher manager reconcile failed: {error}");
                }
            }
            _ = health.tick() => {
                if let Err(error) = reconcile_watcher_manager(
                    &config,
                    config_path.as_deref(),
                    "health-scan",
                    true,
                    event_count,
                    fallback_scan_count,
                ).await {
                    eprintln!("watcher manager reconcile failed: {error}");
                }
            }
        }
    }
}

async fn reconcile_watcher_manager(
    config: &AppConfig,
    config_path: Option<&Path>,
    reason: &str,
    verify_units: bool,
    event_count: u64,
    fallback_scan_count: u64,
) -> Result<()> {
    let started = Instant::now();
    let mut state = load_watcher_manager_state(config.profile)?;
    let previous_state = state.clone();
    state.warnings.clear();

    let sessions = mem_agenttop::collect_lightweight_agent_sessions();
    let mut seen = std::collections::BTreeSet::new();

    for session in sessions {
        let Some(repo_root) = resolve_agent_repo_root(&session.cwd)? else {
            continue;
        };
        if !repo_agent_watch_enabled(&repo_root)? {
            state.warnings.push(format!(
                "Skipped {} session {} in {} because repo opted out of agent-linked watchers.",
                session.agent_cli,
                session.session_id,
                repo_root.display()
            ));
            continue;
        }

        let project = resolve_manager_project_slug(&repo_root);
        ensure_agent_watch_repo_bootstrap(&repo_root, &project)?;

        if legacy_watch_service_is_active(&project) {
            state.warnings.push(format!(
                "Skipped agent-linked watcher for project {} because legacy watcher service {} is active.",
                project,
                legacy_watch_service_name(&project)
            ));
            continue;
        }

        let unit_name = managed_watch_service_name(&session.session_id);
        let tracked = state.sessions.contains_key(&session.session_id);
        let mut unit_loaded = tracked;
        let mut unit_running = tracked;
        if !tracked || verify_units {
            unit_loaded = managed_watch_service_loaded(&session.session_id);
            unit_running = managed_watch_service_running(&session.session_id);
        }
        if should_start_agent_watcher(tracked, unit_loaded, unit_running) {
            if unit_loaded {
                let _ = stop_managed_watch_service(&session.session_id);
            }
            start_managed_agent_watcher(&repo_root, &project, &session, config_path)?;
        }

        let agent_started_at = DateTime::<Utc>::from_timestamp_millis(session.started_at as i64)
            .unwrap_or_else(Utc::now);
        state.sessions.insert(
            session.session_id.clone(),
            ManagedWatcherSession {
                unit_name: unit_name.clone(),
                project,
                repo_root: repo_root.display().to_string(),
                agent_cli: session.agent_cli.to_string(),
                agent_session_id: session.session_id.clone(),
                agent_pid: session.pid,
                agent_started_at,
            },
        );
        seen.insert(session.session_id.clone());
    }

    let stale = state
        .sessions
        .keys()
        .filter(|session_id| !seen.contains(*session_id))
        .cloned()
        .collect::<Vec<_>>();
    for session_id in stale {
        if let Some(entry) = state.sessions.remove(&session_id) {
            let _ = stop_managed_watch_service(&session_id);
            #[cfg(target_os = "macos")]
            if let Ok(path) = managed_watch_launch_agent_path(&session_id) {
                if path.exists() {
                    let _ = fs::remove_file(path);
                }
            }
            let _ = entry;
        }
    }

    state.updated_at = Some(Utc::now());
    state.mode = "event-driven".to_string();
    state.last_reconcile_reason = reason.to_string();
    state.last_reconcile_duration_ms = started.elapsed().as_millis();
    state.event_count = event_count;
    state.fallback_scan_count = fallback_scan_count;
    state.lock_owner_pid = Some(std::process::id());
    save_watcher_manager_state_if_changed(config.profile, &previous_state, &state)?;
    Ok(())
}

fn resolve_agent_repo_root(cwd: &str) -> Result<Option<PathBuf>> {
    // The session cwd may no longer exist (deleted repo, unmounted volume, etc.).
    if !Path::new(cwd).is_dir() {
        return Ok(None);
    }
    let output = ProcessCommand::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output()
        .with_context(|| format!("run git rev-parse in {cwd}"))?;
    if !output.status.success() {
        return Ok(None);
    }
    let repo_root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if repo_root.is_empty() {
        return Ok(None);
    }
    // When the agent runs inside a git worktree (e.g. Claude Code -w), --show-toplevel
    // returns the worktree path, which lacks .mem/project.toml. Resolve through to
    // the main repo root via --git-common-dir so both agents share the same project slug.
    let common_dir_output = ProcessCommand::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(cwd)
        .output();
    if let Ok(common_output) = common_dir_output
        && common_output.status.success()
    {
        let common_dir = String::from_utf8_lossy(&common_output.stdout)
            .trim()
            .to_string();
        if let Some(main_root) = PathBuf::from(&common_dir).parent()
            && main_root.join(".mem").join("project.toml").exists()
        {
            return Ok(Some(main_root.to_path_buf()));
        }
    }
    Ok(Some(PathBuf::from(repo_root)))
}

fn repo_agent_watch_enabled(repo_root: &Path) -> Result<bool> {
    let path = repo_root.join(".agents").join("memory-layer.toml");
    if !path.is_file() {
        return Ok(true);
    }
    let content = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let value: toml::Value = content
        .parse()
        .with_context(|| format!("parse {}", path.display()))?;
    Ok(value
        .get("agent_watch")
        .and_then(|section| section.get("enabled"))
        .and_then(|value| value.as_bool())
        .unwrap_or(true))
}

fn resolve_manager_project_slug(repo_root: &Path) -> String {
    read_repo_project_slug(repo_root)
        .or_else(|| {
            repo_root
                .file_name()
                .and_then(|value| value.to_str())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| "memory".to_string())
}

fn ensure_agent_watch_repo_bootstrap(repo_root: &Path, project: &str) -> Result<()> {
    if !repo_root.join(".mem").is_dir() {
        initialize_repo(repo_root, project, false, false)?;
    } else {
        repair_repo_bootstrap(repo_root, project)?;
    }
    ensure_agent_watch_repo_config(repo_root)
}

fn ensure_agent_watch_repo_config(repo_root: &Path) -> Result<()> {
    let path = repo_root.join(".mem").join("config.toml");
    let content = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let mut value: toml::Value = content
        .parse()
        .with_context(|| format!("parse {}", path.display()))?;
    let root = value
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("{} does not contain a TOML table root", path.display()))?;
    let automation = root
        .entry("automation")
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("{} [automation] is not a table", path.display()))?;
    automation.insert("enabled".to_string(), toml::Value::Boolean(true));
    automation.insert("mode".to_string(), toml::Value::String("auto".to_string()));
    automation.insert(
        "repo_root".to_string(),
        toml::Value::String(repo_root.display().to_string()),
    );
    write_file_if_changed(&path, toml::to_string_pretty(&value)?.as_bytes())?;
    Ok(())
}

fn legacy_watch_service_is_active(project: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        launch_agent_status(&watch_launch_agent_label(project))
            .map(|status| status.running)
            .unwrap_or(false)
    }

    #[cfg(not(target_os = "macos"))]
    {
        unit_is_active(&watch_unit_name(project))
    }
}

fn legacy_watch_service_name(project: &str) -> String {
    #[cfg(target_os = "macos")]
    {
        watch_launch_agent_label(project)
    }

    #[cfg(not(target_os = "macos"))]
    {
        watch_unit_name(project)
    }
}

fn managed_watch_service_name(session_id: &str) -> String {
    #[cfg(target_os = "macos")]
    {
        managed_watch_launch_agent_label(session_id)
    }

    #[cfg(not(target_os = "macos"))]
    {
        format!(
            "memory-watch-codex-{}.service",
            platform::sanitize_service_fragment(session_id)
        )
    }
}

fn managed_watch_service_loaded(session_id: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        launch_agent_status(&managed_watch_launch_agent_label(session_id))
            .map(|status| status.loaded)
            .unwrap_or(false)
    }

    #[cfg(not(target_os = "macos"))]
    {
        unit_is_loaded(&managed_watch_service_name(session_id))
    }
}

fn managed_watch_service_running(session_id: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        launch_agent_status(&managed_watch_launch_agent_label(session_id))
            .map(|status| status.running)
            .unwrap_or(false)
    }

    #[cfg(not(target_os = "macos"))]
    {
        unit_is_active(&managed_watch_service_name(session_id))
    }
}

fn start_managed_agent_watcher(
    repo_root: &Path,
    project: &str,
    session: &LightweightAgentSession,
    config_path: Option<&Path>,
) -> Result<()> {
    let started_at = DateTime::<Utc>::from_timestamp_millis(session.started_at as i64)
        .unwrap_or_else(Utc::now)
        .to_rfc3339();

    #[cfg(target_os = "macos")]
    {
        let plist_path = managed_watch_launch_agent_path(&session.session_id)?;
        let label = managed_watch_launch_agent_label(&session.session_id);
        write_launch_agent(
            &plist_path,
            render_managed_watch_launch_agent(
                repo_root,
                project,
                session,
                &started_at,
                config_path,
            )?,
            &label,
        )?;
        bootstrap_launch_agent(&plist_path, &label)?;
        return Ok(());
    }

    #[cfg(not(target_os = "macos"))]
    let memory_binary = memory_binary_path()?;

    #[cfg(not(target_os = "macos"))]
    let unit_name = managed_watch_service_name(&session.session_id);

    #[cfg(not(target_os = "macos"))]
    let mut cmd = ProcessCommand::new("systemd-run");
    #[cfg(not(target_os = "macos"))]
    cmd.args([
        "--user",
        "--unit",
        &unit_name,
        "--property",
        &format!("WorkingDirectory={}", repo_root.display()),
        "--property",
        "Restart=no",
        "--setenv=MEMORY_LAYER_WATCH_SERVICE_MANAGED=1",
        "--collect",
    ]);
    #[cfg(not(target_os = "macos"))]
    cmd.arg(memory_binary);
    // Prefer the repo-local config so the watcher talks to the same service
    // instance the TUI and CLI use for this project.
    #[cfg(not(target_os = "macos"))]
    {
        let repo_config = repo_root.join(".mem").join("config.toml");
        if repo_config.is_file() {
            cmd.arg("--config").arg(&repo_config);
        } else if let Some(path) = config_path {
            cmd.arg("--config").arg(path);
        }
    }
    #[cfg(not(target_os = "macos"))]
    let output = cmd
        .arg("watcher")
        .arg("run")
        .arg("--project")
        .arg(project)
        .arg("--repo-root")
        .arg(repo_root)
        .arg("--agent-cli")
        .arg(session.agent_cli)
        .arg("--agent-session-id")
        .arg(&session.session_id)
        .arg("--agent-pid")
        .arg(session.pid.to_string())
        .arg("--agent-started-at")
        .arg(started_at)
        .output()
        .with_context(|| format!("run systemd-run for {}", session.session_id))?;
    #[cfg(not(target_os = "macos"))]
    if output.status.success() {
        return Ok(());
    }
    #[cfg(not(target_os = "macos"))]
    if unit_is_loaded(&unit_name) {
        return Ok(());
    }
    #[cfg(not(target_os = "macos"))]
    anyhow::bail!(
        "systemd-run failed for {}: {}",
        unit_name,
        String::from_utf8_lossy(&output.stderr).trim()
    )
}

fn load_watcher_manager_state(profile: Profile) -> Result<WatcherManagerState> {
    let path = watcher_manager_state_path(profile)?;
    if !path.is_file() {
        return Ok(WatcherManagerState::default());
    }
    let content = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&content).with_context(|| format!("parse {}", path.display()))
}

fn save_watcher_manager_state(profile: Profile, state: &WatcherManagerState) -> Result<()> {
    let path = watcher_manager_state_path(profile)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(&path, serde_json::to_vec_pretty(state)?)
        .with_context(|| format!("write {}", path.display()))
}

fn save_watcher_manager_state_if_changed(
    profile: Profile,
    previous: &WatcherManagerState,
    next: &WatcherManagerState,
) -> Result<()> {
    let mut comparable_previous = previous.clone();
    let mut comparable_next = next.clone();
    comparable_previous.updated_at = None;
    comparable_next.updated_at = None;
    comparable_previous.last_reconcile_duration_ms = 0;
    comparable_next.last_reconcile_duration_ms = 0;
    if comparable_previous == comparable_next {
        return Ok(());
    }
    save_watcher_manager_state(profile, next)
}

fn write_file_if_changed(path: &Path, next: &[u8]) -> Result<()> {
    if let Ok(current) = fs::read(path)
        && current == next
    {
        return Ok(());
    }
    fs::write(path, next).with_context(|| format!("write {}", path.display()))
}

fn clear_watcher_manager_state(profile: Profile) -> Result<()> {
    let path = watcher_manager_state_path(profile)?;
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
    }
    Ok(())
}

fn watcher_manager_state_path(profile: Profile) -> Result<PathBuf> {
    let filename = match profile {
        Profile::Dev => "watcher-manager-state-dev.json",
        Profile::Prod => "watcher-manager-state.json",
    };
    Ok(platform::preferred_user_state_dir()
        .ok_or_else(|| anyhow::anyhow!("HOME is not set"))?
        .join(filename))
}

fn watcher_manager_lock_path(profile: Profile) -> Result<PathBuf> {
    Ok(watcher_manager_state_path(profile)?.with_extension("lock"))
}

struct WatcherManagerLock {
    path: PathBuf,
}

impl WatcherManagerLock {
    fn acquire(profile: Profile) -> Result<Self> {
        let path = watcher_manager_lock_path(profile)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        let pid = std::process::id();
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                writeln!(file, "{pid}")?;
                Ok(Self { path })
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let owner = fs::read_to_string(&path)
                    .ok()
                    .and_then(|value| value.trim().parse::<u32>().ok());
                if let Some(owner) = owner
                    && process_is_alive(owner)
                {
                    anyhow::bail!(
                        "watcher manager is already running with pid {owner}; stop it before starting another manager"
                    );
                }
                let _ = fs::remove_file(&path);
                Self::acquire(profile)
            }
            Err(error) => Err(error).with_context(|| format!("create {}", path.display())),
        }
    }
}

impl Drop for WatcherManagerLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn process_is_alive(pid: u32) -> bool {
    ProcessCommand::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn start_watcher_manager_event_source() -> Result<tokio::sync::mpsc::UnboundedReceiver<()>> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
        if result.is_ok() {
            let _ = tx.send(());
        }
    })
    .context("create watcher manager filesystem watcher")?;

    for dir in watcher_manager_session_dirs() {
        if dir.is_dir() {
            notify::Watcher::watch(&mut watcher, &dir, notify::RecursiveMode::Recursive)
                .with_context(|| format!("watch {}", dir.display()))?;
        }
    }

    std::mem::forget(watcher);
    Ok(rx)
}

fn watcher_manager_session_dirs() -> Vec<PathBuf> {
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let mut dirs = vec![home.join(".codex").join("sessions")];
    let claude_base = env::var("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home.join(".claude"));
    dirs.push(claude_base.join("sessions"));
    dirs
}

fn enable_watch_manager_service(config_path: &Path) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        let plist_path = watch_manager_launch_agent_path()?;
        let label = watch_manager_launch_agent_label();
        write_launch_agent(
            &plist_path,
            render_watch_manager_launch_agent(config_path)?,
            label,
        )?;
        bootstrap_launch_agent(&plist_path, label)?;
        return Ok(format!(
            "Installed and started watcher manager LaunchAgent {}.\nPlist: {}\nConfig: {}\n\nManage it with:\n- memory watcher manager status\n- memory watcher manager disable\n- launchctl kickstart -k {}/{}",
            label,
            plist_path.display(),
            config_path.display(),
            launchctl_domain_target()?,
            label
        ));
    }

    #[cfg(not(target_os = "macos"))]
    {
        let unit_dir = user_systemd_unit_dir()?;
        let unit_path = unit_dir.join(WATCH_MANAGER_UNIT_NAME);
        fs::create_dir_all(&unit_dir).with_context(|| format!("create {}", unit_dir.display()))?;
        fs::write(&unit_path, render_watch_manager_unit(config_path)?)
            .with_context(|| format!("write {}", unit_path.display()))?;
        run_systemctl_user(["daemon-reload"])?;
        run_systemctl_user(["enable", "--now", WATCH_MANAGER_UNIT_NAME])?;
        Ok(format!(
            "Installed and started user service {}.\nUnit: {}\nConfig: {}\n\nManage it with:\n- memory watcher manager status\n- memory watcher manager disable\n- systemctl --user restart {}",
            WATCH_MANAGER_UNIT_NAME,
            unit_path.display(),
            config_path.display(),
            WATCH_MANAGER_UNIT_NAME
        ))
    }
}

fn preview_enable_watch_manager_service() -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        return Ok(format!(
            "Dry run: would install and start watcher manager LaunchAgent {}.\nPlist: {}",
            watch_manager_launch_agent_label(),
            watch_manager_launch_agent_path()?.display(),
        ));
    }

    #[cfg(not(target_os = "macos"))]
    {
        let unit_path = user_systemd_unit_dir()?.join(WATCH_MANAGER_UNIT_NAME);
        Ok(format!(
            "Dry run: would install and start user service {}.\nUnit: {}",
            WATCH_MANAGER_UNIT_NAME,
            unit_path.display()
        ))
    }
}

fn disable_watch_manager_service(profile: Profile) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        let plist_path = watch_manager_launch_agent_path()?;
        let label = watch_manager_launch_agent_label();
        let _ = bootout_launch_agent(&plist_path, label);
        if plist_path.exists() {
            fs::remove_file(&plist_path)
                .with_context(|| format!("remove {}", plist_path.display()))?;
        }
        if let Ok(state) = load_watcher_manager_state(profile) {
            for session_id in state.sessions.keys() {
                let _ = stop_managed_watch_service(session_id);
                if let Ok(path) = managed_watch_launch_agent_path(session_id) {
                    if path.exists() {
                        let _ = fs::remove_file(path);
                    }
                }
            }
        }
        clear_watcher_manager_state(profile)?;
        return Ok(format!(
            "Disabled watcher manager LaunchAgent {}.\nRemoved plist: {}",
            label,
            plist_path.display()
        ));
    }

    #[cfg(not(target_os = "macos"))]
    {
        let unit_path = user_systemd_unit_dir()?.join(WATCH_MANAGER_UNIT_NAME);
        let _ = run_systemctl_user(["disable", "--now", WATCH_MANAGER_UNIT_NAME]);
        if unit_path.exists() {
            fs::remove_file(&unit_path)
                .with_context(|| format!("remove {}", unit_path.display()))?;
        }
        if let Ok(state) = load_watcher_manager_state(profile) {
            for entry in state.sessions.values() {
                let _ = stop_unit_if_present(&entry.unit_name);
            }
        }
        clear_watcher_manager_state(profile)?;
        run_systemctl_user(["daemon-reload"])?;
        Ok(format!(
            "Disabled user service {}.\nRemoved unit: {}",
            WATCH_MANAGER_UNIT_NAME,
            unit_path.display()
        ))
    }
}

fn preview_disable_watch_manager_service() -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        return Ok(format!(
            "Dry run: would disable watcher manager LaunchAgent {} and remove {}",
            watch_manager_launch_agent_label(),
            watch_manager_launch_agent_path()?.display(),
        ));
    }

    #[cfg(not(target_os = "macos"))]
    {
        let unit_path = user_systemd_unit_dir()?.join(WATCH_MANAGER_UNIT_NAME);
        Ok(format!(
            "Dry run: would disable user service {} and remove {}",
            WATCH_MANAGER_UNIT_NAME,
            unit_path.display()
        ))
    }
}

fn watch_manager_service_status(profile: Profile) -> Result<String> {
    let state = load_watcher_manager_state(profile).unwrap_or_default();
    let warning_lines = if state.warnings.is_empty() {
        "- warnings: none".to_string()
    } else {
        format!("- warnings: {}", state.warnings.join(" | "))
    };
    let runtime_lines = format!(
        "- mode: {}\n- last reconcile reason: {}\n- last reconcile duration: {} ms\n- event count: {}\n- fallback scans: {}\n- lock owner pid: {}",
        if state.mode.is_empty() {
            "unknown"
        } else {
            state.mode.as_str()
        },
        if state.last_reconcile_reason.is_empty() {
            "n/a"
        } else {
            state.last_reconcile_reason.as_str()
        },
        state.last_reconcile_duration_ms,
        state.event_count,
        state.fallback_scan_count,
        state
            .lock_owner_pid
            .map(|pid| pid.to_string())
            .unwrap_or_else(|| "n/a".to_string())
    );

    #[cfg(target_os = "macos")]
    {
        let plist_path = watch_manager_launch_agent_path()?;
        let label = watch_manager_launch_agent_label();
        let status = launch_agent_status(label)?;
        return Ok(format!(
            "Watcher manager service:\n- label: {}\n- plist: {}\n- installed: {}\n- loaded: {}\n- running: {}\n- tracked sessions: {}\n- last reconcile: {}\n{}\n{}\n\nInspect with:\n- launchctl print {}/{}\n- memory watcher manager status",
            label,
            plist_path.display(),
            yes_no(plist_path.exists()),
            yes_no(status.loaded),
            yes_no(status.running),
            state.sessions.len(),
            state
                .updated_at
                .map(|value| value.to_rfc3339())
                .unwrap_or_else(|| "n/a".to_string()),
            runtime_lines,
            warning_lines,
            launchctl_domain_target()?,
            label
        ));
    }

    #[cfg(not(target_os = "macos"))]
    {
        let unit_path = user_systemd_unit_dir()?.join(WATCH_MANAGER_UNIT_NAME);
        let is_enabled = run_systemctl_user(["is-enabled", WATCH_MANAGER_UNIT_NAME]).is_ok();
        let is_active = run_systemctl_user(["is-active", WATCH_MANAGER_UNIT_NAME]).is_ok();
        Ok(format!(
            "Watcher manager service:\n- unit: {}\n- installed: {}\n- enabled: {}\n- active: {}\n- tracked sessions: {}\n- last reconcile: {}\n{}\n{}\n\nInspect with:\n- systemctl --user status {}\n- memory watcher manager status",
            unit_path.display(),
            yes_no(unit_path.exists()),
            yes_no(is_enabled),
            yes_no(is_active),
            state.sessions.len(),
            state
                .updated_at
                .map(|value| value.to_rfc3339())
                .unwrap_or_else(|| "n/a".to_string()),
            runtime_lines,
            warning_lines,
            WATCH_MANAGER_UNIT_NAME
        ))
    }
}

#[cfg(not(target_os = "macos"))]
fn render_watch_manager_unit(config_path: &Path) -> Result<String> {
    let memory_binary = memory_binary_path()?;
    let home = env::var("HOME").unwrap_or_else(|_| "/".to_string());
    Ok(format!(
        "[Unit]\nDescription=Memory Layer Watcher Manager\nAfter=default.target\n\n[Service]\nType=simple\nWorkingDirectory={}\nExecStart={} --config {} watcher manager run\nRestart=always\nRestartSec=2\n\n[Install]\nWantedBy=default.target\n",
        shell_escape_str(&home),
        shell_escape_path(&memory_binary),
        shell_escape_path(config_path),
    ))
}

#[cfg(target_os = "macos")]
fn render_watch_manager_launch_agent(config_path: &Path) -> Result<String> {
    let binary = memory_binary_path()?;
    let working_directory =
        macos_app_support_dir().ok_or_else(|| anyhow::anyhow!("HOME is not set"))?;
    let log_dir = user_memory_layer_log_dir()?;
    let stdout_path = log_dir.join("memory-watch-manager.stdout.log");
    let stderr_path = log_dir.join("memory-watch-manager.stderr.log");
    let command = launch_agent_shell_command(&[
        binary.display().to_string(),
        "--config".to_string(),
        config_path.display().to_string(),
        "watcher".to_string(),
        "manager".to_string(),
        "run".to_string(),
    ])?;
    render_launch_agent_plist(
        watch_manager_launch_agent_label(),
        &working_directory,
        &command,
        &stdout_path,
        &stderr_path,
    )
}

#[cfg(not(target_os = "macos"))]
fn unit_is_active(unit_name: &str) -> bool {
    run_systemctl_user(["is-active", unit_name]).is_ok()
}

#[cfg(not(target_os = "macos"))]
fn unit_is_loaded(unit_name: &str) -> bool {
    let output = ProcessCommand::new("systemctl")
        .args([
            "--user",
            "show",
            unit_name,
            "--property",
            "LoadState",
            "--value",
        ])
        .output();
    let Ok(output) = output else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let load_state = String::from_utf8_lossy(&output.stdout).trim().to_string();
    !load_state.is_empty() && load_state != "not-found"
}

fn should_start_agent_watcher(session_tracked: bool, unit_loaded: bool, unit_active: bool) -> bool {
    !session_tracked || !unit_loaded || !unit_active
}

fn stop_managed_watch_service(session_id: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let label = managed_watch_launch_agent_label(session_id);
        let path = managed_watch_launch_agent_path(session_id)?;
        let _ = bootout_launch_agent(&path, &label);
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        let unit_name = managed_watch_service_name(session_id);
        stop_unit_if_present(&unit_name)
    }
}

#[cfg(not(target_os = "macos"))]
fn stop_unit_if_present(unit_name: &str) -> Result<()> {
    if unit_is_loaded(unit_name) {
        let _ = run_systemctl_user(["stop", unit_name]);
        let _ = run_systemctl_user(["reset-failed", unit_name]);
    }
    Ok(())
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
fn watch_launch_agent_path(project: &str) -> Result<PathBuf> {
    platform::watch_launch_agent_path(project).ok_or_else(|| anyhow::anyhow!("HOME is not set"))
}

#[cfg(target_os = "macos")]
fn watch_manager_launch_agent_label() -> &'static str {
    platform::watch_manager_launch_agent_label()
}

#[cfg(target_os = "macos")]
fn watch_manager_launch_agent_path() -> Result<PathBuf> {
    platform::watch_manager_launch_agent_path().ok_or_else(|| anyhow::anyhow!("HOME is not set"))
}

#[cfg(target_os = "macos")]
fn managed_watch_launch_agent_label(session_id: &str) -> String {
    platform::managed_watch_launch_agent_label(session_id)
}

#[cfg(target_os = "macos")]
fn managed_watch_launch_agent_path(session_id: &str) -> Result<PathBuf> {
    platform::managed_watch_launch_agent_path(session_id)
        .ok_or_else(|| anyhow::anyhow!("HOME is not set"))
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
fn render_managed_watch_launch_agent(
    repo_root: &Path,
    project: &str,
    session: &AgentSession,
    started_at: &str,
    config_path: Option<&Path>,
) -> Result<String> {
    let binary = memory_binary_path()?;
    let working_directory = repo_root
        .canonicalize()
        .with_context(|| format!("canonicalize {}", repo_root.display()))?;
    let log_dir = user_memory_layer_log_dir()?;
    let sanitized = sanitize_service_fragment(&session.session_id);
    let stdout_path = log_dir.join(format!("memory-watch-codex-{sanitized}.stdout.log"));
    let stderr_path = log_dir.join(format!("memory-watch-codex-{sanitized}.stderr.log"));
    let mut args = vec![binary.display().to_string()];
    // Prefer the repo-local config so the watcher talks to the same service
    // instance the TUI and CLI use for this project.
    let repo_config = repo_root.join(".mem").join("config.toml");
    if repo_config.is_file() {
        args.push("--config".to_string());
        args.push(repo_config.display().to_string());
    } else if let Some(path) = config_path {
        args.push("--config".to_string());
        args.push(path.display().to_string());
    }
    args.extend([
        "watcher".to_string(),
        "run".to_string(),
        "--project".to_string(),
        project.to_string(),
        "--repo-root".to_string(),
        repo_root.display().to_string(),
        "--agent-cli".to_string(),
        session.agent_cli.to_string(),
        "--agent-session-id".to_string(),
        session.session_id.clone(),
        "--agent-pid".to_string(),
        session.pid.to_string(),
        "--agent-started-at".to_string(),
        started_at.to_string(),
    ]);
    let command = launch_agent_shell_command(&args)?;
    let command = format!("export MEMORY_LAYER_WATCH_SERVICE_MANAGED=1; {command}");
    render_launch_agent_plist(
        &managed_watch_launch_agent_label(&session.session_id),
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
fn run_systemctl_user_for<const N: usize>(
    username: &str,
    runtime_dir: &Path,
    args: [&str; N],
) -> Result<()> {
    let output = ProcessCommand::new("runuser")
        .args(["-u", username, "--", "env"])
        .arg(format!("XDG_RUNTIME_DIR={}", runtime_dir.display()))
        .arg("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .with_context(|| format!("run systemctl --user {} for {}", args.join(" "), username))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    anyhow::bail!(
        "systemctl --user {} for {} failed: {}{}{}",
        args.join(" "),
        username,
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

fn watcher_command_requires_config_load(command: &WatcherCommand) -> bool {
    matches!(
        command,
        WatcherCommand::Run(_)
            | WatcherCommand::Manager(WatcherManagerArgs {
                command: WatcherManagerCommand::Run
            })
    )
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
    if let Some(path) = platform::current_exe_share_subdir("skill-template") {
        candidates.push(path);
    }
    if let Ok(data_home) = env::var("XDG_DATA_HOME") {
        candidates.push(
            PathBuf::from(data_home)
                .join("memory-layer")
                .join("skill-template"),
        );
    }
    if let Some(state_dir) = platform::preferred_user_state_dir() {
        candidates.push(state_dir.join("skill-template"));
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
file_events = true
poll_interval = "60s"
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

const CLAUDE_MD_MEMORY_MARKER: &str = "## Memory Layer workflows";

fn render_claude_md_memory_section(project: &str) -> String {
    format!(
        r#"## Memory Layer workflows

This project uses Memory Layer to persist durable project knowledge. The `memory` CLI
must be on PATH (or use `cargo run --bin memory --` from the repo root).

### Shared invariants
1. Query memory before answering project-specific questions.
2. Use `resume` instead of a generic query for interruption-recovery prompts.
3. Save the approved plan before implementation begins when a planning phase turns into execution.
4. Verify plan-backed work is complete before claiming the task is finished.
5. Remember meaningful work after it is actually done.
6. Remember distilled code and codebase explanations after answering explanation requests.
7. Prefer insufficient evidence over unsupported conclusions.
8. Never invent provenance.

### Query and resume
Use when: the user asks a project-specific question or returns after an interruption.

```bash
memory query --project {project} --question "<question>"
memory resume --project {project}
```

### Plan execution
Use when: a planning session ends and the user approves execution.

Save checkpoint and plan at execution start:
```bash
memory checkpoint start-execution --project {project} --plan-file /tmp/approved-plan.md
```

Verify all plan items are complete before claiming finished:
```bash
memory checkpoint finish-execution --project {project}
```

### Remember completed work (mandatory post-task rule)
**After any meaningful repository work, run the remember workflow before sending the
final response** unless one of these is true:
- no durable knowledge was produced
- the work was purely trivial
- the user explicitly asked not to store memory

```bash
memory remember --project {project} \
  --title "<task title>" \
  --summary "<what changed>" \
  --note "<durable fact 1>" \
  --note "<durable fact 2>" \
  --file-changed "<path>"
```

This should default to storing durable project knowledge, not waiting for the user to ask.

### Store code explanations
Use when: you answered a request to explain code, a file, a module, an architecture path, or the whole codebase.

After answering, store a distilled reusable memory when the explanation is durable and grounded in inspected code or existing memory. Do not store the full chat answer, speculative claims, duplicates, or trivial explanations. Do not use `--file-changed` unless files actually changed.

```bash
memory remember --project {project} --type project \
  --title "Explained <file/module/codebase>" \
  --prompt "<user explanation request>" \
  --summary "<short explanation summary>" \
  --note "<stable explanation fact with file/module/symbol provenance>"
```

### Store user context
Use when: you learn about the user's role, preferences, or expertise.

```bash
memory remember --project {project} --type user --note "<what you learned>"
```

### Store feedback
Use when: the user corrects your approach or confirms a non-obvious choice.

```bash
memory remember --project {project} --type feedback \
  --note "<rule or validated approach>" \
  --note "<why: reason or context>"
```

### Store project context
Use when: you learn about goals, deadlines, or ongoing initiatives.

```bash
memory remember --project {project} --type project \
  --note "<fact or decision>" \
  --note "<why: motivation or constraint>"
```

### Store external reference
Use when: you learn about resources tracked in external systems.

```bash
memory remember --project {project} --type reference \
  --note "<what the resource is and where to find it>"
```
"#
    )
}

fn ensure_claude_md_memory_section(repo_root: &Path, project: &str) -> Result<()> {
    let path = repo_root.join("CLAUDE.md");
    let content = if path.exists() {
        fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?
    } else {
        String::new()
    };
    if content.contains(CLAUDE_MD_MEMORY_MARKER) {
        return Ok(());
    }
    let section = render_claude_md_memory_section(project);
    let updated = if content.is_empty() {
        format!("# Project Instructions\n\n{section}")
    } else {
        format!("{}\n\n{}", content.trim_end(), section)
    };
    fs::write(&path, updated).with_context(|| format!("write {}", path.display()))?;
    Ok(())
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
    let watcher_step = if cfg!(target_os = "macos") {
        "7. Optional: enable the Codex-linked watcher manager:\n   memory watcher manager enable\n   Legacy per-repo watcher service: memory watcher enable --project ".to_string()
            + project
    } else {
        "7. Optional: enable the Linux Codex-linked watcher manager:\n   memory watcher manager enable\n   Legacy per-repo watcher service: memory watcher enable --project ".to_string() + project
    };
    format!(
        "{action} repo-local memory bootstrap for project `{project}` at {}.\n\nFiles:\n- {}\n- {}\n- {}\n- {}/runtime/\n- {} (bundled memory skills)\n\nNext steps:\n1. Set shared values like `database.url`, `service.api_token`, and `[llm]` config in {}\n2. Use {} for repo-specific runtime overrides\n3. Use {} to customize project memory behavior\n4. Start the shared backend if it is not already running:\n   memory service run --config {}\n5. Optional: configure repo-local [service] overrides if you want a parallel dev backend for this repo\n6. Optional: run a project scan:\n   memory scan --project {}\n{}\n8. Open the TUI:\n   memory tui --project {}\n9. Use the repo-local memory skill bundle from {} (umbrella skill at {}/memory-layer)",
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
        watcher_step,
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

    if let Ok(output) = output
        && output.status.success()
    {
        let stdout = String::from_utf8(output.stdout).context("decode git rev-parse output")?;
        let root = stdout.trim();
        if !root.is_empty() {
            return Ok(PathBuf::from(root));
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

    pub(crate) async fn project_activities(
        &self,
        project: &str,
        limit: usize,
        kind: Option<&str>,
    ) -> Result<ActivityListResponse> {
        let mut path = format!("/v1/projects/{project}/activities?limit={limit}");
        if let Some(kind) = kind {
            path.push_str("&kind=");
            path.push_str(kind);
        }
        get_json(
            self.client
                .get(service_url(&self.config, &path))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn up_to_speed(
        &self,
        request: &UpToSpeedRequest,
    ) -> Result<UpToSpeedResponse> {
        get_json(
            self.client
                .post(service_url(
                    &self.config,
                    &format!("/v1/projects/{}/up-to-speed", request.project),
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

    pub(crate) async fn log_graph_activity(&self, request: &GraphActivityRequest) -> Result<()> {
        let response = self
            .client
            .post(service_url(&self.config, "/v1/graph/activity"))
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

    pub(crate) async fn list_embedding_backends(
        &self,
        project: Option<&str>,
    ) -> Result<mem_api::EmbeddingBackendsResponse> {
        let mut request = self
            .client
            .get(service_url(&self.config, "/v1/embeddings/backends"));
        if let Some(slug) = project {
            request = request.query(&[("project", slug)]);
        }
        get_json(request.send().await?).await
    }

    pub(crate) async fn activate_embedding_backend(
        &self,
        name: &str,
    ) -> Result<mem_api::EmbeddingBackendsResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/embeddings/activate"))
                .headers(write_headers(&self.config)?)
                .json(&mem_api::ActivateEmbeddingBackendRequest {
                    name: name.to_string(),
                })
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn deactivate_embedding_backend(
        &self,
    ) -> Result<mem_api::EmbeddingBackendsResponse> {
        let response = self
            .client
            .post(service_url(&self.config, "/v1/embeddings/deactivate"))
            .headers(write_headers(&self.config)?)
            .json(&mem_api::DeactivateEmbeddingBackendRequest::default())
            .send()
            .await?;
        if response.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED {
            anyhow::bail!(
                "service does not support turning embeddings off yet; restart or upgrade memory-service so /v1/embeddings/deactivate is available"
            );
        }
        get_json(response).await
    }

    pub(crate) async fn set_embedding_creation_enabled(
        &self,
        name: &str,
        enabled: bool,
    ) -> Result<mem_api::EmbeddingBackendsResponse> {
        let response = self
            .client
            .post(service_url(&self.config, "/v1/embeddings/create-enabled"))
            .headers(write_headers(&self.config)?)
            .json(&mem_api::SetEmbeddingCreationRequest {
                name: name.to_string(),
                enabled,
            })
            .send()
            .await?;
        if response.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED {
            anyhow::bail!(
                "service does not support toggling automatic embedding creation yet; restart or upgrade memory-service so /v1/embeddings/create-enabled is available"
            );
        }
        get_json(response).await
    }

    pub(crate) async fn memory_history(
        &self,
        memory_id: &str,
    ) -> Result<mem_api::MemoryHistoryResponse> {
        get_json(
            self.client
                .get(service_url(
                    &self.config,
                    &format!("/v1/memory/{memory_id}/history"),
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
                    raw_capture_id: None,
                    dry_run,
                })
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn curate_capture(
        &self,
        project: &str,
        raw_capture_id: Uuid,
        replacement_policy: ReplacementPolicy,
        dry_run: bool,
    ) -> Result<CurateResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/curate"))
                .headers(write_headers(&self.config)?)
                .json(&CurateRequest {
                    project: project.to_string(),
                    batch_size: Some(1),
                    raw_capture_id: Some(raw_capture_id),
                    replacement_policy: Some(replacement_policy),
                    dry_run,
                })
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn reindex(
        &self,
        project: &str,
        dry_run: bool,
        backend: Option<&str>,
    ) -> Result<ReindexResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/reindex"))
                .headers(write_headers(&self.config)?)
                .json(&ReindexRequest {
                    project: project.to_string(),
                    dry_run,
                    backend: backend.map(str::to_string),
                })
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn reembed(
        &self,
        project: &str,
        dry_run: bool,
        backend: Option<&str>,
    ) -> Result<ReembedResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/reembed"))
                .headers(write_headers(&self.config)?)
                .json(&ReembedRequest {
                    project: project.to_string(),
                    dry_run,
                    backend: backend.map(str::to_string),
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

fn print_embedding_backends(payload: &mem_api::EmbeddingBackendsResponse) {
    if payload.backends.is_empty() {
        println!("No embedding backends configured.");
        return;
    }
    let active = payload.active.as_deref();
    let name_width = payload
        .backends
        .iter()
        .map(|b| b.name.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let provider_width = payload
        .backends
        .iter()
        .map(|b| b.provider.len())
        .max()
        .unwrap_or(8)
        .max(8);
    println!(
        "  {:name_width$}  {:provider_width$}  CREATE  MODEL",
        "NAME",
        "PROVIDER",
        name_width = name_width,
        provider_width = provider_width
    );
    for backend in &payload.backends {
        let marker = if Some(backend.name.as_str()) == active {
            "*"
        } else if !backend.ready {
            "!"
        } else {
            " "
        };
        println!(
            "{marker} {:name_width$}  {:provider_width$}  {:7} {}",
            backend.name,
            backend.provider,
            if backend.create_enabled { "on" } else { "off" },
            backend.model,
            name_width = name_width,
            provider_width = provider_width
        );
    }
    println!();
    if let Some(name) = active {
        println!("Active: {name}");
    } else {
        println!("Active: (none) — run `memory embeddings activate <name>` to pick one.");
    }
    let not_ready: Vec<&str> = payload
        .backends
        .iter()
        .filter(|b| !b.ready)
        .map(|b| b.name.as_str())
        .collect();
    if !not_ready.is_empty() {
        println!(
            "Not ready ({} — missing API key or model): {}",
            not_ready.len(),
            not_ready.join(", ")
        );
    }
}

fn print_memory_history(payload: &mem_api::MemoryHistoryResponse) {
    println!(
        "Canonical {} in project {} — {} version(s)",
        payload.canonical_id,
        payload.project,
        payload.versions.len()
    );
    for version in &payload.versions {
        let marker = if version.is_tombstone {
            " [tombstone]"
        } else {
            ""
        };
        let status_label = match version.status {
            mem_api::MemoryStatus::Active => "active",
            mem_api::MemoryStatus::Archived => "archived",
        };
        println!(
            "\nv{} — {} ({}){}\n  id: {}\n  updated: {}",
            version.version_no,
            version.memory_type,
            status_label,
            marker,
            version.id,
            version.updated_at.to_rfc3339(),
        );
        if version.is_tombstone {
            println!("  (empty — memory was deleted at this point)");
        } else {
            println!("  summary: {}", version.summary);
            let preview: String = version.canonical_text.chars().take(240).collect();
            let ellipsis = if version.canonical_text.chars().count() > 240 {
                "..."
            } else {
                ""
            };
            println!("  text: {preview}{ellipsis}");
        }
    }
}

fn print_query_response(payload: QueryResponse) {
    println!("Answer:\n{}\n", payload.answer);
    println!(
        "Confidence: {:.2} | Evidence: {} | Method: {} | Citations: {}\n",
        payload.confidence,
        if payload.insufficient_evidence {
            "insufficient"
        } else {
            "sufficient"
        },
        payload.answer_generation.method,
        format_query_citations(&payload.answer_generation.cited_result_numbers)
    );
    if let Some(reason) = &payload.answer_generation.fallback_reason {
        println!("Fallback: {reason}\n");
    }
    println!(
        "Diagnostics: lexical {} ({} ms) | semantic {} ({} ms) | graph {} [{}] ({} ms) | merged {} | returned {} | rerank {} ms | total {} ms\n",
        payload.diagnostics.lexical_candidates,
        payload.diagnostics.lexical_duration_ms,
        payload.diagnostics.semantic_candidates,
        payload.diagnostics.semantic_duration_ms,
        payload.diagnostics.graph_candidates,
        payload.diagnostics.graph_status,
        payload.diagnostics.graph_duration_ms,
        payload.diagnostics.merged_candidates,
        payload.diagnostics.returned_results,
        payload.diagnostics.rerank_duration_ms,
        payload.diagnostics.total_duration_ms,
    );
    if !payload.answer_citations.is_empty() {
        println!("Cited memories:");
        for citation in &payload.answer_citations {
            println!(
                "{}. {} [{}] {}",
                citation.result_number, citation.summary, citation.memory_type, citation.snippet
            );
        }
        println!();
    }
    for (index, result) in payload.results.into_iter().enumerate() {
        println!(
            "{}. {} [{} / {}] score={:.2}",
            index + 1,
            result.summary,
            result.memory_type,
            result.match_kind,
            result.score
        );
        println!("  {}", result.snippet);
        println!(
            "  debug: chunk {:.2} | entry {:.2} | semantic {:.2} | relation {:.2} | graph {:.2}",
            result.debug.chunk_fts,
            result.debug.entry_fts,
            result.debug.semantic_similarity,
            result.debug.relation_boost,
            result.debug.graph_boost,
        );
        if !result.score_explanation.is_empty() {
            println!("  why: {}", result.score_explanation.join(" | "));
        }
        for connection in &result.graph_connections {
            let symbol = connection
                .symbol
                .as_deref()
                .map(|value| format!(" symbol={value}"))
                .unwrap_or_default();
            let edge = connection
                .edge_kind
                .as_deref()
                .map(|value| format!(" edge={value}"))
                .unwrap_or_default();
            let neighbor = connection
                .neighbor_symbol
                .as_deref()
                .map(|value| format!(" neighbor={value}"))
                .unwrap_or_default();
            println!(
                "  graph: {} {}{}{}{} boost={:.2}",
                connection.reason,
                connection.file_path,
                symbol,
                edge,
                neighbor,
                connection.score_boost
            );
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

fn format_query_citations(numbers: &[usize]) -> String {
    if numbers.is_empty() {
        "none".to_string()
    } else {
        numbers
            .iter()
            .map(|number| format!("[{number}]"))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

async fn handle_eval_command(args: EvalArgs, cwd: &Path, api: &ApiClient) -> Result<()> {
    match args.command {
        EvalCommand::Doctor(args) => {
            let suite = mem_eval::load_suite(&args.suite)?;
            let mut checks = Vec::new();
            checks.push(serde_json::json!({
                "name": "suite.load",
                "status": "ok",
                "message": format!("Loaded {} item(s).", suite.items.len()),
            }));
            let checksum = mem_eval::suite_checksum(&suite)?;
            checks.push(serde_json::json!({
                "name": "suite.checksum",
                "status": "ok",
                "message": checksum,
            }));
            let reviewed = suite.manifest.label_status.as_deref() == Some("reviewed");
            checks.push(serde_json::json!({
                "name": "suite.labels",
                "status": if reviewed { "ok" } else { "warn" },
                "message": suite.manifest.label_status.as_deref().unwrap_or("unreviewed"),
            }));
            if let Some(min_items) = suite.manifest.min_items {
                checks.push(serde_json::json!({
                    "name": "suite.min_items",
                    "status": if suite.items.len() >= min_items { "ok" } else { "fail" },
                    "message": format!("{} item(s), required {}", suite.items.len(), min_items),
                }));
            }
            for item in &suite.items {
                match item {
                    mem_eval::EvalItem::AgentBuildTask(item) => {
                        let result = validate_agent_build_suite_item(&suite, item);
                        checks.push(serde_json::json!({
                            "name": format!("agent_build_task.{}", item.id),
                            "status": if result.is_ok() { "ok" } else { "fail" },
                            "message": result.err().map(|error| error.to_string()).unwrap_or_else(|| "fixture and paths are valid".to_string()),
                        }));
                    }
                    mem_eval::EvalItem::AgentBuildSequence(item) => {
                        let result = validate_agent_build_sequence_suite_item(&suite, item);
                        checks.push(serde_json::json!({
                            "name": format!("agent_build_sequence.{}", item.id),
                            "status": if result.is_ok() { "ok" } else { "fail" },
                            "message": result.err().map(|error| error.to_string()).unwrap_or_else(|| format!("fixture, paths, and {} steps are valid", item.steps.len())),
                        }));
                    }
                    _ => {}
                }
            }
            match api.health().await {
                Ok(value) => checks.push(serde_json::json!({
                    "name": "backend.health",
                    "status": "ok",
                    "message": value,
                })),
                Err(error) => checks.push(serde_json::json!({
                    "name": "backend.health",
                    "status": "fail",
                    "message": error.to_string(),
                })),
            }
            let failed = checks
                .iter()
                .any(|check| check.get("status").and_then(|value| value.as_str()) == Some("fail"));
            let payload = serde_json::json!({
                "ok": !failed,
                "suite": suite.manifest.name,
                "checks": checks,
            });
            if args.text {
                println!(
                    "{}: {}",
                    payload["suite"].as_str().unwrap_or("suite"),
                    if failed { "fail" } else { "ok" }
                );
                for check in payload["checks"].as_array().into_iter().flatten() {
                    println!(
                        "{} [{}] {}",
                        check["name"].as_str().unwrap_or("?"),
                        check["status"].as_str().unwrap_or("?"),
                        check["message"]
                    );
                }
            } else {
                println!("{}", serde_json::to_string_pretty(&payload)?);
            }
            if failed {
                anyhow::bail!("eval doctor failed");
            }
        }
        EvalCommand::Scaffold(args) => {
            let project = resolve_project_slug(args.project, cwd)?;
            let response = api.project_memories(&project).await?;
            let selected = response
                .items
                .into_iter()
                .take(args.limit.clamp(1, 100))
                .collect::<Vec<_>>();
            let manifest = format!(
                "name = \"{} starter eval\"\nproject = \"{}\"\nitems = \"items.jsonl\"\n",
                project, project
            );
            let mut lines = Vec::new();
            for item in selected {
                lines.push(serde_json::json!({
                    "eval_type": "retrieval_qa",
                    "id": format!("memory-{}", item.id),
                    "project": project,
                    "question": format!("What should an agent know about {}?", item.summary),
                    "top_k": 8,
                    "expected_memory_ids": [item.id],
                    "expected_tags": item.tags,
                }));
            }
            if args.dry_run {
                let payload = serde_json::json!({
                    "dry_run": true,
                    "out": args.out,
                    "suite_toml": manifest,
                    "items": lines,
                });
                if args.text {
                    println!(
                        "Would write starter eval suite with {} item(s) to {}",
                        lines.len(),
                        payload["out"].as_str().unwrap_or("<path>")
                    );
                } else {
                    println!("{}", serde_json::to_string_pretty(&payload)?);
                }
                return Ok(());
            }
            fs::create_dir_all(&args.out)
                .with_context(|| format!("create {}", args.out.display()))?;
            fs::write(args.out.join("suite.toml"), manifest)
                .with_context(|| format!("write {}", args.out.join("suite.toml").display()))?;
            let jsonl = lines
                .iter()
                .map(serde_json::to_string)
                .collect::<Result<Vec<_>, _>>()?
                .join("\n");
            fs::write(args.out.join("items.jsonl"), format!("{jsonl}\n"))
                .with_context(|| format!("write {}", args.out.join("items.jsonl").display()))?;
            let payload = serde_json::json!({
                "dry_run": false,
                "out": args.out,
                "items": lines.len(),
            });
            if args.text {
                println!(
                    "Wrote starter eval suite with {} item(s) to {}",
                    payload["items"].as_u64().unwrap_or_default(),
                    payload["out"].as_str().unwrap_or("<path>")
                );
            } else {
                println!("{}", serde_json::to_string_pretty(&payload)?);
            }
        }
        EvalCommand::Run(args) => {
            let suite = mem_eval::load_suite(&args.suite)?;
            if args.fail_on_unreviewed_labels
                && suite.manifest.label_status.as_deref() != Some("reviewed")
            {
                anyhow::bail!(
                    "suite labels are not reviewed; set label_status = \"reviewed\" or omit --fail-on-unreviewed-labels"
                );
            }
            let project = match suite.manifest.project.clone() {
                Some(project) => project,
                None => resolve_project_slug(None, cwd)?,
            };
            let profile = args.profile.parse::<mem_eval::EvalProfile>()?;
            let conditions = args
                .conditions
                .iter()
                .map(|value| value.parse::<mem_eval::EvalCondition>())
                .collect::<Result<Vec<_>>>()?;
            let mut runs = Vec::new();
            let repeat = args
                .repeat
                .max(1)
                .max(suite.manifest.default_repeats.unwrap_or(1));
            let run_group_id = uuid::Uuid::new_v4();
            let suite_checksum = mem_eval::suite_checksum(&suite).ok();
            let mut total_tokens = 0u64;
            for repeat_index in 0..repeat {
                for condition in &conditions {
                    let context = EvalRunContext {
                        profile,
                        repeat_index,
                        run_group_id,
                        suite_checksum: suite_checksum.clone(),
                        dry_run: args.dry_run,
                        artifacts_root: args.out.clone(),
                        memory_command: eval_memory_command(),
                        memory_base_url: service_url(&api.config, ""),
                        memory_config_path: eval_memory_config_path(cwd),
                        llm_judge: args.llm_judge,
                    };
                    let run = run_eval_suite(&suite, &project, *condition, context, api).await?;
                    total_tokens += run
                        .results
                        .iter()
                        .filter_map(|result| result.token_usage.as_ref())
                        .map(|usage| usage.total_tokens)
                        .sum::<u64>();
                    if let Some(max_cost) = args.max_cost
                        && total_tokens > max_cost
                    {
                        anyhow::bail!(
                            "eval token budget exceeded: used {} tokens, limit {}",
                            total_tokens,
                            max_cost
                        );
                    }
                    let filename = format!(
                        "{}-{}-r{}-{}.json",
                        sanitize_filename(&suite.manifest.name),
                        condition,
                        repeat_index,
                        Utc::now().format("%Y%m%d%H%M%S")
                    );
                    let path = args.out.join(filename);
                    mem_eval::write_json(&path, &run)?;
                    runs.push(serde_json::json!({
                    "condition": condition,
                    "profile": profile,
                    "repeat_index": repeat_index,
                    "run_group_id": run_group_id,
                    "path": path,
                    "items": run.results.len(),
                    "successes": run.results.iter().filter(|result| result.success).count(),
                    "skipped": run.results.iter().filter(|result| result.skipped).count(),
                    "tokens": run.results.iter().filter_map(|result| result.token_usage.as_ref()).map(|usage| usage.total_tokens).sum::<u64>(),
                }));
                }
            }
            let payload = serde_json::json!({
                "run_group_id": run_group_id,
                "profile": profile,
                "repeat": repeat,
                "write_transcripts": args.write_transcripts,
                "llm_judge": args.llm_judge,
                "total_tokens": total_tokens,
                "runs": runs,
            });
            if args.text {
                for run in &payload["runs"].as_array().cloned().unwrap_or_default() {
                    println!(
                        "{} [{} r{}]: {} item(s), {} success, {} skipped, {} tokens -> {}",
                        run["condition"].as_str().unwrap_or("?"),
                        run["profile"].as_str().unwrap_or("?"),
                        run["repeat_index"].as_u64().unwrap_or_default(),
                        run["items"].as_u64().unwrap_or_default(),
                        run["successes"].as_u64().unwrap_or_default(),
                        run["skipped"].as_u64().unwrap_or_default(),
                        run["tokens"].as_u64().unwrap_or_default(),
                        run["path"].as_str().unwrap_or("<path>")
                    );
                }
            } else {
                println!("{}", serde_json::to_string_pretty(&payload)?);
            }
        }
        EvalCommand::Compare(args) => {
            let baseline = load_eval_runs_from_patterns(&args.baseline)?;
            let candidate = load_eval_runs_from_patterns(&args.candidate)?;
            let comparison = mem_eval::compare_run_sets(&baseline, &candidate);
            if let Some(path) = args.out {
                mem_eval::write_json(&path, &comparison)?;
            }
            if args.text {
                println!("{}", mem_eval::comparison_text(&comparison));
            } else {
                println!("{}", serde_json::to_string_pretty(&comparison)?);
            }
        }
        EvalCommand::Report(args) => {
            let comparison: mem_eval::EvalComparison = serde_json::from_str(
                &fs::read_to_string(&args.comparison)
                    .with_context(|| format!("read {}", args.comparison.display()))?,
            )
            .with_context(|| format!("parse {}", args.comparison.display()))?;
            let rendered = if args.markdown {
                mem_eval::comparison_markdown(&comparison)
            } else if args.text {
                mem_eval::comparison_text(&comparison)
            } else {
                serde_json::to_string_pretty(&comparison)?
            };
            if let Some(path) = args.out {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("create {}", parent.display()))?;
                }
                fs::write(&path, rendered).with_context(|| format!("write {}", path.display()))?;
            } else {
                println!("{rendered}");
            }
        }
        EvalCommand::Gate(args) => {
            let comparison: mem_eval::EvalComparison = serde_json::from_str(
                &fs::read_to_string(&args.comparison)
                    .with_context(|| format!("read {}", args.comparison.display()))?,
            )
            .with_context(|| format!("parse {}", args.comparison.display()))?;
            let policy: mem_eval::EvalGatePolicy = toml::from_str(
                &fs::read_to_string(&args.policy)
                    .with_context(|| format!("read {}", args.policy.display()))?,
            )
            .with_context(|| format!("parse {}", args.policy.display()))?;
            let result = mem_eval::evaluate_gate(&comparison, &policy);
            if args.text {
                println!("gate: {}", if result.passed { "pass" } else { "fail" });
                for reason in &result.reasons {
                    println!("- {reason}");
                }
            } else {
                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            if !result.passed {
                anyhow::bail!("eval gate failed");
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct EvalRunContext {
    profile: mem_eval::EvalProfile,
    repeat_index: usize,
    run_group_id: uuid::Uuid,
    suite_checksum: Option<String>,
    dry_run: bool,
    artifacts_root: PathBuf,
    memory_command: String,
    memory_base_url: String,
    memory_config_path: Option<PathBuf>,
    llm_judge: bool,
}

async fn run_eval_suite(
    suite: &mem_eval::EvalSuite,
    default_project: &str,
    condition: mem_eval::EvalCondition,
    context: EvalRunContext,
    api: &ApiClient,
) -> Result<mem_eval::EvalRun> {
    let mut results = Vec::new();
    for item in &suite.items {
        if context.dry_run
            && !matches!(
                item,
                mem_eval::EvalItem::AgentBuildTask(_) | mem_eval::EvalItem::AgentBuildSequence(_)
            )
        {
            results.push(mem_eval::skipped_result(
                item,
                condition,
                "dry-run: execution skipped",
            ));
            continue;
        }
        let project = item.project(default_project);
        let mut result = match item {
            mem_eval::EvalItem::RetrievalQa(item) => {
                if condition == mem_eval::EvalCondition::NoMemory {
                    no_memory_retrieval_result(item, condition)
                } else {
                    let response = api
                        .query(&QueryRequest {
                            project: project.to_string(),
                            query: item.question.clone(),
                            filters: QueryFilters::default(),
                            top_k: item.top_k,
                            min_confidence: None,
                            history: false,
                            retrieval_mode: Some(eval_condition_retrieval_mode(condition)),
                            answer_mode: Some(mem_api::QueryAnswerMode::Deterministic),
                        })
                        .await?;
                    mem_eval::score_retrieval_qa(item, condition, &response)
                }
            }
            mem_eval::EvalItem::GroundedAnswer(item) => {
                if condition == mem_eval::EvalCondition::NoMemory {
                    if context.profile == mem_eval::EvalProfile::Offline {
                        offline_no_memory_grounded_answer_eval_item(item, condition)
                    } else {
                        run_no_memory_grounded_answer_eval_item(api, item, condition).await?
                    }
                } else {
                    let response = api
                        .query(&QueryRequest {
                            project: project.to_string(),
                            query: item.question.clone(),
                            filters: QueryFilters::default(),
                            top_k: item.top_k,
                            min_confidence: None,
                            history: false,
                            retrieval_mode: Some(eval_condition_retrieval_mode(condition)),
                            answer_mode: Some(match context.profile {
                                mem_eval::EvalProfile::Llm => mem_api::QueryAnswerMode::Llm,
                                mem_eval::EvalProfile::Offline => {
                                    mem_api::QueryAnswerMode::Deterministic
                                }
                            }),
                        })
                        .await?;
                    mem_eval::score_grounded_answer(item, condition, &response)
                }
            }
            mem_eval::EvalItem::ResumeQuality(item) => {
                if condition == mem_eval::EvalCondition::NoMemory {
                    if context.profile == mem_eval::EvalProfile::Offline {
                        offline_no_memory_resume_quality_eval_item(item, condition)
                    } else {
                        run_no_memory_resume_quality_eval_item(api, item, condition).await?
                    }
                } else {
                    let response = api
                        .up_to_speed(&UpToSpeedRequest {
                            project: project.to_string(),
                            include_llm_summary: false,
                            limit: 20,
                        })
                        .await?;
                    mem_eval::score_up_to_speed_quality(item, condition, &response)
                }
            }
            mem_eval::EvalItem::CommandTask(item) => run_command_eval_item(item, condition)?,
            mem_eval::EvalItem::AgentBuildTask(item) => {
                run_agent_build_eval_item(suite, item, condition, &context)?
            }
            mem_eval::EvalItem::AgentBuildSequence(item) => {
                run_agent_build_sequence_eval_item(suite, item, condition, &context)?
            }
        };
        if matches!(
            condition,
            mem_eval::EvalCondition::Lexical
                | mem_eval::EvalCondition::Semantic
                | mem_eval::EvalCondition::Graph
        ) {
            result
                .notes
                .push("retrieval mode was explicitly requested for eval isolation".to_string());
        }
        if context.llm_judge && context.profile == mem_eval::EvalProfile::Llm {
            add_llm_judge_scores(api, item, &mut result).await?;
        }
        results.push(result);
    }
    Ok(mem_eval::EvalRun {
        suite: suite.manifest.name.clone(),
        project: default_project.to_string(),
        condition,
        profile: context.profile,
        run_group_id: context.run_group_id,
        repeat_index: context.repeat_index,
        suite_checksum: context.suite_checksum,
        fixture_checksum: suite.manifest.fixture.clone(),
        config_fingerprint: None,
        dry_run: context.dry_run,
        created_at: Utc::now(),
        git_head: git_head(),
        service_version: None,
        results,
    })
}

fn load_eval_runs_from_patterns(patterns: &[PathBuf]) -> Result<Vec<mem_eval::EvalRun>> {
    if patterns.is_empty() {
        anyhow::bail!("at least one eval run path is required");
    }
    let mut paths = Vec::new();
    for pattern in patterns {
        let pattern_text = pattern.to_string_lossy();
        if pattern_text.contains('*') || pattern_text.contains('?') {
            paths.extend(expand_eval_run_pattern(pattern)?);
        } else {
            paths.push(pattern.clone());
        }
    }
    paths.sort();
    paths.dedup();
    if paths.is_empty() {
        anyhow::bail!("eval run pattern(s) matched no files");
    }
    paths
        .iter()
        .map(|path| mem_eval::load_run(path))
        .collect::<Result<Vec<_>>>()
}

fn expand_eval_run_pattern(pattern: &Path) -> Result<Vec<PathBuf>> {
    let file_pattern = pattern
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow::anyhow!("invalid eval run glob `{}`", pattern.display()))?;
    let dir = pattern
        .parent()
        .filter(|value| !value.as_os_str().is_empty());
    let dir = dir.unwrap_or_else(|| Path::new("."));
    let mut matches = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry.with_context(|| format!("read entry in {}", dir.display()))?;
        let path = entry.path();
        if path.is_file()
            && path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| wildcard_match(file_pattern, name))
        {
            matches.push(path);
        }
    }
    Ok(matches)
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let (mut p, mut v) = (0usize, 0usize);
    let mut star = None;
    let mut star_value = 0usize;
    while v < value.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == value[v]) {
            p += 1;
            v += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            star_value = v;
        } else if let Some(star_index) = star {
            p = star_index + 1;
            star_value += 1;
            v = star_value;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

fn no_memory_retrieval_result(
    item: &mem_eval::RetrievalQaItem,
    condition: mem_eval::EvalCondition,
) -> mem_eval::EvalItemResult {
    let mut scores = std::collections::BTreeMap::new();
    scores.insert("recall_at_k".to_string(), 0.0);
    scores.insert("mrr".to_string(), 0.0);
    scores.insert("ndcg".to_string(), 0.0);
    scores.insert("citation_precision".to_string(), 1.0);
    scores.insert(
        "tag_recall_at_k".to_string(),
        if item.expected_tags.is_empty() {
            1.0
        } else {
            0.0
        },
    );
    scores.insert(
        "file_recall_at_k".to_string(),
        if item.expected_files.is_empty() {
            1.0
        } else {
            0.0
        },
    );
    mem_eval::EvalItemResult {
        item_id: item.id.clone(),
        eval_type: "retrieval_qa".to_string(),
        condition,
        metadata: item.metadata.clone(),
        success: item.expected_memory_ids.is_empty()
            && item.expected_tags.is_empty()
            && item.expected_files.is_empty(),
        skipped: false,
        scores,
        duration_ms: Some(0),
        token_usage: None,
        answer: None,
        notes: vec!["no-memory condition has no memory retrieval channel".to_string()],
        sub_results: Vec::new(),
    }
}

fn eval_condition_retrieval_mode(
    condition: mem_eval::EvalCondition,
) -> mem_api::QueryRetrievalMode {
    match condition {
        mem_eval::EvalCondition::NoMemory | mem_eval::EvalCondition::FullMemory => {
            mem_api::QueryRetrievalMode::FullMemory
        }
        mem_eval::EvalCondition::Lexical => mem_api::QueryRetrievalMode::Lexical,
        mem_eval::EvalCondition::Semantic => mem_api::QueryRetrievalMode::Semantic,
        mem_eval::EvalCondition::Graph => mem_api::QueryRetrievalMode::Graph,
    }
}

fn offline_no_memory_grounded_answer_eval_item(
    item: &mem_eval::GroundedAnswerItem,
    condition: mem_eval::EvalCondition,
) -> mem_eval::EvalItemResult {
    mem_eval::score_plain_llm_grounded_answer(
        item,
        condition,
        "Offline no-memory baseline: no Memory Layer context was supplied.".to_string(),
        Some(0.0),
        Some(0),
        None,
        vec!["answer_source: offline deterministic no-memory baseline".to_string()],
    )
}

fn offline_no_memory_resume_quality_eval_item(
    item: &mem_eval::ResumeQualityItem,
    condition: mem_eval::EvalCondition,
) -> mem_eval::EvalItemResult {
    mem_eval::score_resume_text_quality(
        item,
        condition,
        "Offline no-memory baseline: no Memory timeline or retrieval context was supplied."
            .to_string(),
        Some(0),
        None,
        vec!["answer_source: offline deterministic no-memory baseline".to_string()],
    )
}

#[derive(Debug)]
struct DirectLlmEvalResponse {
    content: String,
    duration_ms: u64,
    token_usage: Option<TokenUsage>,
}

#[derive(Debug, Deserialize)]
struct NoMemoryGroundedAnswerPayload {
    answer: String,
    #[serde(default)]
    confidence: Option<f32>,
}

#[derive(Debug, Deserialize)]
struct EvalJudgePayload {
    #[serde(default)]
    evidence_use: Option<f64>,
    #[serde(default)]
    reasoning_quality: Option<f64>,
    #[serde(default)]
    consistency: Option<f64>,
    #[serde(default)]
    maintainability: Option<f64>,
    #[serde(default)]
    notes: Option<String>,
}

async fn add_llm_judge_scores(
    api: &ApiClient,
    item: &mem_eval::EvalItem,
    result: &mut mem_eval::EvalItemResult,
) -> Result<()> {
    if !matches!(
        item,
        mem_eval::EvalItem::GroundedAnswer(_) | mem_eval::EvalItem::ResumeQuality(_)
    ) {
        return Ok(());
    }
    let Some(answer) = result.answer.as_deref() else {
        return Ok(());
    };
    let prompt = format!(
        "Eval item id: {}\nEval type: {}\nReasoning mode: {}\nMemory capability: {}\n\nAnswer or briefing:\n{}",
        result.item_id,
        result.eval_type,
        result
            .metadata
            .reasoning_mode
            .as_deref()
            .unwrap_or("unspecified"),
        result
            .metadata
            .memory_capability
            .as_deref()
            .unwrap_or("unspecified"),
        answer
    );
    let response = run_direct_llm_eval(
        api,
        "You are an eval judge. Return strict JSON with numeric 0..1 keys: evidence_use, reasoning_quality, consistency, maintainability, and a short notes string. Score only the supplied answer, not whether you personally know the facts.",
        &prompt,
        api.config.llm.max_output_tokens.min(700),
    )
    .await?;
    let (judge, mut notes) = parse_eval_judge_payload(&response.content);
    if let Some(value) = judge.evidence_use {
        result
            .scores
            .insert("judge_evidence_use".to_string(), value.clamp(0.0, 1.0));
    }
    if let Some(value) = judge.reasoning_quality {
        result
            .scores
            .insert("judge_reasoning_quality".to_string(), value.clamp(0.0, 1.0));
    }
    if let Some(value) = judge.consistency {
        result
            .scores
            .insert("judge_consistency".to_string(), value.clamp(0.0, 1.0));
    }
    if let Some(value) = judge.maintainability {
        result
            .scores
            .insert("judge_maintainability".to_string(), value.clamp(0.0, 1.0));
    }
    if let Some(note) = judge.notes.filter(|note| !note.trim().is_empty()) {
        notes.push(format!("llm_judge: {}", note.trim()));
    }
    result.notes.extend(notes);
    if let Some(usage) = response.token_usage {
        result
            .scores
            .insert("judge_total_tokens".to_string(), usage.total_tokens as f64);
    }
    Ok(())
}

fn parse_eval_judge_payload(content: &str) -> (EvalJudgePayload, Vec<String>) {
    let trimmed = content.trim();
    let json = trimmed
        .strip_prefix("```json")
        .and_then(|value| value.strip_suffix("```"))
        .or_else(|| {
            trimmed
                .strip_prefix("```")
                .and_then(|value| value.strip_suffix("```"))
        })
        .map(str::trim)
        .unwrap_or(trimmed);
    match serde_json::from_str::<EvalJudgePayload>(json) {
        Ok(payload) => (payload, vec!["llm_judge: scored answer".to_string()]),
        Err(_) => (
            EvalJudgePayload {
                evidence_use: None,
                reasoning_quality: None,
                consistency: None,
                maintainability: None,
                notes: None,
            },
            vec!["llm_judge: response was not strict judge JSON".to_string()],
        ),
    }
}

async fn run_no_memory_grounded_answer_eval_item(
    api: &ApiClient,
    item: &mem_eval::GroundedAnswerItem,
    condition: mem_eval::EvalCondition,
) -> Result<mem_eval::EvalItemResult> {
    let response = run_direct_llm_eval(
        api,
        "You answer evaluation questions without Memory Layer context. Return strict JSON with keys: answer (string), confidence (0..1). If you do not know, say so in the answer and use low confidence.",
        &format!("Question: {}", item.question),
        api.config.llm.max_output_tokens.min(800),
    )
    .await?;
    let (answer, confidence, mut notes) = parse_no_memory_grounded_answer(&response.content);
    notes.push("answer_source: direct no-memory LLM call".to_string());
    Ok(mem_eval::score_plain_llm_grounded_answer(
        item,
        condition,
        answer,
        confidence,
        Some(response.duration_ms),
        response.token_usage,
        notes,
    ))
}

async fn run_no_memory_resume_quality_eval_item(
    api: &ApiClient,
    item: &mem_eval::ResumeQualityItem,
    condition: mem_eval::EvalCondition,
) -> Result<mem_eval::EvalItemResult> {
    let prompt = if item.prompt.trim().is_empty() {
        "Get me up to speed on this project. You do not have access to Memory Layer context, repository history, or persisted project timeline data.".to_string()
    } else {
        item.prompt.clone()
    };
    let response = run_direct_llm_eval(
        api,
        "You write concise project resume briefings without Memory Layer context. Be explicit when the prompt lacks enough project evidence.",
        &prompt,
        api.config.llm.max_output_tokens.min(800),
    )
    .await?;
    Ok(mem_eval::score_resume_text_quality(
        item,
        condition,
        response.content,
        Some(response.duration_ms),
        response.token_usage,
        vec!["answer_source: direct no-memory LLM call".to_string()],
    ))
}

async fn run_direct_llm_eval(
    api: &ApiClient,
    system_prompt: &str,
    user_prompt: &str,
    max_output_tokens: u32,
) -> Result<DirectLlmEvalResponse> {
    ensure_direct_llm_eval_config(&api.config)?;
    let api_key = resolve_llm_api_key(&api.config.llm);
    let url = format!(
        "{}/chat/completions",
        effective_llm_base_url(&api.config.llm)
    );
    let mut request = serde_json::json!({
        "model": api.config.llm.model,
        "temperature": 0.0,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_prompt }
        ]
    });
    request[llm_max_output_tokens_field(&api.config.llm.provider)] =
        serde_json::json!(max_output_tokens);
    let started = std::time::Instant::now();
    let mut builder = api.client.post(url);
    if let Some(api_key) = api_key {
        builder = builder.bearer_auth(api_key);
    }
    let http_response = builder
        .json(&request)
        .send()
        .await
        .context("send no-memory eval llm request")?;
    let status = http_response.status();
    let body = http_response
        .text()
        .await
        .context("read no-memory eval llm body")?;
    if !status.is_success() {
        anyhow::bail!("no-memory eval llm request failed: {status} {body}");
    }
    let content = chat_completion_content(&body)?;
    Ok(DirectLlmEvalResponse {
        content,
        duration_ms: started.elapsed().as_millis() as u64,
        token_usage: token_usage_from_chat_body(&body),
    })
}

fn ensure_direct_llm_eval_config(config: &AppConfig) -> Result<()> {
    if !is_supported_llm_provider(&config.llm.provider) {
        anyhow::bail!(
            "no-memory eval requires [llm].provider = openai_compatible or ollama; got `{}`",
            config.llm.provider
        );
    }
    if config.llm.model.trim().is_empty() {
        anyhow::bail!("no-memory eval requires [llm].model to be configured");
    }
    if llm_requires_api_key(&config.llm) && resolve_llm_api_key(&config.llm).is_none() {
        anyhow::bail!(
            "no-memory eval requires {} to be set",
            config.llm.api_key_env
        );
    }
    Ok(())
}

fn chat_completion_content(body: &str) -> Result<String> {
    let payload: serde_json::Value = serde_json::from_str(body).context("parse llm response")?;
    payload
        .get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_str())
        .map(str::trim)
        .filter(|content| !content.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("llm response missing content"))
}

fn parse_no_memory_grounded_answer(content: &str) -> (String, Option<f32>, Vec<String>) {
    let trimmed = content.trim();
    let json = trimmed
        .strip_prefix("```json")
        .and_then(|value| value.strip_suffix("```"))
        .or_else(|| {
            trimmed
                .strip_prefix("```")
                .and_then(|value| value.strip_suffix("```"))
        })
        .map(str::trim)
        .unwrap_or(trimmed);
    match serde_json::from_str::<NoMemoryGroundedAnswerPayload>(json) {
        Ok(payload) if !payload.answer.trim().is_empty() => (
            payload.answer.trim().to_string(),
            payload.confidence.map(|value| value.clamp(0.0, 1.0)),
            Vec::new(),
        ),
        _ => (
            trimmed.to_string(),
            None,
            vec!["plain_llm response was not strict answer/confidence JSON".to_string()],
        ),
    }
}

fn token_usage_from_chat_body(body: &str) -> Option<TokenUsage> {
    let payload: serde_json::Value = serde_json::from_str(body).ok()?;
    let usage = payload.get("usage")?;
    let input_tokens = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(|value| value.as_u64())
        .unwrap_or_default();
    let output_tokens = usage
        .get("completion_tokens")
        .or_else(|| usage.get("output_tokens"))
        .and_then(|value| value.as_u64())
        .unwrap_or_default();
    let cache_read_tokens = usage
        .get("cache_read_input_tokens")
        .or_else(|| usage.get("cached_input_tokens"))
        .and_then(|value| value.as_u64())
        .unwrap_or_default();
    let cache_write_tokens = usage
        .get("cache_creation_input_tokens")
        .and_then(|value| value.as_u64())
        .unwrap_or_default();
    let total_tokens = usage
        .get("total_tokens")
        .and_then(|value| value.as_u64())
        .unwrap_or(input_tokens + output_tokens + cache_read_tokens + cache_write_tokens);
    if input_tokens == 0
        && output_tokens == 0
        && cache_read_tokens == 0
        && cache_write_tokens == 0
        && total_tokens == 0
    {
        return None;
    }
    Some(TokenUsage {
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        total_tokens,
    })
}

fn run_command_eval_item(
    item: &mem_eval::CommandTaskItem,
    condition: mem_eval::EvalCondition,
) -> Result<mem_eval::EvalItemResult> {
    let started = std::time::Instant::now();
    let status = ProcessCommand::new("sh")
        .arg("-c")
        .arg(&item.command)
        .status()
        .with_context(|| format!("run eval command `{}`", item.command))?;
    Ok(mem_eval::score_command_task(
        item,
        condition,
        status.code(),
        Some(started.elapsed().as_millis() as u64),
        Vec::new(),
    ))
}

fn run_agent_build_eval_item(
    suite: &mem_eval::EvalSuite,
    item: &mem_eval::AgentBuildTaskItem,
    condition: mem_eval::EvalCondition,
    context: &EvalRunContext,
) -> Result<mem_eval::EvalItemResult> {
    let started = Instant::now();
    let fixture_dir = suite.root.join(&item.fixture);
    if !fixture_dir.is_dir() {
        anyhow::bail!(
            "agent build task `{}` fixture is not a directory: {}",
            item.id,
            fixture_dir.display()
        );
    }
    validate_agent_build_paths(item)?;
    if context.dry_run {
        return Ok(mem_eval::score_agent_build_task(
            item,
            condition,
            mem_eval::AgentBuildScoreInput {
                agent_exit_code: None,
                setup_exit_codes: Vec::new(),
                score_exit_codes: Vec::new(),
                required_files_present: 0,
                required_files_total: item.required_files.len(),
                forbidden_files_absent: 0,
                forbidden_files_total: item.forbidden_files.len(),
                content_assertions_passed: 0,
                content_assertions_total: item.required_content.len(),
                memory_queries_required: item.memory_questions.len(),
                memory_queries_verified: 0,
                memory_evidence_required: condition != mem_eval::EvalCondition::NoMemory
                    && !item.memory_questions.is_empty(),
                memory_evidence_ok: false,
                token_usage_required: false,
                token_usage_ok: true,
                token_usage: None,
                duration_ms: Some(0),
                notes: vec![
                    "dry-run: validated fixture and command templates without execution"
                        .to_string(),
                ],
                sub_results: Vec::new(),
                skipped: true,
            },
        ));
    }

    let run_dir = context.artifacts_root.join("build-runs").join(format!(
        "{}-{}-{}-r{}-{}",
        sanitize_filename(&suite.manifest.name),
        sanitize_filename(&item.id),
        condition,
        context.repeat_index,
        context.run_group_id.simple()
    ));
    if run_dir.exists() {
        fs::remove_dir_all(&run_dir)
            .with_context(|| format!("remove previous build run {}", run_dir.display()))?;
    }
    let workspace = run_dir.join("workspace");
    fs::create_dir_all(&run_dir).with_context(|| format!("create {}", run_dir.display()))?;
    copy_dir_recursive(&fixture_dir, &workspace)?;
    let project = item
        .project
        .as_deref()
        .or(suite.manifest.project.as_deref())
        .unwrap_or("");
    if condition != mem_eval::EvalCondition::NoMemory && !item.memory_questions.is_empty() {
        write_agent_build_memory_helper(&workspace, item, context)?;
    }

    let prompt = agent_build_prompt(item, condition, context);
    let prompt_file = run_dir.join("prompt.md");
    fs::write(&prompt_file, &prompt).with_context(|| format!("write {}", prompt_file.display()))?;

    let mut notes = vec![format!("artifacts: {}", run_dir.display())];
    let mut setup_exit_codes = Vec::new();
    for (index, command) in item.setup_commands.iter().enumerate() {
        let command = expand_agent_build_template(
            command,
            suite,
            condition,
            &run_dir,
            &workspace,
            &prompt_file,
            project,
        );
        let output = run_eval_shell_command(
            &command,
            &workspace,
            item.timeout_seconds,
            Some(condition),
            Some(project),
            Some(context),
        )?;
        write_command_artifacts(&run_dir, &format!("setup-{index}"), &output)?;
        setup_exit_codes.push(output.exit_code);
    }

    let agent_command = expand_agent_build_template(
        &item.agent_command,
        suite,
        condition,
        &run_dir,
        &workspace,
        &prompt_file,
        project,
    );
    let agent_output = run_eval_shell_command(
        &agent_command,
        &workspace,
        item.timeout_seconds,
        Some(condition),
        Some(project),
        Some(context),
    )?;
    write_command_artifacts(&run_dir, "agent", &agent_output)?;
    if agent_output.timed_out {
        notes.push(format!(
            "agent command timed out after {} second(s)",
            item.timeout_seconds
        ));
    }
    let memory_evidence = validate_agent_build_memory_evidence(&workspace, item, condition)?;
    notes.extend(memory_evidence.notes.clone());

    let mut score_exit_codes = Vec::new();
    for (index, command) in item.score_commands.iter().enumerate() {
        let command = expand_agent_build_template(
            command,
            suite,
            condition,
            &run_dir,
            &workspace,
            &prompt_file,
            project,
        );
        let output = run_eval_shell_command(
            &command,
            &workspace,
            item.timeout_seconds,
            Some(condition),
            Some(project),
            Some(context),
        )?;
        write_command_artifacts(&run_dir, &format!("score-{index}"), &output)?;
        score_exit_codes.push(output.exit_code);
    }

    let required_files_present = item
        .required_files
        .iter()
        .filter(|path| workspace.join(path).is_file())
        .count();
    let forbidden_files_absent = item
        .forbidden_files
        .iter()
        .filter(|path| !workspace.join(path).exists())
        .count();
    let content_assertions_passed = item
        .required_content
        .iter()
        .filter(|assertion| {
            fs::read_to_string(workspace.join(&assertion.file))
                .map(|contents| contents.contains(&assertion.contains))
                .unwrap_or(false)
        })
        .count();

    let summary = serde_json::json!({
        "item_id": item.id,
        "condition": condition,
        "run_dir": run_dir,
        "workspace": workspace,
        "agent_exit_code": agent_output.exit_code,
        "agent_timed_out": agent_output.timed_out,
        "setup_exit_codes": setup_exit_codes,
        "score_exit_codes": score_exit_codes,
        "required_files_present": required_files_present,
        "required_files_total": item.required_files.len(),
        "forbidden_files_absent": forbidden_files_absent,
        "forbidden_files_total": item.forbidden_files.len(),
        "content_assertions_passed": content_assertions_passed,
        "content_assertions_total": item.required_content.len(),
        "memory_queries_required": memory_evidence.required,
        "memory_queries_verified": memory_evidence.verified,
        "memory_evidence_required": true,
        "memory_evidence_ok": memory_evidence.ok,
        "memory_evidence_notes": memory_evidence.notes,
    });
    fs::write(
        run_dir.join("summary.json"),
        serde_json::to_string_pretty(&summary)?,
    )
    .with_context(|| format!("write {}", run_dir.join("summary.json").display()))?;

    Ok(mem_eval::score_agent_build_task(
        item,
        condition,
        mem_eval::AgentBuildScoreInput {
            agent_exit_code: agent_output.exit_code,
            setup_exit_codes,
            score_exit_codes,
            required_files_present,
            required_files_total: item.required_files.len(),
            forbidden_files_absent,
            forbidden_files_total: item.forbidden_files.len(),
            content_assertions_passed,
            content_assertions_total: item.required_content.len(),
            memory_queries_required: memory_evidence.required,
            memory_queries_verified: memory_evidence.verified,
            memory_evidence_required: true,
            memory_evidence_ok: memory_evidence.ok,
            token_usage_required: false,
            token_usage_ok: true,
            token_usage: codex_token_usage_from_run_dir(&run_dir)?,
            duration_ms: Some(started.elapsed().as_millis() as u64),
            notes,
            sub_results: Vec::new(),
            skipped: false,
        },
    ))
}

fn run_agent_build_sequence_eval_item(
    suite: &mem_eval::EvalSuite,
    item: &mem_eval::AgentBuildSequenceItem,
    condition: mem_eval::EvalCondition,
    context: &EvalRunContext,
) -> Result<mem_eval::EvalItemResult> {
    let started = Instant::now();
    let fixture_dir = suite.root.join(&item.fixture);
    if !fixture_dir.is_dir() {
        anyhow::bail!(
            "agent build sequence `{}` fixture is not a directory: {}",
            item.id,
            fixture_dir.display()
        );
    }
    validate_agent_build_sequence_paths(item)?;
    if context.dry_run {
        return Ok(mem_eval::score_agent_build_sequence(
            item,
            condition,
            mem_eval::AgentBuildScoreInput {
                agent_exit_code: None,
                setup_exit_codes: Vec::new(),
                score_exit_codes: Vec::new(),
                required_files_present: 0,
                required_files_total: item
                    .steps
                    .iter()
                    .map(|step| step.required_files.len())
                    .sum(),
                forbidden_files_absent: 0,
                forbidden_files_total: item
                    .steps
                    .iter()
                    .map(|step| step.forbidden_files.len())
                    .sum(),
                content_assertions_passed: 0,
                content_assertions_total: item
                    .steps
                    .iter()
                    .map(|step| step.required_content.len())
                    .sum(),
                memory_queries_required: item
                    .steps
                    .iter()
                    .map(|step| step.memory_questions.len())
                    .sum(),
                memory_queries_verified: 0,
                memory_evidence_required: condition != mem_eval::EvalCondition::NoMemory,
                memory_evidence_ok: false,
                token_usage_required: false,
                token_usage_ok: true,
                token_usage: None,
                duration_ms: Some(0),
                notes: vec![
                    "dry-run: validated sequence fixture and command templates without execution"
                        .to_string(),
                ],
                sub_results: Vec::new(),
                skipped: true,
            },
        ));
    }

    let run_dir = context.artifacts_root.join("build-runs").join(format!(
        "{}-{}-{}-r{}-{}",
        sanitize_filename(&suite.manifest.name),
        sanitize_filename(&item.id),
        condition,
        context.repeat_index,
        context.run_group_id.simple()
    ));
    if run_dir.exists() {
        fs::remove_dir_all(&run_dir)
            .with_context(|| format!("remove previous sequence run {}", run_dir.display()))?;
    }
    let workspace = run_dir.join("workspace");
    let steps_dir = run_dir.join("steps");
    fs::create_dir_all(&steps_dir).with_context(|| format!("create {}", steps_dir.display()))?;
    copy_dir_recursive(&fixture_dir, &workspace)?;
    let project = item
        .project
        .as_deref()
        .or(suite.manifest.project.as_deref())
        .unwrap_or("");

    let mut notes = vec![format!("artifacts: {}", run_dir.display())];
    let mut setup_exit_codes = Vec::new();
    for (index, command) in item.setup_commands.iter().enumerate() {
        let output = run_eval_shell_command(
            &expand_agent_build_template(
                command,
                suite,
                condition,
                &run_dir,
                &workspace,
                &run_dir.join("setup-prompt.md"),
                project,
            ),
            &workspace,
            item.timeout_seconds,
            Some(condition),
            Some(project),
            Some(context),
        )?;
        write_command_artifacts(&run_dir, &format!("setup-{index}"), &output)?;
        setup_exit_codes.push(output.exit_code);
    }

    let mut agent_exit_codes = Vec::new();
    let mut score_exit_codes = Vec::new();
    let mut required_files_present = 0usize;
    let mut required_files_total = 0usize;
    let mut forbidden_files_absent = 0usize;
    let mut forbidden_files_total = 0usize;
    let mut content_assertions_passed = 0usize;
    let mut content_assertions_total = 0usize;
    let mut memory_queries_required = 0usize;
    let mut memory_queries_verified = 0usize;
    let mut memory_evidence_ok = true;
    let mut token_usage = TokenUsage::default();
    let mut saw_token_usage = false;
    let mut step_summaries = Vec::new();
    let mut sub_results = Vec::new();

    for (index, step) in item.steps.iter().enumerate() {
        let step_started = Instant::now();
        let step_label = format!("{:02}-{}", index + 1, sanitize_filename(&step.id));
        let step_dir = steps_dir.join(&step_label);
        fs::create_dir_all(&step_dir).with_context(|| format!("create {}", step_dir.display()))?;
        let step_timeout = step.timeout_seconds.unwrap_or(item.timeout_seconds);
        let step_task = sequence_step_as_task(item, step, step_timeout);
        if workspace.join(".memory-eval").exists() {
            fs::remove_dir_all(workspace.join(".memory-eval"))
                .with_context(|| format!("clear step Memory evidence for {}", step.id))?;
        }
        if condition != mem_eval::EvalCondition::NoMemory && !step.memory_questions.is_empty() {
            write_agent_build_memory_helper(&workspace, &step_task, context)?;
        }
        let prompt = agent_build_prompt(&step_task, condition, context);
        let prompt_file = step_dir.join("prompt.md");
        fs::write(&prompt_file, &prompt)
            .with_context(|| format!("write {}", prompt_file.display()))?;
        let agent_command = expand_agent_build_template(
            &item.agent_command,
            suite,
            condition,
            &step_dir,
            &workspace,
            &prompt_file,
            project,
        );
        let agent_output = run_eval_shell_command(
            &agent_command,
            &workspace,
            step_timeout,
            Some(condition),
            Some(project),
            Some(context),
        )?;
        write_command_artifacts(&step_dir, "agent", &agent_output)?;
        agent_exit_codes.push(agent_output.exit_code);
        if agent_output.timed_out {
            notes.push(format!(
                "step {} agent command timed out after {} second(s)",
                step.id, step_timeout
            ));
        }
        let memory_evidence =
            validate_agent_build_memory_evidence(&workspace, &step_task, condition)?;
        notes.extend(
            memory_evidence
                .notes
                .iter()
                .map(|note| format!("step {}: {note}", step.id)),
        );
        memory_queries_required += memory_evidence.required;
        memory_queries_verified += memory_evidence.verified;
        memory_evidence_ok &= memory_evidence.ok;
        if workspace.join(".memory-eval").is_dir() {
            copy_dir_recursive(
                &workspace.join(".memory-eval"),
                &step_dir.join("memory-eval"),
            )?;
        }

        let mut step_score_exit_codes = Vec::new();
        for (score_index, command) in step.score_commands.iter().enumerate() {
            let output = run_eval_shell_command(
                &expand_agent_build_template(
                    command,
                    suite,
                    condition,
                    &step_dir,
                    &workspace,
                    &prompt_file,
                    project,
                ),
                &workspace,
                step_timeout,
                Some(condition),
                Some(project),
                Some(context),
            )?;
            write_command_artifacts(&step_dir, &format!("score-{score_index}"), &output)?;
            step_score_exit_codes.push(output.exit_code);
            score_exit_codes.push(output.exit_code);
        }

        let step_required_present = step
            .required_files
            .iter()
            .filter(|path| workspace.join(path).is_file())
            .count();
        let step_forbidden_absent = step
            .forbidden_files
            .iter()
            .filter(|path| !workspace.join(path).exists())
            .count();
        let step_content_passed = step
            .required_content
            .iter()
            .filter(|assertion| {
                fs::read_to_string(workspace.join(&assertion.file))
                    .map(|contents| contents.contains(&assertion.contains))
                    .unwrap_or(false)
            })
            .count();
        required_files_present += step_required_present;
        required_files_total += step.required_files.len();
        forbidden_files_absent += step_forbidden_absent;
        forbidden_files_total += step.forbidden_files.len();
        content_assertions_passed += step_content_passed;
        content_assertions_total += step.required_content.len();

        let step_token_usage = codex_token_usage_from_run_dir(&step_dir)?;
        if let Some(usage) = &step_token_usage {
            saw_token_usage = true;
            add_token_usage(&mut token_usage, usage);
        }
        let step_success = agent_output.exit_code == Some(0)
            && step_score_exit_codes.iter().all(|code| *code == Some(0))
            && step_required_present == step.required_files.len()
            && step_forbidden_absent == step.forbidden_files.len()
            && step_content_passed == step.required_content.len()
            && memory_evidence.ok;
        let mut step_scores = BTreeMap::new();
        step_scores.insert(
            "agent_exit_code".to_string(),
            agent_output.exit_code.unwrap_or(-1) as f64,
        );
        step_scores.insert(
            "score_commands_passed".to_string(),
            step_score_exit_codes
                .iter()
                .filter(|code| **code == Some(0))
                .count() as f64,
        );
        step_scores.insert(
            "score_commands_total".to_string(),
            step_score_exit_codes.len() as f64,
        );
        step_scores.insert(
            "required_files_present".to_string(),
            step_required_present as f64,
        );
        step_scores.insert(
            "required_files_total".to_string(),
            step.required_files.len() as f64,
        );
        step_scores.insert(
            "forbidden_files_absent".to_string(),
            step_forbidden_absent as f64,
        );
        step_scores.insert(
            "forbidden_files_total".to_string(),
            step.forbidden_files.len() as f64,
        );
        step_scores.insert(
            "content_assertions_passed".to_string(),
            step_content_passed as f64,
        );
        step_scores.insert(
            "content_assertions_total".to_string(),
            step.required_content.len() as f64,
        );
        step_scores.insert(
            "memory_queries_required".to_string(),
            memory_evidence.required as f64,
        );
        step_scores.insert(
            "memory_queries_verified".to_string(),
            memory_evidence.verified as f64,
        );
        step_scores.insert(
            "memory_evidence_ok".to_string(),
            if memory_evidence.ok { 1.0 } else { 0.0 },
        );
        step_scores.insert(
            "total_score".to_string(),
            if step_success { 1.0 } else { 0.0 },
        );
        sub_results.push(mem_eval::EvalSubResult {
            id: step.id.clone(),
            eval_type: "agent_build_sequence_step".to_string(),
            metadata: step.metadata.clone(),
            success: step_success,
            skipped: false,
            scores: step_scores,
            duration_ms: Some(step_started.elapsed().as_millis() as u64),
            token_usage: step_token_usage.clone(),
            notes: memory_evidence.notes.clone(),
        });
        step_summaries.push(serde_json::json!({
            "id": step.id,
            "metadata": step.metadata,
            "success": step_success,
            "agent_exit_code": agent_output.exit_code,
            "score_exit_codes": step_score_exit_codes,
            "memory_queries_required": memory_evidence.required,
            "memory_queries_verified": memory_evidence.verified,
            "memory_evidence_ok": memory_evidence.ok,
            "token_usage": step_token_usage,
        }));
    }

    let token_usage_required = agent_build_command_requires_token_usage(&item.agent_command);
    let token_usage_ok = !token_usage_required || saw_token_usage;
    if !token_usage_ok {
        notes.push(
            "Codex sequence run did not emit parseable token usage; expected codex-events.jsonl or codex-token-usage.json"
                .to_string(),
        );
    }

    let summary = serde_json::json!({
        "item_id": item.id,
        "condition": condition,
        "run_dir": run_dir,
        "workspace": workspace,
        "steps": step_summaries,
        "setup_exit_codes": setup_exit_codes,
        "memory_queries_required": memory_queries_required,
        "memory_queries_verified": memory_queries_verified,
        "memory_evidence_ok": memory_evidence_ok,
        "token_usage_required": token_usage_required,
        "token_usage_ok": token_usage_ok,
        "token_usage": if saw_token_usage { Some(&token_usage) } else { None },
    });
    fs::write(
        run_dir.join("summary.json"),
        serde_json::to_string_pretty(&summary)?,
    )
    .with_context(|| format!("write {}", run_dir.join("summary.json").display()))?;

    let agent_exit_code = if agent_exit_codes.iter().all(|code| *code == Some(0)) {
        Some(0)
    } else {
        agent_exit_codes
            .iter()
            .copied()
            .find(|code| *code != Some(0))
            .flatten()
    };
    Ok(mem_eval::score_agent_build_sequence(
        item,
        condition,
        mem_eval::AgentBuildScoreInput {
            agent_exit_code,
            setup_exit_codes,
            score_exit_codes,
            required_files_present,
            required_files_total,
            forbidden_files_absent,
            forbidden_files_total,
            content_assertions_passed,
            content_assertions_total,
            memory_queries_required,
            memory_queries_verified,
            memory_evidence_required: true,
            memory_evidence_ok,
            token_usage_required,
            token_usage_ok,
            token_usage: saw_token_usage.then_some(token_usage),
            duration_ms: Some(started.elapsed().as_millis() as u64),
            notes,
            sub_results,
            skipped: false,
        },
    ))
}

fn agent_build_command_requires_token_usage(command: &str) -> bool {
    command.contains("run-codex") || command.split_whitespace().any(|part| part == "codex")
}

fn sequence_step_as_task(
    item: &mem_eval::AgentBuildSequenceItem,
    step: &mem_eval::AgentBuildSequenceStep,
    timeout_seconds: u64,
) -> mem_eval::AgentBuildTaskItem {
    mem_eval::AgentBuildTaskItem {
        id: step.id.clone(),
        metadata: step.metadata.clone(),
        project: item.project.clone(),
        prompt: step.prompt.clone(),
        fixture: item.fixture.clone(),
        agent_command: item.agent_command.clone(),
        memory_questions: step.memory_questions.clone(),
        setup_commands: Vec::new(),
        score_commands: step.score_commands.clone(),
        timeout_seconds,
        required_files: step.required_files.clone(),
        forbidden_files: step.forbidden_files.clone(),
        required_content: step.required_content.clone(),
    }
}

#[derive(Debug)]
struct AgentBuildMemoryEvidence {
    required: usize,
    verified: usize,
    ok: bool,
    notes: Vec<String>,
}

fn write_agent_build_memory_helper(
    workspace: &Path,
    item: &mem_eval::AgentBuildTaskItem,
    _context: &EvalRunContext,
) -> Result<()> {
    let evidence_dir = workspace.join(".memory-eval");
    fs::create_dir_all(&evidence_dir)
        .with_context(|| format!("create {}", evidence_dir.display()))?;
    let helper_binary = evidence_dir.join("memory");
    let current_exe = env::current_exe()?;
    let copy_source = if Path::new("/proc/self/exe").is_file() {
        Path::new("/proc/self/exe")
    } else {
        current_exe.as_path()
    };
    fs::copy(copy_source, &helper_binary)
        .with_context(|| format!("copy Memory CLI to {}", helper_binary.display()))?;
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&helper_binary)
            .with_context(|| format!("stat {}", helper_binary.display()))?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&helper_binary, permissions)
            .with_context(|| format!("chmod {}", helper_binary.display()))?;
    }
    let questions = item
        .memory_questions
        .iter()
        .enumerate()
        .map(|(index, question)| {
            serde_json::json!({
                "id": agent_build_memory_question_id(index),
                "question": question,
            })
        })
        .collect::<Vec<_>>();
    fs::write(
        evidence_dir.join("required-questions.json"),
        serde_json::to_vec_pretty(&questions)?,
    )
    .with_context(|| {
        format!(
            "write {}",
            evidence_dir.join("required-questions.json").display()
        )
    })?;
    let helper = r#"#!/usr/bin/env sh
set -eu

if [ "$#" -lt 2 ]; then
  echo "usage: ./.memory-eval/query-memory <question-id> <question>" >&2
  exit 64
fi

question_id="$1"
shift
question="$*"

case "$question_id" in
  q[0-9]*) ;;
  *)
    echo "invalid Memory eval question id: $question_id" >&2
    exit 64
    ;;
esac

mkdir -p .memory-eval
out=".memory-eval/${question_id}.json"
err=".memory-eval/${question_id}.stderr.txt"
status=".memory-eval/${question_id}.status.json"
cmd="./.memory-eval/memory"

set +e
"$cmd" query --project "${MEMORY_LAYER_PROJECT:?}" --question "$question" --json > "$out" 2> "$err"
code=$?
set -e
if [ "$code" -eq 0 ] && [ ! -s "$out" ]; then
  echo "Memory query wrote an empty JSON payload" >> "$err"
  code=65
fi

printf '{"question_id":"%s","exit_code":%s,"output_file":"%s"}\n' "$question_id" "$code" "$out" > "$status"
exit "$code"
"#;
    let helper_path = evidence_dir.join("query-memory");
    fs::write(&helper_path, helper).with_context(|| format!("write {}", helper_path.display()))?;
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&helper_path)
            .with_context(|| format!("stat {}", helper_path.display()))?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&helper_path, permissions)
            .with_context(|| format!("chmod {}", helper_path.display()))?;
    }
    Ok(())
}

fn validate_agent_build_memory_evidence(
    workspace: &Path,
    item: &mem_eval::AgentBuildTaskItem,
    condition: mem_eval::EvalCondition,
) -> Result<AgentBuildMemoryEvidence> {
    if condition == mem_eval::EvalCondition::NoMemory {
        let forbidden = [
            workspace.join("memory-evidence.md"),
            workspace.join("memory-evidence.json"),
            workspace.join(".memory-eval"),
        ];
        let leaked = forbidden
            .iter()
            .filter(|path| path.exists())
            .map(|path| {
                path.strip_prefix(workspace)
                    .unwrap_or(path)
                    .display()
                    .to_string()
            })
            .collect::<Vec<_>>();
        let ok = leaked.is_empty();
        return Ok(AgentBuildMemoryEvidence {
            required: 0,
            verified: 0,
            ok,
            notes: if ok {
                vec!["no-memory run left no Memory evidence artifacts".to_string()]
            } else {
                vec![format!(
                    "no-memory run produced forbidden Memory evidence artifact(s): {}",
                    leaked.join(", ")
                )]
            },
        });
    }

    if item.memory_questions.is_empty() {
        return Ok(AgentBuildMemoryEvidence {
            required: 0,
            verified: 0,
            ok: true,
            notes: vec!["memory-enabled run had no required Memory questions".to_string()],
        });
    }

    let mut verified = 0usize;
    let mut notes = Vec::new();
    for (index, question) in item.memory_questions.iter().enumerate() {
        let question_id = agent_build_memory_question_id(index);
        let output_path = workspace
            .join(".memory-eval")
            .join(format!("{question_id}.json"));
        let status_path = workspace
            .join(".memory-eval")
            .join(format!("{question_id}.status.json"));
        let status_ok = if !status_path.is_file() {
            notes.push(format!("missing Memory query status for {question_id}"));
            false
        } else {
            match read_json_file(&status_path) {
                Ok(status) => {
                    if status.get("exit_code").and_then(serde_json::Value::as_i64) != Some(0) {
                        notes.push(format!("Memory query {question_id} exited non-zero"));
                        false
                    } else {
                        true
                    }
                }
                Err(error) => {
                    notes.push(format!(
                        "Memory query {question_id} status is invalid: {error}"
                    ));
                    false
                }
            }
        };
        let result_count = if !output_path.is_file() {
            notes.push(format!("missing Memory query output for {question_id}"));
            0
        } else {
            match read_json_file(&output_path) {
                Ok(output) => {
                    let result_count = output
                        .get("results")
                        .and_then(serde_json::Value::as_array)
                        .map(Vec::len)
                        .unwrap_or(0);
                    if result_count == 0 {
                        notes.push(format!("Memory query {question_id} returned no memories"));
                    }
                    result_count
                }
                Err(error) => {
                    notes.push(format!(
                        "Memory query {question_id} output is invalid: {error}"
                    ));
                    0
                }
            }
        };
        if status_ok && result_count > 0 {
            verified += 1;
            notes.push(format!(
                "verified Memory query {question_id} ({result_count} result(s)): {question}"
            ));
        }
    }
    let required = item.memory_questions.len();
    Ok(AgentBuildMemoryEvidence {
        required,
        verified,
        ok: verified == required,
        notes,
    })
}

fn read_json_file(path: &Path) -> Result<serde_json::Value> {
    let contents = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&contents).with_context(|| format!("parse {}", path.display()))
}

fn agent_build_memory_question_id(index: usize) -> String {
    format!("q{}", index + 1)
}

#[derive(Debug)]
struct EvalShellOutput {
    exit_code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    timed_out: bool,
}

fn run_eval_shell_command(
    command: &str,
    cwd: &Path,
    timeout_seconds: u64,
    condition: Option<mem_eval::EvalCondition>,
    project: Option<&str>,
    context: Option<&EvalRunContext>,
) -> Result<EvalShellOutput> {
    let mut command_builder = ProcessCommand::new("sh");
    command_builder
        .arg("-c")
        .arg(command)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("MEMORY_EVAL_WORKSPACE", cwd)
        .env("MEMORY_EVAL_TIMEOUT_SECONDS", timeout_seconds.to_string());
    if let Some(condition) = condition {
        command_builder.env("MEMORY_EVAL_CONDITION", condition.to_string());
        match condition {
            mem_eval::EvalCondition::NoMemory => {
                command_builder
                    .env("MEMORY_EVAL_MEMORY_ENABLED", "0")
                    .env_remove("MEMORY_LAYER_API_TOKEN")
                    .env_remove("MEMORY_LAYER_AGENT_ID")
                    .env_remove("MEMORY_LAYER_PROJECT")
                    .env_remove("MEMORY_CONFIG")
                    .env_remove("MEMORY_LAYER_CONFIG")
                    .env_remove("MEMORY_BASE_URL");
            }
            _ => {
                command_builder.env("MEMORY_EVAL_MEMORY_ENABLED", "1");
                if let Some(project) = project {
                    command_builder.env("MEMORY_LAYER_PROJECT", project);
                }
                if let Some(context) = context {
                    command_builder
                        .env("MEMORY_EVAL_MEMORY_COMMAND", &context.memory_command)
                        .env("MEMORY_BASE_URL", &context.memory_base_url);
                    if let Some(config_path) = &context.memory_config_path {
                        command_builder
                            .env("MEMORY_CONFIG", config_path)
                            .env("MEMORY_LAYER_CONFIG", config_path);
                        if config_path
                            .parent()
                            .map(|parent| parent.join("config.dev.toml").is_file())
                            .unwrap_or(false)
                        {
                            command_builder.env("MEMORY_LAYER_PROFILE", "dev");
                        }
                    }
                }
            }
        }
    }
    let mut child = command_builder
        .spawn()
        .with_context(|| format!("run eval command `{command}` in {}", cwd.display()))?;
    let timeout = Duration::from_secs(timeout_seconds.max(1));
    let started = Instant::now();
    let mut timed_out = false;
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            let _ = child.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let output = child
        .wait_with_output()
        .with_context(|| format!("collect eval command `{command}` output"))?;
    Ok(EvalShellOutput {
        exit_code: output.status.code(),
        stdout: output.stdout,
        stderr: output.stderr,
        timed_out,
    })
}

fn write_command_artifacts(run_dir: &Path, stem: &str, output: &EvalShellOutput) -> Result<()> {
    fs::write(run_dir.join(format!("{stem}.stdout.txt")), &output.stdout)
        .with_context(|| format!("write {stem} stdout"))?;
    fs::write(run_dir.join(format!("{stem}.stderr.txt")), &output.stderr)
        .with_context(|| format!("write {stem} stderr"))?;
    fs::write(
        run_dir.join(format!("{stem}.status.json")),
        serde_json::to_string_pretty(&serde_json::json!({
            "exit_code": output.exit_code,
            "timed_out": output.timed_out,
        }))?,
    )
    .with_context(|| format!("write {stem} status"))?;
    Ok(())
}

fn codex_token_usage_from_run_dir(run_dir: &Path) -> Result<Option<TokenUsage>> {
    let usage_path = run_dir.join("codex-token-usage.json");
    if usage_path.is_file() {
        let value: serde_json::Value = read_json_file(&usage_path)?;
        return Ok(token_usage_from_json_value(&value));
    }
    let events_path = run_dir.join("codex-events.jsonl");
    if !events_path.is_file() {
        return Ok(None);
    }
    let contents = fs::read_to_string(&events_path)
        .with_context(|| format!("read {}", events_path.display()))?;
    let mut usage = TokenUsage::default();
    let mut found = false;
    for line in contents.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if let Some(candidate) = token_usage_from_json_value(&value)
            && candidate.total_tokens >= usage.total_tokens
        {
            usage = candidate;
            found = true;
        }
    }
    if found {
        fs::write(&usage_path, serde_json::to_vec_pretty(&usage)?)
            .with_context(|| format!("write {}", usage_path.display()))?;
        Ok(Some(usage))
    } else {
        Ok(None)
    }
}

fn token_usage_from_json_value(value: &serde_json::Value) -> Option<TokenUsage> {
    let usage = value
        .get("usage")
        .or_else(|| value.get("token_usage"))
        .or_else(|| value.get("tokenUsage"))
        .or_else(|| value.get("total_token_usage"))
        .or_else(|| value.get("totalTokenUsage"))
        .unwrap_or(value);
    let input_tokens = json_u64_any(
        usage,
        &[
            "input_tokens",
            "prompt_tokens",
            "inputTokens",
            "promptTokens",
        ],
    );
    let output_tokens = json_u64_any(
        usage,
        &[
            "output_tokens",
            "completion_tokens",
            "outputTokens",
            "completionTokens",
        ],
    );
    let cache_read_tokens = json_u64_any(
        usage,
        &[
            "cache_read_tokens",
            "cached_input_tokens",
            "cacheReadTokens",
            "cachedInputTokens",
        ],
    );
    let cache_write_tokens = json_u64_any(
        usage,
        &[
            "cache_write_tokens",
            "cache_creation_input_tokens",
            "cacheWriteTokens",
            "cacheCreationInputTokens",
        ],
    );
    let total_tokens = json_u64_any(usage, &["total_tokens", "totalTokens", "tokens_used"])
        .unwrap_or(
            input_tokens.unwrap_or(0)
                + output_tokens.unwrap_or(0)
                + cache_read_tokens.unwrap_or(0)
                + cache_write_tokens.unwrap_or(0),
        );
    if total_tokens == 0 {
        for child in value_children(value) {
            if let Some(nested) = token_usage_from_json_value(child)
                && nested.total_tokens > 0
            {
                return Some(nested);
            }
        }
        return None;
    }
    Some(TokenUsage {
        input_tokens: input_tokens.unwrap_or(0),
        output_tokens: output_tokens.unwrap_or(0),
        cache_read_tokens: cache_read_tokens.unwrap_or(0),
        cache_write_tokens: cache_write_tokens.unwrap_or(0),
        total_tokens,
    })
}

fn json_u64_any(value: &serde_json::Value, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(serde_json::Value::as_u64))
}

fn value_children(value: &serde_json::Value) -> Vec<&serde_json::Value> {
    match value {
        serde_json::Value::Array(values) => values.iter().collect(),
        serde_json::Value::Object(map) => map.values().collect(),
        _ => Vec::new(),
    }
}

fn add_token_usage(total: &mut TokenUsage, usage: &TokenUsage) {
    total.input_tokens += usage.input_tokens;
    total.output_tokens += usage.output_tokens;
    total.cache_read_tokens += usage.cache_read_tokens;
    total.cache_write_tokens += usage.cache_write_tokens;
    total.total_tokens += usage.total_tokens;
}

fn agent_build_prompt(
    item: &mem_eval::AgentBuildTaskItem,
    condition: mem_eval::EvalCondition,
    context: &EvalRunContext,
) -> String {
    let mut prompt = item.prompt.trim().to_string();
    prompt.push_str("\n\n");
    match condition {
        mem_eval::EvalCondition::NoMemory => prompt.push_str(
            "Evaluation condition: no-memory. Do not query, read, or use Memory Layer context. Do not create memory-evidence.md, memory-evidence.json, or .memory-eval artifacts. Work only from the repository files and this prompt.\n",
        ),
        _ => {
            prompt.push_str(
                "Evaluation condition: memory-enabled. Use Memory Layer context before implementing, then make the requested code changes in the workspace.\n",
            );
            prompt.push_str("\nUse this Memory CLI command from the shell:\n\n```bash\n");
            prompt.push_str(&context.memory_command);
            prompt.push_str("\n```\n\n");
            prompt.push_str("A harness-provided helper exists at `./.memory-eval/query-memory`. Use that helper for every required Memory question so the eval can verify real Memory service access. Do not fabricate Memory evidence; if a helper command fails, stop and report the failure.\n");
            prompt.push_str("Write a file named memory-evidence.md that summarizes the useful facts you used after the helper commands succeed.\n");
            if !item.memory_questions.is_empty() {
                prompt.push_str("\nRequired Memory questions:\n");
                for (index, question) in item.memory_questions.iter().enumerate() {
                    let question_id = agent_build_memory_question_id(index);
                    prompt.push_str("- ");
                    prompt.push_str(&question_id);
                    prompt.push_str(": ");
                    prompt.push_str(question);
                    prompt.push('\n');
                }
                prompt.push_str("\nRun these exact helper commands before editing files:\n\n```bash\n");
                for (index, question) in item.memory_questions.iter().enumerate() {
                    let question_id = agent_build_memory_question_id(index);
                    prompt.push_str("./.memory-eval/query-memory ");
                    prompt.push_str(&question_id);
                    prompt.push(' ');
                    prompt.push_str(&shell_quote_value(question));
                    prompt.push('\n');
                }
                prompt.push_str("```\n");
            }
        }
    }
    prompt
}

fn expand_agent_build_template(
    template: &str,
    suite: &mem_eval::EvalSuite,
    condition: mem_eval::EvalCondition,
    run_dir: &Path,
    workspace: &Path,
    prompt_file: &Path,
    project: &str,
) -> String {
    template
        .replace("{suite_dir}", &shell_quote_path(&suite.root))
        .replace("{run_dir}", &shell_quote_path(run_dir))
        .replace("{workspace}", &shell_quote_path(workspace))
        .replace("{prompt_file}", &shell_quote_path(prompt_file))
        .replace("{condition}", &condition.to_string())
        .replace("{project}", project)
}

fn shell_quote_value(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn shell_quote_path(path: &Path) -> String {
    let absolute = absolute_eval_path(path);
    let value = absolute.to_string_lossy();
    shell_quote_value(&value)
}

fn absolute_eval_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

fn validate_agent_build_paths(item: &mem_eval::AgentBuildTaskItem) -> Result<()> {
    for path in item
        .required_files
        .iter()
        .chain(item.forbidden_files.iter())
        .chain(
            item.required_content
                .iter()
                .map(|assertion| &assertion.file),
        )
    {
        let candidate = Path::new(path);
        if candidate.is_absolute()
            || candidate
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            anyhow::bail!(
                "agent build task `{}` path must be workspace-relative without `..`: {}",
                item.id,
                path
            );
        }
    }
    Ok(())
}

fn validate_agent_build_suite_item(
    suite: &mem_eval::EvalSuite,
    item: &mem_eval::AgentBuildTaskItem,
) -> Result<()> {
    let fixture_dir = suite.root.join(&item.fixture);
    if !fixture_dir.is_dir() {
        anyhow::bail!("fixture is not a directory: {}", fixture_dir.display());
    }
    if item.agent_command.trim().is_empty() {
        anyhow::bail!("agent_command must not be empty");
    }
    validate_agent_build_paths(item)?;
    Ok(())
}

fn validate_agent_build_sequence_paths(item: &mem_eval::AgentBuildSequenceItem) -> Result<()> {
    if item.steps.is_empty() {
        anyhow::bail!(
            "agent build sequence `{}` must contain at least one step",
            item.id
        );
    }
    for step in &item.steps {
        let task = sequence_step_as_task(
            item,
            step,
            step.timeout_seconds.unwrap_or(item.timeout_seconds),
        );
        validate_agent_build_paths(&task)?;
    }
    Ok(())
}

fn validate_agent_build_sequence_suite_item(
    suite: &mem_eval::EvalSuite,
    item: &mem_eval::AgentBuildSequenceItem,
) -> Result<()> {
    let fixture_dir = suite.root.join(&item.fixture);
    if !fixture_dir.is_dir() {
        anyhow::bail!("fixture is not a directory: {}", fixture_dir.display());
    }
    if item.agent_command.trim().is_empty() {
        anyhow::bail!("agent_command must not be empty");
    }
    validate_agent_build_sequence_paths(item)?;
    Ok(())
}

fn eval_memory_command() -> String {
    if let (Ok(exe), Ok(cwd)) = (env::current_exe(), env::current_dir()) {
        let manifest_path = cwd.join("Cargo.toml");
        let is_cargo_target_binary = exe
            .components()
            .any(|component| component.as_os_str() == "target")
            && manifest_path.is_file();
        if is_cargo_target_binary {
            return format!(
                "cargo run --quiet --manifest-path {} --bin memory --",
                shell_quote_value(&manifest_path.to_string_lossy())
            );
        }
        return exe.to_string_lossy().to_string();
    }
    "memory".to_string()
}

fn eval_memory_config_path(cwd: &Path) -> Option<PathBuf> {
    env::var_os("MEMORY_CONFIG")
        .map(PathBuf::from)
        .or_else(|| env::var_os("MEMORY_LAYER_CONFIG").map(PathBuf::from))
        .or_else(|| {
            let candidate = cwd.join(".mem").join("config.toml");
            candidate.exists().then_some(candidate)
        })
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination).with_context(|| format!("create {}", destination.display()))?;
    for entry in fs::read_dir(source).with_context(|| format!("read {}", source.display()))? {
        let entry = entry.with_context(|| format!("read entry in {}", source.display()))?;
        let file_type = entry
            .file_type()
            .with_context(|| format!("read file type for {}", entry.path().display()))?;
        let target = destination.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), &target).with_context(|| {
                format!("copy {} to {}", entry.path().display(), target.display())
            })?;
        } else if file_type.is_symlink() {
            anyhow::bail!(
                "agent build fixtures may not contain symlinks: {}",
                entry.path().display()
            );
        }
    }
    Ok(())
}

fn git_head() -> Option<String> {
    ProcessCommand::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
}

fn sanitize_filename(value: &str) -> String {
    let slug = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    slug.trim_matches('-').to_string()
}

fn print_activities_response(response: &ActivityListResponse) {
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

fn activity_graph_suffix(event: &mem_api::ActivityEvent) -> String {
    match &event.details {
        Some(mem_api::ActivityDetails::Query {
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

fn print_up_to_speed_response(response: &UpToSpeedResponse) {
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

fn activity_kind_text(kind: &mem_api::ActivityKind) -> &'static str {
    match kind {
        mem_api::ActivityKind::Checkpoint => "checkpoint",
        mem_api::ActivityKind::Scan => "scan",
        mem_api::ActivityKind::Plan => "plan",
        mem_api::ActivityKind::CommitSync => "commit_sync",
        mem_api::ActivityKind::BundleExport => "bundle_export",
        mem_api::ActivityKind::BundleImport => "bundle_import",
        mem_api::ActivityKind::GraphExtract => "graph_extract",
        mem_api::ActivityKind::Query => "query",
        mem_api::ActivityKind::QueryError => "query_error",
        mem_api::ActivityKind::WatcherHealth => "watcher_health",
        mem_api::ActivityKind::MemoryReplacement => "replacement",
        mem_api::ActivityKind::CaptureTask => "capture",
        mem_api::ActivityKind::Curate => "curate",
        mem_api::ActivityKind::Reindex => "reindex",
        mem_api::ActivityKind::Reembed => "reembed",
        mem_api::ActivityKind::Archive => "archive",
        mem_api::ActivityKind::DeleteMemory => "delete",
        mem_api::ActivityKind::Briefing => "briefing",
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

async fn connect_graph_database(config: &AppConfig) -> Result<sqlx::PgPool> {
    PgPoolOptions::new()
        .max_connections(5)
        .connect(&config.database.url)
        .await
        .context("connect graph database")
}

fn print_graph_extract_report(report: &mem_graph::GraphExtractionReport, index_path: &Path) {
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

fn build_graph_activity_request(report: &mem_graph::GraphExtractionReport) -> GraphActivityRequest {
    GraphActivityRequest {
        project: report.project.clone(),
        repo_root: report.repo_root.clone(),
        git_head: report.git_head.clone(),
        since: report.since.clone(),
        extraction_run_id: report.extraction_run_id,
        dry_run: report.dry_run,
        reused_existing_run: report.reused_existing_run,
        index_reused: report.index_reused,
        analyzer_version: report.analyzer_version.clone(),
        strategy_version: report.strategy_version.clone(),
        symbol_count: report.symbol_count,
        reference_count: report.reference_count,
        resolved_reference_count: report.resolved_reference_count,
        unresolved_reference_count: report.unresolved_reference_count,
        ambiguous_reference_count: report.ambiguous_reference_count,
        graph_node_count: report.graph_node_count,
        graph_edge_count: report.graph_edge_count,
        evidence_count: report.evidence_count,
    }
}

fn print_graph_status(status: &Option<mem_graph::GraphStatusReport>, project: &str) {
    let Some(status) = status else {
        println!("No code graph extraction found for {project}.");
        println!("Build one with: memory graph extract --project {project}");
        return;
    };
    println!("Code graph status for {}\n", status.project);
    println!("Status: {}", status.status);
    if let Some(completed_at) = status.completed_at {
        println!("Completed: {completed_at}");
    }
    println!("Extraction run: {}", status.extraction_run_id);
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
    println!(
        "Analyzer: {} | Strategy: {}",
        status.analyzer_version, status.strategy_version
    );
    if let Some(head) = &status.git_head {
        println!("HEAD: {head}");
    }
    if let Some(since) = &status.since {
        println!("Since: {since}");
    }
    println!("Repo: {}", status.repo_root);
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
        "task" => Ok(mem_api::MemoryType::Task),
        "plan" => Ok(mem_api::MemoryType::Plan),
        "implementation" => Ok(mem_api::MemoryType::Implementation),
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
        .collect::<Vec<_>>();

    let title = args
        .title
        .unwrap_or_else(|| format!("Memory update for {project}"));
    let prompt = args
        .prompt
        .unwrap_or_else(|| format!("Auto-captured repository work in project {project}."));
    let summary = args
        .summary
        .unwrap_or_else(|| derive_summary(project, &files_changed));
    let mut candidate = build_remember_implementation_candidate(
        &summary,
        &prompt,
        &args.notes,
        &files_changed,
        &tests,
        command_output.as_deref(),
    );
    if let Some(type_str) = &args.memory_type {
        candidate.memory_type = parse_memory_type_arg(type_str)?;
    }

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
        structured_candidates: vec![candidate],
        command_output,
        idempotency_key: None,
        dry_run: false,
    })
}

fn parse_memory_type_arg(value: &str) -> Result<MemoryType> {
    match value {
        "architecture" => Ok(MemoryType::Architecture),
        "convention" => Ok(MemoryType::Convention),
        "decision" => Ok(MemoryType::Decision),
        "incident" => Ok(MemoryType::Incident),
        "debugging" => Ok(MemoryType::Debugging),
        "environment" => Ok(MemoryType::Environment),
        "domain_fact" => Ok(MemoryType::DomainFact),
        "task" => Ok(MemoryType::Task),
        "plan" => Ok(MemoryType::Plan),
        "implementation" => Ok(MemoryType::Implementation),
        "user" => Ok(MemoryType::User),
        "feedback" => Ok(MemoryType::Feedback),
        "project" => Ok(MemoryType::Project),
        "reference" => Ok(MemoryType::Reference),
        _ => anyhow::bail!(
            "unknown memory type '{value}'; expected one of: architecture, convention, \
             decision, incident, debugging, environment, domain_fact, task, plan, implementation, \
             user, feedback, project, reference"
        ),
    }
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

#[allow(clippy::too_many_arguments)]
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
struct PlanExecutionFinishReport {
    project: String,
    thread_key: String,
    plan_title: String,
    total_items: usize,
    completed_items: usize,
    completed_item_texts: Vec<String>,
    remaining_items: Vec<String>,
    verified_complete: bool,
}

#[derive(Debug, Clone, Serialize)]
struct ImplementationMemoryPreview {
    summary: String,
    memory_type: mem_api::MemoryType,
    tags: Vec<String>,
    canonical_text: String,
}

#[derive(Debug, Clone, Serialize)]
struct ImplementationMemoryResult {
    recorded: bool,
    summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    preview: Option<ImplementationMemoryPreview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    capture: Option<mem_api::CaptureTaskResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    curate: Option<CurateResponse>,
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

#[allow(clippy::too_many_arguments)]
fn build_plan_execution_request(
    project: &str,
    writer: &WriterIdentity,
    title: &str,
    thread_key: &str,
    plan_markdown: &str,
    source_path: Option<&Path>,
    repo_root: &Path,
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
    if let Some(source_path) = source_path
        && let Some(source_path) = durable_plan_source_path(source_path, repo_root)
    {
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

fn build_task_start_idempotency_key(
    project: &str,
    thread_key: &str,
    title: &str,
    prompt: &str,
    git_head: Option<&str>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"task-start");
    hasher.update(project.as_bytes());
    hasher.update(thread_key.as_bytes());
    hasher.update(title.trim().as_bytes());
    hasher.update(prompt.trim().as_bytes());
    if let Some(git_head) = git_head.map(str::trim).filter(|value| !value.is_empty()) {
        hasher.update(git_head.as_bytes());
    }
    format!("task-start:{:x}", hasher.finalize())
}

fn build_task_start_canonical_text(
    project: &str,
    title: &str,
    prompt: &str,
    thread_key: &str,
    git_head: Option<&str>,
) -> String {
    let mut lines = vec![
        format!("# Task: {}", title.trim()),
        String::new(),
        "Status: started".to_string(),
        format!("Project: {project}"),
        format!("Thread: {thread_key}"),
    ];
    if let Some(git_head) = git_head.map(str::trim).filter(|value| !value.is_empty()) {
        lines.push(format!("Git head: {git_head}"));
    }
    lines.extend([
        String::new(),
        "Original user request:".to_string(),
        prompt.trim().to_string(),
    ]);
    lines.join("\n")
}

fn build_task_start_request(
    project: &str,
    writer: &WriterIdentity,
    title: &str,
    prompt: &str,
    thread_key: &str,
    git_head: Option<&str>,
) -> CaptureTaskRequest {
    let canonical_text =
        build_task_start_canonical_text(project, title, prompt, thread_key, git_head);
    CaptureTaskRequest {
        project: project.to_string(),
        task_title: format!("Task started: {}", title.trim()),
        user_prompt: prompt.trim().to_string(),
        writer_id: writer.id.clone(),
        writer_name: writer.name.clone(),
        agent_summary: format!("Started direct no-plan task: {}", title.trim()),
        files_changed: Vec::new(),
        git_diff_summary: git_head.map(|head| format!("Task started from git HEAD {head}")),
        tests: Vec::new(),
        notes: Vec::new(),
        structured_candidates: vec![mem_api::CaptureCandidateInput {
            canonical_text,
            summary: title.trim().to_string(),
            memory_type: mem_api::MemoryType::Task,
            confidence: 0.95,
            importance: 3,
            tags: vec![
                "task".to_string(),
                format!("task-thread:{thread_key}"),
                "direct-execution".to_string(),
                "no-approved-plan".to_string(),
            ],
            sources: vec![
                mem_api::CaptureCandidateSourceInput {
                    file_path: None,
                    source_kind: mem_api::SourceKind::TaskPrompt,
                    excerpt: Some(prompt.trim().to_string()),
                },
                mem_api::CaptureCandidateSourceInput {
                    file_path: None,
                    source_kind: mem_api::SourceKind::Note,
                    excerpt: Some("Direct no-plan task entered execution.".to_string()),
                },
            ],
        }],
        command_output: None,
        idempotency_key: Some(build_task_start_idempotency_key(
            project, thread_key, title, prompt, git_head,
        )),
        dry_run: false,
    }
}

async fn verify_task_start_memory(
    api: &ApiClient,
    project: &str,
    thread_key: &str,
) -> Result<mem_api::ProjectMemoryListItem> {
    let thread_tag = format!("task-thread:{thread_key}");
    let memories = api.project_memories(project).await?;
    memories
        .items
        .into_iter()
        .find(|item| {
            item.status == mem_api::MemoryStatus::Active
                && item.memory_type == mem_api::MemoryType::Task
                && item.tags.iter().any(|tag| tag == &thread_tag)
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "task start capture was written, but no active `task` memory with tag `{thread_tag}` exists. Run `memory curate --project {project}` or review queued replacement proposals, then retry."
            )
        })
}

fn implementation_sources(
    prompt: &str,
    notes: &[String],
    files_changed: &[String],
    tests: &[TestResult],
    command_output: Option<&str>,
) -> Vec<mem_api::CaptureCandidateSourceInput> {
    let mut sources = Vec::new();
    if !prompt.trim().is_empty() {
        sources.push(mem_api::CaptureCandidateSourceInput {
            file_path: None,
            source_kind: mem_api::SourceKind::TaskPrompt,
            excerpt: Some(prompt.trim().to_string()),
        });
    }
    for note in notes {
        let trimmed = note.trim();
        if trimmed.is_empty() {
            continue;
        }
        sources.push(mem_api::CaptureCandidateSourceInput {
            file_path: None,
            source_kind: mem_api::SourceKind::Note,
            excerpt: Some(trimmed.to_string()),
        });
    }
    for file in files_changed {
        sources.push(mem_api::CaptureCandidateSourceInput {
            file_path: Some(file.clone()),
            source_kind: mem_api::SourceKind::File,
            excerpt: Some(format!("Changed file during task: {file}")),
        });
    }
    for test in tests {
        sources.push(mem_api::CaptureCandidateSourceInput {
            file_path: None,
            source_kind: mem_api::SourceKind::Test,
            excerpt: Some(format!("{}: {}", test.command, test.status)),
        });
    }
    if let Some(output) = command_output
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        sources.push(mem_api::CaptureCandidateSourceInput {
            file_path: None,
            source_kind: mem_api::SourceKind::CommandOutput,
            excerpt: Some(output.to_string()),
        });
    }
    sources
}

fn normalize_sentence_fragment(input: &str) -> String {
    let mut value = input.trim().replace('\n', " ");
    while value.contains("  ") {
        value = value.replace("  ", " ");
    }
    if value.is_empty() {
        return value;
    }
    if !value.ends_with('.') {
        value.push('.');
    }
    value
}

fn build_implementation_canonical_text(
    title: &str,
    summary: &str,
    implemented_items: &[String],
    notes: &[String],
) -> String {
    let mut sections = vec![normalize_sentence_fragment(summary)];
    if !title.trim().is_empty() {
        sections.push(format!("Plan: {}.", title.trim()));
    }
    if !implemented_items.is_empty() {
        sections.push(format!(
            "Implemented items:\n{}",
            implemented_items
                .iter()
                .map(|item| format!("- {}", item.trim()))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    let cleaned_notes = notes
        .iter()
        .map(|note| note.trim())
        .filter(|note| !note.is_empty())
        .collect::<Vec<_>>();
    if !cleaned_notes.is_empty() {
        sections.push(format!(
            "Implementation notes:\n{}",
            cleaned_notes
                .iter()
                .map(|note| format!("- {note}"))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    sections.join("\n\n")
}

fn build_remember_implementation_candidate(
    summary: &str,
    prompt: &str,
    notes: &[String],
    files_changed: &[String],
    tests: &[TestResult],
    command_output: Option<&str>,
) -> mem_api::CaptureCandidateInput {
    let canonical_text = build_implementation_canonical_text("", summary, &[], notes);
    let mut tags = vec!["implementation".to_string(), "implemented".to_string()];
    for file in files_changed {
        if let Some(prefix) = file.split('/').next().filter(|prefix| !prefix.is_empty()) {
            tags.push(prefix.to_string());
        }
    }
    tags.sort();
    tags.dedup();

    mem_api::CaptureCandidateInput {
        canonical_text,
        summary: summary.trim().to_string(),
        memory_type: mem_api::MemoryType::Implementation,
        confidence: if tests.iter().any(|test| test.status == "passed") {
            0.9
        } else {
            0.8
        },
        importance: if !tests.is_empty() || !files_changed.is_empty() {
            3
        } else {
            2
        },
        tags,
        sources: implementation_sources(prompt, notes, files_changed, tests, command_output),
    }
}

fn derive_finish_execution_implementation_summary(
    explicit_summary: Option<&str>,
    report: &PlanExecutionFinishReport,
) -> String {
    if let Some(summary) = explicit_summary
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return summary.to_string();
    }
    match report.completed_items {
        0 => format!("Completed {}", report.plan_title),
        1 => report
            .completed_item_texts
            .first()
            .cloned()
            .unwrap_or_else(|| format!("Completed {}", report.plan_title)),
        _ => format!(
            "Implemented {} items for {}",
            report.completed_items, report.plan_title
        ),
    }
}

fn build_finish_execution_implementation_idempotency_key(
    project: &str,
    report: &PlanExecutionFinishReport,
    summary: &str,
    notes: &[String],
    git_head: Option<&str>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"implementation-finish");
    hasher.update(project.as_bytes());
    hasher.update(report.thread_key.as_bytes());
    hasher.update(report.plan_title.as_bytes());
    hasher.update(summary.as_bytes());
    for item in &report.completed_item_texts {
        hasher.update(item.as_bytes());
    }
    for note in notes {
        hasher.update(note.trim().as_bytes());
    }
    if let Some(git_head) = git_head.map(str::trim).filter(|value| !value.is_empty()) {
        hasher.update(git_head.as_bytes());
    }
    format!("implementation-finish:{:x}", hasher.finalize())
}

fn build_finish_execution_implementation_request(
    project: &str,
    writer: &WriterIdentity,
    report: &PlanExecutionFinishReport,
    summary: &str,
    notes: &[String],
    git_head: Option<&str>,
) -> CaptureTaskRequest {
    let canonical_text = build_implementation_canonical_text(
        &report.plan_title,
        summary,
        &report.completed_item_texts,
        notes,
    );
    let mut tags = vec![
        "implementation".to_string(),
        "implemented".to_string(),
        format!("plan-thread:{}", report.thread_key),
    ];
    tags.sort();
    tags.dedup();

    CaptureTaskRequest {
        project: project.to_string(),
        task_title: format!("Implemented: {}", report.plan_title),
        user_prompt: format!(
            "Verified completed implementation for plan {} in project {}.",
            report.plan_title, project
        ),
        writer_id: writer.id.clone(),
        writer_name: writer.name.clone(),
        agent_summary: summary.to_string(),
        files_changed: Vec::new(),
        git_diff_summary: git_head
            .map(|head| format!("Implementation verified from git HEAD {head}")),
        tests: Vec::new(),
        notes: Vec::new(),
        structured_candidates: vec![mem_api::CaptureCandidateInput {
            canonical_text: canonical_text.clone(),
            summary: summary.to_string(),
            memory_type: mem_api::MemoryType::Implementation,
            confidence: 0.95,
            importance: 4,
            tags,
            sources: implementation_sources(
                &format!(
                    "Verified completed implementation for plan {} in project {}.",
                    report.plan_title, project
                ),
                notes,
                &[],
                &[],
                None,
            ),
        }],
        command_output: None,
        idempotency_key: Some(build_finish_execution_implementation_idempotency_key(
            project, report, summary, notes, git_head,
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
    let completed_item_texts = items
        .iter()
        .filter(|item| item.checked)
        .map(|item| item.text.clone())
        .collect::<Vec<_>>();
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
        completed_item_texts,
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
        embedding_spaces: Vec::new(),
        project: String::new(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        canonical_id: memory_id,
        version_no: 1,
        is_tombstone: false,
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
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::Mutex,
        time::Duration,
    };

    use clap::{Command, CommandFactory, Parser, error::ErrorKind};
    use uuid::Uuid;

    use super::{
        Cli, DEV_API_TOKEN, EvalRunContext, PlanExecutionFinishReport, RememberArgs,
        SERVICE_API_TOKEN_KEY, ServiceApiTokenAction, TuiRestartMarker, WatcherCommand,
        WatcherManagerArgs, WatcherManagerCommand, agent_build_prompt,
        build_finish_execution_implementation_request, build_graph_activity_request,
        build_plan_execution_finish_report, build_plan_execution_request, build_remember_request,
        build_task_start_request, chat_completion_content, derive_plan_thread_key,
        derive_plan_title, durable_plan_source_path, ensure_checkbox_plan,
        ensure_direct_llm_eval_config, ensure_shared_service_api_token, initialize_repo,
        is_placeholder_database_url, mask_database_url, newest_tui_restart_notice,
        parse_memory_type_arg, parse_no_memory_grounded_answer, parse_plan_checkboxes,
        render_agent_project_config, render_claude_md_memory_section, repair_repo_bootstrap,
        resolve_project_slug, resolve_repo_root, resolve_writer_identity,
        root_gitignore_contains_mem, shared_env_lookup, should_start_agent_watcher,
        token_usage_from_chat_body, watcher_command_requires_config_load, write_file_if_changed,
        write_headers,
    };
    use mem_api::AppConfig;

    #[cfg(target_os = "macos")]
    use chrono::Utc;

    #[cfg(target_os = "macos")]
    use mem_agenttop::{AgentSession, SessionStatus as AgentSessionStatus};

    #[cfg(target_os = "macos")]
    use super::{
        backend_launch_agent_label, default_global_config_path, managed_watch_launch_agent_label,
        render_backend_launch_agent, render_managed_watch_launch_agent, render_watch_launch_agent,
        render_watch_manager_launch_agent, sanitize_service_fragment, watch_launch_agent_label,
        watch_manager_launch_agent_label,
    };

    #[cfg(not(target_os = "macos"))]
    use super::{parse_systemd_unit_names, render_watch_unit, watch_unit_name};

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
            let after_help = cmd
                .get_after_help()
                .map(|value| value.to_string())
                .or_else(|| cmd.get_after_long_help().map(|value| value.to_string()));
            assert!(
                after_help
                    .as_deref()
                    .map(|value| value.contains("Agent notes:") || value.contains("Agent contract:"))
                    .unwrap_or(false),
                "command {path} is missing agent-oriented after_help"
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
        assert!(output.contains("Agent contract:"));
        assert!(output.contains("Prefer --json"));
        assert!(output.contains("checkpoint start-execution"));
        assert!(output.contains("checkpoint start-task"));
        assert!(output.contains("checkpoint finish-execution"));
        assert!(output.contains("Examples:"));
        assert!(output.contains("docs/user/README.md"));
        assert!(output.contains("Ask a project-specific question against curated memory"));
    }

    #[test]
    fn grouped_help_includes_subcommand_descriptions() {
        let output = rendered_help(&["memory", "service", "--help"]);
        assert!(output.contains("Manage the Memory Layer backend service"));
        assert!(output.contains("Run the backend service in the foreground"));
        assert!(
            output.contains("Restart active Memory Layer services after an install or upgrade")
        );
        assert!(output.contains("Agent notes:"));
        assert!(output.contains("browser web UI"));
        assert!(output.contains("127.0.0.1:4250"));
        assert!(output.contains("Examples:"));
        assert!(output.contains("docs/user/cli/service.md"));
    }

    #[test]
    fn leaf_help_includes_flag_help_examples_and_docs_hint() {
        let output = rendered_help(&["memory", "checkpoint", "start-execution", "--help"]);
        assert!(output.contains("Save a checkpoint and record the approved execution plan"));
        assert!(output.contains("Read the approved plan markdown from a file"));
        assert!(output.contains("Agent notes:"));
        assert!(output.contains("approved plan moves into implementation"));
        assert!(output.contains("Examples:"));
        assert!(output.contains("docs/user/cli/checkpoint.md"));
    }

    #[test]
    fn start_task_help_includes_agent_workflow_guidance() {
        let output = rendered_help(&["memory", "checkpoint", "start-task", "--help"]);
        assert!(output.contains("Record a direct no-plan task"));
        assert!(output.contains("Original user instruction"));
        assert!(output.contains("direct no-plan task"));
        assert!(output.contains("--dry-run --json"));
        assert!(output.contains("docs/user/cli/checkpoint.md"));
    }

    #[test]
    fn query_help_includes_argument_descriptions() {
        let output = rendered_help(&["memory", "query", "--help"]);
        assert!(output.contains("Natural-language question to answer from project memory"));
        assert!(output.contains("Restrict results to one or more memory types"));
        assert!(output.contains("Use before answering project-specific questions"));
        assert!(output.contains("insufficient_evidence"));
        assert!(output.contains("docs/user/cli/query.md"));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn systemd_unit_parser_finds_memory_watch_services() {
        let units = parse_systemd_unit_names(
            "memory-watch-manager.service loaded active running Manager\n\
             memory-watch-codex-abc.service loaded active running Watcher\n\
             ssh.service loaded active running SSH\n",
        );

        assert_eq!(
            units,
            vec![
                "memory-watch-manager.service".to_string(),
                "memory-watch-codex-abc.service".to_string()
            ]
        );
    }

    #[test]
    fn restart_notice_detects_newer_or_different_marker() {
        let dir =
            std::env::temp_dir().join(format!("memory-tui-restart-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let marker_path = dir.join("tui-restart-required.json");
        let startup_at = chrono::Utc::now();
        let marker = TuiRestartMarker {
            version: "9.9.9".to_string(),
            marked_at: startup_at - chrono::Duration::seconds(30),
            reason: "install-or-upgrade".to_string(),
            binary_path: "memory".to_string(),
            restarted_services: vec!["memory-layer.service".to_string()],
        };
        fs::write(&marker_path, serde_json::to_string(&marker).unwrap()).unwrap();

        let notice =
            newest_tui_restart_notice(startup_at, "0.1.0", vec![marker_path.clone()]).unwrap();

        assert_eq!(notice.marker_path, marker_path);
        assert_eq!(notice.version, "9.9.9");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn restart_notice_ignores_prod_marker_for_dev_tui() {
        let dir = std::env::temp_dir().join(format!(
            "memory-tui-restart-dev-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let marker_path = dir.join("tui-restart-required.json");
        let startup_at = chrono::Utc::now();
        let marker = TuiRestartMarker {
            version: "0.8.2".to_string(),
            marked_at: startup_at + chrono::Duration::seconds(30),
            reason: "install-or-upgrade".to_string(),
            binary_path: "memory".to_string(),
            restarted_services: vec!["memory-layer.service".to_string()],
        };
        fs::write(&marker_path, serde_json::to_string(&marker).unwrap()).unwrap();

        let notice = newest_tui_restart_notice(startup_at, "0.8.2-dev", vec![marker_path.clone()]);

        assert!(notice.is_none());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn packaging_hooks_restart_services_and_mark_tui_restart() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap();
        let postinst = fs::read_to_string(workspace.join("packaging/debian/postinst")).unwrap();
        let pkg = fs::read_to_string(workspace.join("packaging/build-pkg.sh")).unwrap();
        let formula = fs::read_to_string(workspace.join("Formula/memory-layer.rb")).unwrap();

        for contents in [postinst, pkg, formula] {
            assert!(contents.contains("service restart-all"));
            assert!(contents.contains("--mark-tui-restart"));
        }
    }

    #[test]
    fn agent_critical_help_mentions_current_surfaces() {
        let resume = rendered_help(&["memory", "resume", "--help"]);
        assert!(resume.contains("after interruptions"));
        assert!(resume.contains("--json"));

        let remember = rendered_help(&["memory", "remember", "--help"]);
        assert!(remember.contains("after meaningful completed work"));
        assert!(remember.contains("--file-changed"));

        let dev = rendered_help(&["memory", "dev", "init", "--help"]);
        assert!(dev.contains("127.0.0.1:4250"));
        assert!(dev.contains("--copy-from-global"));
        assert!(dev.contains("docs/developer/dev-stack.md"));

        let watcher_manager = rendered_help(&["memory", "watcher", "manager", "--help"]);
        assert!(watcher_manager.contains("Preferred automation surface"));
        assert!(watcher_manager.contains("one watcher per repo/session"));

        let embeddings_list = rendered_help(&["memory", "embeddings", "list", "--help"]);
        assert!(embeddings_list.contains("Active backends are marked with *"));
        assert!(embeddings_list.contains("avoid guessing backend names"));

        let embeddings_activate = rendered_help(&["memory", "embeddings", "activate", "--help"]);
        assert!(embeddings_activate.contains("does not recompute embeddings"));
        assert!(embeddings_activate.contains("memory embeddings list"));

        let history = rendered_help(&["memory", "prune-history", "--help"]);
        assert!(history.contains("Always use --dry-run and --json first"));
    }

    #[test]
    fn watcher_manager_run_requires_config_load() {
        let command = WatcherCommand::Manager(WatcherManagerArgs {
            command: WatcherManagerCommand::Run,
        });
        assert!(watcher_command_requires_config_load(&command));
        let status = WatcherCommand::Manager(WatcherManagerArgs {
            command: WatcherManagerCommand::Status,
        });
        assert!(!watcher_command_requires_config_load(&status));
    }

    #[test]
    fn remember_request_uses_defaults() {
        let request = build_remember_request(
            RememberArgs {
                project: None,
                title: None,
                memory_type: None,
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
        assert_eq!(request.structured_candidates.len(), 1);
        assert_eq!(
            request.structured_candidates[0].memory_type,
            mem_api::MemoryType::Implementation
        );
    }

    #[test]
    fn remember_accepts_file_alias_for_provenance() {
        let args = Cli::try_parse_from([
            "memory",
            "remember",
            "--project",
            "memory",
            "--note",
            "query match types",
            "--file",
            "crates/mem-search/src/lib.rs",
        ])
        .unwrap();

        let super::Command::Remember(args) = args.command else {
            panic!("expected remember command");
        };
        assert_eq!(args.files_changed, vec!["crates/mem-search/src/lib.rs"]);
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
            Path::new("/tmp/memory"),
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
    fn task_memory_type_parses_from_cli_args() {
        assert_eq!(
            parse_memory_type_arg("task").unwrap(),
            mem_api::MemoryType::Task
        );
    }

    #[test]
    fn task_start_request_uses_task_type_and_prompt_source() {
        let writer = super::WriterIdentity {
            id: "writer".to_string(),
            name: Some("Writer".to_string()),
        };
        let request = build_task_start_request(
            "memory",
            &writer,
            "Fix query input",
            "Make the query input easier to use.",
            "fix-query-input",
            Some("abc123"),
        );

        assert_eq!(request.task_title, "Task started: Fix query input");
        assert_eq!(request.user_prompt, "Make the query input easier to use.");
        assert_eq!(request.structured_candidates.len(), 1);
        assert!(
            request
                .idempotency_key
                .as_deref()
                .is_some_and(|key| key.starts_with("task-start:"))
        );
        let candidate = &request.structured_candidates[0];
        assert_eq!(candidate.memory_type, mem_api::MemoryType::Task);
        assert!(candidate.canonical_text.contains("# Task: Fix query input"));
        assert!(candidate.canonical_text.contains("Status: started"));
        assert!(candidate.canonical_text.contains("Git head: abc123"));
        assert!(candidate.tags.contains(&"task".to_string()));
        assert!(
            candidate
                .tags
                .contains(&"task-thread:fix-query-input".to_string())
        );
        assert!(candidate.tags.contains(&"direct-execution".to_string()));
        assert!(candidate.tags.contains(&"no-approved-plan".to_string()));
        assert!(candidate.sources.iter().any(|source| {
            source.source_kind == mem_api::SourceKind::TaskPrompt
                && source
                    .excerpt
                    .as_deref()
                    .is_some_and(|excerpt| excerpt.contains("query input"))
        }));
    }

    #[test]
    fn graph_activity_request_copies_extraction_report_counts() {
        let run_id = Uuid::new_v4();
        let report = mem_graph::GraphExtractionReport {
            project: "memory".to_string(),
            repo_root: "/repo".to_string(),
            git_head: Some("abc123".to_string()),
            since: Some("HEAD~1".to_string()),
            analyzer_version: "mem-analyze-v2".to_string(),
            strategy_version: "code-graph-resolution-v1".to_string(),
            extraction_run_id: Some(run_id),
            reused_existing_run: true,
            dry_run: false,
            index_reused: true,
            symbol_count: 10,
            reference_count: 20,
            resolved_reference_count: 12,
            unresolved_reference_count: 7,
            ambiguous_reference_count: 1,
            graph_node_count: 10,
            graph_edge_count: 9,
            evidence_count: 19,
            sample_unresolved_references: Vec::new(),
        };

        let request = build_graph_activity_request(&report);

        assert_eq!(request.project, "memory");
        assert_eq!(request.extraction_run_id, Some(run_id));
        assert!(request.reused_existing_run);
        assert_eq!(request.reference_count, 20);
        assert_eq!(request.graph_edge_count, 9);
        assert!(request.validate().is_ok());
    }

    #[test]
    fn durable_plan_source_path_keeps_repo_files_and_drops_outside_paths() {
        let repo_root = unique_temp_dir("mem-plan-source-repo");
        let plans_dir = repo_root.join("plans");
        fs::create_dir_all(&plans_dir).unwrap();
        let repo_plan = plans_dir.join("approved-plan.md");
        fs::write(&repo_plan, "# Plan\n\n- [ ] step").unwrap();

        let outside_root = unique_temp_dir("mem-plan-source-outside");
        fs::create_dir_all(&outside_root).unwrap();
        let outside_plan = outside_root.join("approved-plan.md");
        fs::write(&outside_plan, "# Plan\n\n- [ ] step").unwrap();

        assert_eq!(
            durable_plan_source_path(&repo_plan, &repo_root),
            Some(fs::canonicalize(&repo_plan).unwrap())
        );
        assert_eq!(durable_plan_source_path(&outside_plan, &repo_root), None);

        let _ = fs::remove_dir_all(repo_root);
        let _ = fs::remove_dir_all(outside_root);
    }

    #[test]
    fn plan_execution_request_omits_outside_repo_plan_file_source() {
        let repo_root = unique_temp_dir("mem-plan-request-repo");
        fs::create_dir_all(&repo_root).unwrap();
        let outside_root = unique_temp_dir("mem-plan-request-outside");
        fs::create_dir_all(&outside_root).unwrap();
        let outside_plan = outside_root.join("approved-plan.md");
        fs::write(&outside_plan, "# Plan\n\n- [ ] step").unwrap();
        let writer = super::WriterIdentity {
            id: "writer".to_string(),
            name: Some("Writer".to_string()),
        };

        let request = build_plan_execution_request(
            "memory",
            &writer,
            "Resume Redesign",
            "resume-redesign",
            "# Resume Redesign\n\n- [ ] step",
            Some(outside_plan.as_path()),
            &repo_root,
            Some("abc123"),
        );

        assert!(
            !request.structured_candidates[0]
                .sources
                .iter()
                .any(|source| source.source_kind == mem_api::SourceKind::File)
        );

        let _ = fs::remove_dir_all(repo_root);
        let _ = fs::remove_dir_all(outside_root);
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
    fn no_memory_grounded_answer_parser_accepts_strict_json() {
        let (answer, confidence, notes) = parse_no_memory_grounded_answer(
            "```json\n{\"answer\":\"Token usage is reported.\",\"confidence\":1.4}\n```",
        );

        assert_eq!(answer, "Token usage is reported.");
        assert_eq!(confidence, Some(1.0));
        assert!(notes.is_empty());
    }

    #[test]
    fn no_memory_grounded_answer_parser_falls_back_to_text() {
        let (answer, confidence, notes) = parse_no_memory_grounded_answer("I do not know.");

        assert_eq!(answer, "I do not know.");
        assert_eq!(confidence, None);
        assert_eq!(
            notes,
            vec!["plain_llm response was not strict answer/confidence JSON".to_string()]
        );
    }

    #[test]
    fn token_usage_from_chat_body_supports_openai_and_cache_fields() {
        let usage = token_usage_from_chat_body(
            r#"{
                "choices":[{"message":{"content":"ok"}}],
                "usage":{
                    "prompt_tokens":10,
                    "completion_tokens":5,
                    "cached_input_tokens":2,
                    "cache_creation_input_tokens":3
                }
            }"#,
        )
        .unwrap();

        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 5);
        assert_eq!(usage.cache_read_tokens, 2);
        assert_eq!(usage.cache_write_tokens, 3);
        assert_eq!(usage.total_tokens, 20);
    }

    #[test]
    fn token_usage_from_json_value_supports_codex_nested_events() {
        let value: serde_json::Value = serde_json::from_str(
            r#"{
                "type":"token_count",
                "msg":{
                    "total_token_usage":{
                        "input_tokens":100,
                        "output_tokens":25,
                        "cached_input_tokens":10,
                        "total_tokens":135
                    }
                }
            }"#,
        )
        .unwrap();

        let usage = crate::token_usage_from_json_value(&value).unwrap();

        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 25);
        assert_eq!(usage.cache_read_tokens, 10);
        assert_eq!(usage.total_tokens, 135);
    }

    #[test]
    fn chat_completion_content_rejects_missing_content() {
        let error = chat_completion_content(r#"{"choices":[{"message":{}}]}"#)
            .expect_err("missing content should fail");

        assert!(error.to_string().contains("missing content"));
    }

    #[test]
    fn agent_build_prompt_adds_memory_questions_for_memory_conditions() {
        let item = mem_eval::AgentBuildTaskItem {
            id: "build".to_string(),
            metadata: mem_eval::EvalItemMetadata::default(),
            project: Some("memory".to_string()),
            prompt: "Build the app.".to_string(),
            fixture: "fixtures/app".to_string(),
            agent_command: "codex exec - < {prompt_file}".to_string(),
            memory_questions: vec!["What changed recently?".to_string()],
            setup_commands: Vec::new(),
            score_commands: Vec::new(),
            timeout_seconds: 60,
            required_files: Vec::new(),
            forbidden_files: Vec::new(),
            required_content: Vec::new(),
        };
        let context = EvalRunContext {
            profile: mem_eval::EvalProfile::Llm,
            repeat_index: 0,
            run_group_id: Uuid::nil(),
            suite_checksum: None,
            dry_run: false,
            artifacts_root: PathBuf::from("target/memory-evals"),
            memory_command: "/tmp/memory".to_string(),
            memory_base_url: "http://127.0.0.1:4250".to_string(),
            memory_config_path: Some(PathBuf::from(".mem/config.toml")),
            llm_judge: false,
        };

        let prompt = agent_build_prompt(&item, mem_eval::EvalCondition::FullMemory, &context);

        assert!(prompt.contains("memory-enabled"));
        assert!(prompt.contains("/tmp/memory"));
        assert!(prompt.contains("What changed recently?"));
        assert!(prompt.contains("memory-evidence.md"));
    }

    #[test]
    fn agent_build_prompt_forbids_memory_for_no_memory_condition() {
        let item = mem_eval::AgentBuildTaskItem {
            id: "build".to_string(),
            metadata: mem_eval::EvalItemMetadata::default(),
            project: Some("memory".to_string()),
            prompt: "Build the app.".to_string(),
            fixture: "fixtures/app".to_string(),
            agent_command: "codex exec - < {prompt_file}".to_string(),
            memory_questions: vec!["What changed recently?".to_string()],
            setup_commands: Vec::new(),
            score_commands: Vec::new(),
            timeout_seconds: 60,
            required_files: Vec::new(),
            forbidden_files: Vec::new(),
            required_content: Vec::new(),
        };
        let context = EvalRunContext {
            profile: mem_eval::EvalProfile::Llm,
            repeat_index: 0,
            run_group_id: Uuid::nil(),
            suite_checksum: None,
            dry_run: false,
            artifacts_root: PathBuf::from("target/memory-evals"),
            memory_command: "/tmp/memory".to_string(),
            memory_base_url: "http://127.0.0.1:4250".to_string(),
            memory_config_path: None,
            llm_judge: false,
        };

        let prompt = agent_build_prompt(&item, mem_eval::EvalCondition::NoMemory, &context);

        assert!(prompt.contains("no-memory"));
        assert!(prompt.contains("Do not query"));
        assert!(prompt.contains("Do not create memory-evidence.md"));
        assert!(prompt.contains(".memory-eval"));
        assert!(!prompt.contains("What changed recently?"));
    }

    #[test]
    fn agent_build_memory_evidence_verifies_required_queries() {
        let workspace = std::env::temp_dir().join(format!("memory-eval-test-{}", Uuid::new_v4()));
        fs::create_dir_all(workspace.join(".memory-eval")).unwrap();
        fs::write(
            workspace.join(".memory-eval/q1.status.json"),
            r#"{"question_id":"q1","exit_code":0,"output_file":".memory-eval/q1.json"}"#,
        )
        .unwrap();
        fs::write(
            workspace.join(".memory-eval/q1.json"),
            r#"{"results":[{"memory_id":"00000000-0000-0000-0000-000000000000"}]}"#,
        )
        .unwrap();
        let item = mem_eval::AgentBuildTaskItem {
            id: "build".to_string(),
            metadata: mem_eval::EvalItemMetadata::default(),
            project: Some("memory".to_string()),
            prompt: "Build the app.".to_string(),
            fixture: "fixtures/app".to_string(),
            agent_command: "codex exec - < {prompt_file}".to_string(),
            memory_questions: vec!["What changed recently?".to_string()],
            setup_commands: Vec::new(),
            score_commands: Vec::new(),
            timeout_seconds: 60,
            required_files: Vec::new(),
            forbidden_files: Vec::new(),
            required_content: Vec::new(),
        };

        let evidence = crate::validate_agent_build_memory_evidence(
            &workspace,
            &item,
            mem_eval::EvalCondition::FullMemory,
        )
        .unwrap();

        assert!(evidence.ok);
        assert_eq!(evidence.required, 1);
        assert_eq!(evidence.verified, 1);
        fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn agent_build_no_memory_fails_on_memory_evidence_artifacts() {
        let workspace = std::env::temp_dir().join(format!("memory-eval-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&workspace).unwrap();
        fs::write(workspace.join("memory-evidence.md"), "query").unwrap();
        let item = mem_eval::AgentBuildTaskItem {
            id: "build".to_string(),
            metadata: mem_eval::EvalItemMetadata::default(),
            project: Some("memory".to_string()),
            prompt: "Build the app.".to_string(),
            fixture: "fixtures/app".to_string(),
            agent_command: "codex exec - < {prompt_file}".to_string(),
            memory_questions: vec!["What changed recently?".to_string()],
            setup_commands: Vec::new(),
            score_commands: Vec::new(),
            timeout_seconds: 60,
            required_files: Vec::new(),
            forbidden_files: Vec::new(),
            required_content: Vec::new(),
        };

        let evidence = crate::validate_agent_build_memory_evidence(
            &workspace,
            &item,
            mem_eval::EvalCondition::NoMemory,
        )
        .unwrap();

        assert!(!evidence.ok);
        assert!(evidence.notes[0].contains("forbidden Memory evidence"));
        fs::remove_dir_all(workspace).unwrap();
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
                embedding_spaces: Vec::new(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                canonical_id: Uuid::nil(),
                version_no: 1,
                is_tombstone: false,
            },
        )
        .expect("build finish report");

        assert_eq!(report.total_items, 2);
        assert_eq!(report.completed_items, 1);
        assert_eq!(report.completed_item_texts, vec!["done".to_string()]);
        assert!(!report.verified_complete);
        assert_eq!(report.remaining_items, vec!["remaining".to_string()]);
    }

    #[test]
    fn finish_execution_implementation_request_uses_completed_items() {
        let writer = super::WriterIdentity {
            id: "writer".to_string(),
            name: Some("Writer".to_string()),
        };
        let report = PlanExecutionFinishReport {
            project: "memory".to_string(),
            thread_key: "footer-fix".to_string(),
            plan_title: "Footer Fix".to_string(),
            total_items: 2,
            completed_items: 2,
            completed_item_texts: vec![
                "Add implementation memory type".to_string(),
                "Record finish-execution implementation outcomes".to_string(),
            ],
            remaining_items: Vec::new(),
            verified_complete: true,
        };

        let request = build_finish_execution_implementation_request(
            "memory",
            &writer,
            &report,
            "Implemented 2 items for Footer Fix",
            &["The memories view now shows implemented outcomes.".to_string()],
            Some("abc123"),
        );

        assert_eq!(request.structured_candidates.len(), 1);
        let candidate = &request.structured_candidates[0];
        assert_eq!(candidate.memory_type, mem_api::MemoryType::Implementation);
        assert!(candidate.tags.contains(&"implemented".to_string()));
        assert!(candidate.canonical_text.contains("Implemented items:"));
        assert!(
            candidate
                .canonical_text
                .contains("Add implementation memory type")
        );
        assert!(request.idempotency_key.is_some());
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
        if cfg!(target_os = "macos") {
            assert!(summary.contains("memory watcher enable --project memory"));
        } else {
            assert!(summary.contains("memory watcher manager enable"));
        }
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

    #[test]
    fn claude_memory_section_mentions_code_explanation_memory() {
        let content = render_claude_md_memory_section("memory");

        assert!(content.contains("Remember distilled code and codebase explanations"));
        assert!(content.contains("### Store code explanations"));
        assert!(content.contains("memory remember --project memory --type project"));
        assert!(content.contains("Do not store the full chat answer"));
        assert!(content.contains("Do not use `--file-changed` unless files actually changed"));
    }

    #[test]
    fn init_copies_code_explanation_memory_rule_to_skills() {
        let repo_root = unique_temp_dir("mem-init-explanation-memory");
        fs::create_dir_all(&repo_root).unwrap();

        initialize_repo(&repo_root, "memory", false, false).unwrap();

        let umbrella =
            fs::read_to_string(repo_root.join(".agents/skills/memory-layer/SKILL.md")).unwrap();
        let query_resume =
            fs::read_to_string(repo_root.join(".agents/skills/memory-query-resume/SKILL.md"))
                .unwrap();
        let remember =
            fs::read_to_string(repo_root.join(".agents/skills/memory-remember/SKILL.md")).unwrap();

        assert!(umbrella.contains("Code explanation memory rule"));
        assert!(umbrella.contains("Store a distilled memory, not the whole chat answer"));
        assert!(query_resume.contains("explain code, a file, a module"));
        assert!(query_resume.contains("store the distilled reusable explanation"));
        assert!(remember.contains("Remember a distilled code explanation"));
        assert!(remember.contains("--type project"));
        assert!(remember.contains("Do not use `--file-changed`"));

        let _ = fs::remove_dir_all(repo_root);
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

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn watch_manager_unit_uses_manager_subcommand() {
        let unit = super::render_watch_manager_unit(Path::new("/tmp/memory-layer.toml")).unwrap();
        assert!(unit.contains("watcher manager run"));
        assert!(unit.contains("Restart=always"));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn agent_watcher_start_logic_reuses_loaded_active_units() {
        assert!(!super::should_start_agent_watcher(true, true, true));
        assert!(super::should_start_agent_watcher(true, true, false));
        assert!(super::should_start_agent_watcher(true, false, false));
        assert!(super::should_start_agent_watcher(false, true, true));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn launch_agent_labels_are_project_scoped() {
        assert_eq!(backend_launch_agent_label(), "com.memory-layer.mem-service");
        assert_eq!(
            watch_manager_launch_agent_label(),
            "com.memory-layer.memory-watch-manager"
        );
        assert_eq!(
            watch_launch_agent_label("customer portal"),
            "com.memory-layer.memory-watch.customer-portal"
        );
        assert_eq!(
            managed_watch_launch_agent_label("session 123"),
            "com.memory-layer.memory-watch.codex.session-123"
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

    #[cfg(target_os = "macos")]
    #[test]
    fn watch_manager_launch_agent_uses_manager_subcommand() {
        let plist = render_watch_manager_launch_agent(&default_global_config_path()).unwrap();

        assert!(plist.contains("<string>com.memory-layer.memory-watch-manager</string>"));
        assert!(plist.contains("<string>/bin/zsh</string>"));
        assert!(plist.contains("watcher"));
        assert!(plist.contains("manager"));
        assert!(plist.contains("run"));
        assert!(plist.contains("memory-watch-manager.stdout.log"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn managed_watch_launch_agent_uses_agent_metadata() {
        let repo_root = unique_temp_dir("mem-managed-watch-launch-agent");
        fs::create_dir_all(&repo_root).unwrap();
        let session = LightweightAgentSession {
            agent_cli: "codex",
            pid: 42,
            session_id: "session-123".to_string(),
            cwd: repo_root.display().to_string(),
            started_at: Utc::now().timestamp_millis() as u64,
        };
        let plist = render_managed_watch_launch_agent(
            &repo_root,
            "homelab",
            &session,
            "2026-04-10T00:00:00Z",
            None,
        )
        .unwrap();

        assert!(plist.contains("<string>com.memory-layer.memory-watch.codex.session-123</string>"));
        assert!(plist.contains("--agent-session-id"));
        assert!(plist.contains("session-123"));
        assert!(plist.contains("--agent-pid"));
        assert!(plist.contains("42"));
        assert!(plist.contains("--repo-root"));
        assert!(plist.contains(&repo_root.display().to_string()));

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
    fn cluster_enabled_is_added_when_missing() {
        let dir = unique_temp_dir("mem-cluster-config");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("memory-layer.toml");
        fs::write(&path, "[service]\nbind_addr = \"127.0.0.1:4040\"\n").unwrap();

        super::set_cluster_enabled_in_shared_config(&path, true).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("[cluster]"));
        assert!(content.contains("enabled = true"));

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
    fn cluster_enabled_is_updated_in_existing_section() {
        let dir = unique_temp_dir("mem-cluster-existing");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("memory-layer.toml");
        fs::write(
            &path,
            "[cluster]\nenabled = false\npriority = 50\n\n[service]\nbind_addr = \"127.0.0.1:4040\"\n",
        )
        .unwrap();

        super::set_cluster_enabled_in_shared_config(&path, true).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("[cluster]\nenabled = true\npriority = 50"));

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

    #[test]
    fn direct_llm_eval_accepts_ollama_without_api_key() {
        let mut config = test_app_config();
        config.llm = mem_api::LlmConfig {
            provider: "ollama".to_string(),
            base_url: String::new(),
            api_key_env: "OPENAI_API_KEY".to_string(),
            model: "llama3.2".to_string(),
            ..mem_api::LlmConfig::default()
        };

        assert!(ensure_direct_llm_eval_config(&config).is_ok());
    }

    #[test]
    fn watcher_manager_start_check_uses_cached_tracked_state() {
        assert!(!should_start_agent_watcher(true, true, true));
        assert!(should_start_agent_watcher(false, true, true));
        assert!(should_start_agent_watcher(true, false, true));
        assert!(should_start_agent_watcher(true, true, false));
    }

    #[test]
    fn write_file_if_changed_skips_identical_content() {
        let dir = unique_temp_dir("mem-write-if-changed");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        write_file_if_changed(&path, b"same").unwrap();
        let first_modified = fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(Duration::from_millis(5));
        write_file_if_changed(&path, b"same").unwrap();
        let second_modified = fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(first_modified, second_modified);
        write_file_if_changed(&path, b"different").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "different");
        let _ = fs::remove_dir_all(&dir);
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
            embeddings: mem_api::EmbeddingsConfig::default(),
            cluster: mem_api::ClusterConfig::default(),
            writer: mem_api::WriterConfig::default(),
            automation: mem_api::AutomationConfig::default(),
            retention: mem_api::RetentionConfig::default(),
            profile: mem_api::Profile::Prod,
            resolved_config_path: None,
            resolved_dev_overlay_path: None,
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
