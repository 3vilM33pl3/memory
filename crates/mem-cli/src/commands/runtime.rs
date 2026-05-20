use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::{Args, Parser, Subcommand};
use clap_complete::Shell;
use mem_api::{AppConfig, Profile};
use mem_platform as platform;
use reqwest::Client;
use serde::Serialize;
use uuid::Uuid;

use crate::commands::{api::ApiClient, skill_support::set_private_file_permissions};

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
  memory status --project memory
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
  Mutates user-local project config plus the repo-local .mem marker and agent skill files unless --dry-run is passed.

Examples:
  memory init
  memory init --project memory --dry-run
  memory init --force

See also:
  docs/user/cli/init.md";

const UPGRADE_AFTER_HELP: &str = "\
Agent notes:
  Use after installing a newer Memory Layer package to refresh repo-local Memory skills.
  Backs up replaced skill directories under the user-local project runtime directory before writing.
  Prefer --dry-run --json before applying from automation.

Examples:
  memory upgrade --dry-run
  memory upgrade --dry-run --json
  memory upgrade
  memory upgrade --force

See also:
  docs/user/cli/upgrade.md";

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
  Mutates the user-local project dev overlay and dev runtime directory unless --dry-run is passed.

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

const MCP_GROUP_AFTER_HELP: &str = "\
Agent notes:
  Use mcp run from stdio MCP clients. It writes only MCP JSON-RPC frames to stdout.
  Use mcp status for a read-only check of service reachability and exposed MCP surface.
  HTTP MCP is mounted by memory service when [mcp].enabled and [mcp].http_enabled are true.

Examples:
  memory mcp run --project memory
  memory mcp status --project memory
  memory mcp status --project memory --json

See also:
  docs/user/cli/mcp.md";

const EVAL_GROUP_AFTER_HELP: &str = "\
Agent notes:
  Use eval commands to produce reproducible evidence that memory changes retrieval, grounding, cost, or task success.
  Prefer --dry-run before runs that would call the backend or execute task commands.
  JSON is the default output for automation; use --text for human summaries.

Examples:
  memory eval scaffold --project memory --out evals/suites/memory-smoke
  memory eval doctor --suite evals/examples/memory-smoke
  memory eval run --suite evals/examples/app-build-smoke --condition no-memory --condition full-memory --profile offline --allow-shell --text
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
  Suites with command_task, agent_build_task, or agent_build_sequence execute shell commands and require --allow-shell for real runs.
  agent_build_task items copy fixtures to target/memory-evals/build-runs and capture prompts, stdout, stderr, and scoring summaries.

Examples:
  memory eval run --suite evals/examples/memory-smoke --condition full-memory --dry-run
  memory eval run --suite evals/examples/app-build-smoke --condition no-memory --condition full-memory --profile offline --allow-shell --text
  memory eval run --suite evals/examples/memory-smoke --condition no-memory --condition full-memory --repeat 5
  memory eval run --suite evals/suites/memory-improvement-v1 --condition no-memory --condition full-memory --allow-shell --llm-judge --repeat 5

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

const VERIFY_PROVENANCE_AFTER_HELP: &str = "\
Agent notes:
  Use before relying on source citations after large refactors, file moves, or cleanup.
  Prefer --dry-run --json first; omit --dry-run only when you want the verification results stored.

Examples:
  memory verify-provenance --project memory --dry-run --json
  memory verify-provenance --project memory --repo-root . --json
  memory verify-provenance --project memory

See also:
  docs/user/cli/verify-provenance.md";

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
  Use --type user, feedback, project, reference, implementation, or refactor when classification should be explicit.

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

const PROPOSALS_GROUP_AFTER_HELP: &str = "\
Agent notes:
  Review pending memory replacement proposals produced by curation.
  List/show are read-only. Approve/reject mutate memory state and require an explicit review decision.

Examples:
  memory proposals list --project memory --json
  memory proposals show --project memory --id 00000000-0000-0000-0000-000000000000
  memory proposals approve --project memory --id 00000000-0000-0000-0000-000000000000 --json

See also:
  docs/user/cli/proposals.md";

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

const STATUS_AFTER_HELP: &str = "\
Agent notes:
  Recommended first diagnostic command for users and agents.
  Aggregates service reachability, config, watcher state, MCP status, skill bundle checks, and doctor diagnostics.
  JSON mode keeps the aggregate payload on stdout; any future progress or warnings must go to stderr.

Examples:
  memory status --project memory
  memory status --project memory --json

See also:
  docs/user/cli/status.md";

const HEALTH_AFTER_HELP: &str = "\
Agent notes:
  Compatibility read-only backend health check. Prefer status for first diagnosis and doctor for full environment repair guidance.

Examples:
  memory health
  memory status --project memory
  memory stats

See also:
  docs/user/cli/health.md";

const STATS_AFTER_HELP: &str = "\
Agent notes:
  Compatibility read-only memory and project summary. Prefer status when diagnosing an install.

Examples:
  memory stats
  memory status --project memory
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

const COMPLETION_AFTER_HELP: &str = "\
Agent notes:
  Read-only command that prints shell completion scripts to stdout.
  Package installers normally install completions automatically; use this for manual setup or debugging.

Examples:
  memory completion bash > ~/.local/share/bash-completion/completions/memory
  memory completion zsh > ~/.zfunc/_memory
  memory completion fish > ~/.config/fish/completions/memory.fish

See also:
  docs/user/cli/completion.md";

#[derive(Debug, Parser)]
#[command(
    name = "memory",
    version,
    about = "Project memory CLI for setup, retrieval, capture, curation, and operations.",
    after_help = ROOT_AFTER_HELP
)]
pub(in crate::commands) struct Cli {
    /// Use a specific config file instead of the discovered default.
    #[arg(long, env = "MEMORY_LAYER_CONFIG")]
    pub(crate) config: Option<PathBuf>,
    /// Override the writer identity used for write-capable commands.
    #[arg(
        long = "writer-id",
        visible_alias = "agent-id",
        env = "MEMORY_LAYER_WRITER_ID"
    )]
    pub(crate) writer_id: Option<String>,
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(in crate::commands) enum Command {
    #[command(about = "Run the interactive setup wizard.", after_help = WIZARD_AFTER_HELP)]
    Wizard(WizardArgs),
    #[command(about = "Bootstrap a repo-local Memory Layer setup.", after_help = INIT_AFTER_HELP)]
    Init(InitArgs),
    #[command(about = "Upgrade repo-local Memory Layer skill files.", after_help = UPGRADE_AFTER_HELP)]
    Upgrade(UpgradeArgs),
    #[command(about = "Manage the Memory Layer backend service.", after_help = SERVICE_GROUP_AFTER_HELP)]
    Service(ServiceArgs),
    #[command(about = "Run and inspect the built-in Memory MCP server.", after_help = MCP_GROUP_AFTER_HELP)]
    Mcp(McpArgs),
    #[command(about = "Manage project watchers and watcher daemons.", after_help = WATCHER_GROUP_AFTER_HELP)]
    Watcher(WatcherArgs),
    #[command(about = "Inspect configuration and environment health.", after_help = DOCTOR_AFTER_HELP)]
    Doctor(DoctorArgs),
    #[command(about = "Show the aggregate Memory Layer diagnostic status.", after_help = STATUS_AFTER_HELP)]
    Status(StatusArgs),
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
    #[command(about = "Verify memory source provenance against the filesystem.", after_help = VERIFY_PROVENANCE_AFTER_HELP)]
    VerifyProvenance(VerifyProvenanceArgs),
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
    #[command(about = "Review pending memory replacement proposals.", after_help = PROPOSALS_GROUP_AFTER_HELP)]
    Proposals(ProposalsArgs),
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
    #[command(about = "Generate shell completion scripts.", after_help = COMPLETION_AFTER_HELP)]
    Completion(CompletionArgs),
    #[command(about = "Scaffold and inspect the dev-profile overlay.", after_help = DEV_GROUP_AFTER_HELP)]
    Dev(DevArgs),
}

#[derive(Debug, Args)]
pub(in crate::commands) struct CompletionArgs {
    /// Shell to generate completions for.
    #[arg(value_enum)]
    pub(crate) shell: Shell,
}

#[derive(Debug, Args)]
pub(in crate::commands) struct StatusArgs {
    /// Project slug to inspect; defaults to repo metadata or current directory name.
    #[arg(long)]
    pub(crate) project: Option<String>,
    /// Emit the aggregate status report as JSON.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(in crate::commands) struct DevArgs {
    #[command(subcommand)]
    pub(crate) command: DevCommand,
}

#[derive(Debug, Subcommand)]
pub(in crate::commands) enum DevCommand {
    /// Create the user-local project dev overlay and dev runtime directory.
    #[command(after_help = DEV_INIT_AFTER_HELP)]
    Init(DevInitArgs),
}

#[derive(Debug, Args)]
pub(in crate::commands) struct DevInitArgs {
    /// Overwrite an existing dev overlay instead of preserving it.
    #[arg(long)]
    pub(crate) force: bool,
    /// Print what would be written without touching the filesystem.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Address the dev service should bind. Defaults to `127.0.0.1:4250`.
    #[arg(long, default_value = "127.0.0.1:4250")]
    pub(crate) bind_addr: String,
    /// Cap'n Proto TCP address for the dev service. Defaults to `127.0.0.1:4251`.
    #[arg(long, default_value = "127.0.0.1:4251")]
    pub(crate) capnp_tcp_addr: String,
    /// Copy database URL and LLM/embedding endpoints from the global config
    /// into the dev overlay. Without this flag and without a TTY, nothing is
    /// copied. With a TTY, the command asks interactively.
    #[arg(long)]
    pub(crate) copy_from_global: bool,
    /// Skip the interactive prompt and leave shared settings out of the overlay.
    #[arg(long, conflicts_with = "copy_from_global")]
    pub(crate) no_copy_from_global: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Run the interactive setup wizard for global or repo-local Memory Layer configuration.",
    after_help = WIZARD_AFTER_HELP
)]
pub(in crate::commands) struct WizardArgs {
    /// Override the project slug used for repo-local setup.
    #[arg(long)]
    pub(crate) project: Option<String>,
    /// Edit shared machine-level configuration instead of only the current repo.
    #[arg(long)]
    pub(crate) global: bool,
    /// Preview the wizard's file and service actions without applying them.
    #[arg(long)]
    pub(crate) dry_run: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Create or refresh the repo-local Memory Layer bootstrap files.",
    after_help = INIT_AFTER_HELP
)]
pub(in crate::commands) struct InitArgs {
    /// Override the project slug written into the repo-local bootstrap files.
    #[arg(long)]
    pub(crate) project: Option<String>,
    /// Replace existing managed bootstrap files instead of preserving them.
    #[arg(long)]
    pub(crate) force: bool,
    /// Preview the files and skill bundle paths that would be written.
    #[arg(long)]
    pub(crate) dry_run: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Upgrade repo-local Memory Layer skill files from the installed template.",
    after_help = UPGRADE_AFTER_HELP
)]
pub(in crate::commands) struct UpgradeArgs {
    /// Replace all known Memory skills, including newer or same-version local copies.
    #[arg(long)]
    pub(crate) force: bool,
    /// Preview skill changes without touching the filesystem.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Emit a structured JSON report.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Manage the Memory Layer backend service for local or packaged installs.",
    after_help = SERVICE_GROUP_AFTER_HELP
)]
pub(in crate::commands) struct ServiceArgs {
    #[command(subcommand)]
    pub(crate) command: ServiceCommand,
}

#[derive(Debug, Subcommand)]
pub(in crate::commands) enum ServiceCommand {
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
pub(in crate::commands) struct ServiceLifecycleArgs {
    /// Preview the service manager actions without changing service state.
    #[arg(long)]
    pub(crate) dry_run: bool,
}

#[derive(Debug, Args)]
pub(in crate::commands) struct ServiceRestartAllArgs {
    /// Preview active service discovery and restart actions without changing service state.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Emit the restart report as JSON.
    #[arg(long)]
    pub(crate) json: bool,
    /// Write a TUI restart marker after restart planning/execution.
    #[arg(long)]
    pub(crate) mark_tui_restart: bool,
}

#[derive(Debug, Args)]
pub(in crate::commands) struct ServiceEnsureApiTokenArgs {
    /// Operate on the shared machine-level env file instead of a repo-local override.
    #[arg(long)]
    pub(crate) shared: bool,
    /// Replace the development placeholder token if it is still configured.
    #[arg(long)]
    pub(crate) rotate_placeholder: bool,
    /// Preview the env-file change without writing it.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Emit the result as JSON.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Run and inspect the built-in Memory MCP server.",
    after_help = MCP_GROUP_AFTER_HELP
)]
pub(in crate::commands) struct McpArgs {
    #[command(subcommand)]
    pub(crate) command: McpCommand,
}

#[derive(Debug, Subcommand)]
pub(in crate::commands) enum McpCommand {
    #[command(about = "Run a stdio MCP server for local clients.", after_help = MCP_GROUP_AFTER_HELP)]
    Run(McpRunArgs),
    #[command(about = "Check service reachability and exposed MCP surface.", after_help = MCP_GROUP_AFTER_HELP)]
    Status(McpStatusArgs),
}

#[derive(Debug, Args)]
pub(in crate::commands) struct McpRunArgs {
    /// Default project slug for stdio tool calls that omit the project argument.
    #[arg(long)]
    pub(crate) project: Option<String>,
}

#[derive(Debug, Args)]
pub(in crate::commands) struct McpStatusArgs {
    /// Project slug to verify; defaults to the current repo when available.
    #[arg(long)]
    pub(crate) project: Option<String>,
    /// Emit the status report as JSON.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Inspect configuration, connectivity, watchers, and skill runtime prerequisites.",
    after_help = DOCTOR_AFTER_HELP
)]
pub(in crate::commands) struct DoctorArgs {
    /// Limit checks to one project context instead of the inferred current repo.
    #[arg(long)]
    pub(crate) project: Option<String>,
    /// Attempt automatic repairs for supported problems.
    #[arg(long)]
    pub(crate) fix: bool,
    /// Emit the diagnostic report as JSON.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Manage project watcher daemons and watcher registration.",
    after_help = WATCHER_GROUP_AFTER_HELP
)]
pub(in crate::commands) struct WatcherArgs {
    #[command(subcommand)]
    pub(crate) command: WatcherCommand,
}

#[derive(Debug, Subcommand)]
pub(in crate::commands) enum WatcherCommand {
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
pub(in crate::commands) struct WatchProjectArgs {
    /// Project slug to inspect; defaults to the current repo when available.
    #[arg(long)]
    pub(crate) project: Option<String>,
}

#[derive(Debug, Args)]
pub(in crate::commands) struct WatcherManageArgs {
    /// Project slug to manage; defaults to the current repo when available.
    #[arg(long)]
    pub(crate) project: Option<String>,
    /// Preview the watcher service action without applying it.
    #[arg(long)]
    pub(crate) dry_run: bool,
}

#[derive(Debug, Args)]
pub(in crate::commands) struct WatcherRunCliArgs {
    /// Project slug to watch; defaults to the current repo when available.
    #[arg(long)]
    pub(crate) project: Option<String>,
    /// Override the repository root used for watcher state and file detection.
    #[arg(long)]
    pub(crate) repo_root: Option<PathBuf>,
    /// Owning agent CLI name for agent-linked watcher mode.
    #[arg(long)]
    pub(crate) agent_cli: Option<String>,
    /// Owning agent session id for agent-linked watcher mode.
    #[arg(long)]
    pub(crate) agent_session_id: Option<String>,
    /// Owning agent pid for agent-linked watcher mode.
    #[arg(long)]
    pub(crate) agent_pid: Option<u32>,
    /// Owning agent started-at timestamp for agent-linked watcher mode.
    #[arg(long)]
    pub(crate) agent_started_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Args)]
pub(in crate::commands) struct WatcherManagerArgs {
    #[command(subcommand)]
    pub(crate) command: WatcherManagerCommand,
}

#[derive(Debug, Subcommand)]
pub(in crate::commands) enum WatcherManagerCommand {
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
pub(in crate::commands) struct QueryArgs {
    /// Project slug to query.
    #[arg(long)]
    pub(crate) project: String,
    /// Natural-language question to answer from project memory.
    #[arg(long)]
    pub(crate) question: String,
    /// Restrict results to one or more memory types.
    #[arg(long = "type")]
    pub(crate) types: Vec<String>,
    /// Restrict results to one or more tags.
    #[arg(long = "tag")]
    pub(crate) tags: Vec<String>,
    /// Maximum number of memories to retrieve before answer synthesis.
    #[arg(long, default_value_t = 8)]
    pub(crate) limit: i64,
    /// Ignore memories below this confidence threshold.
    #[arg(long)]
    pub(crate) min_confidence: Option<f32>,
    /// Bypass provenance-based stale-source de-ranking.
    #[arg(long)]
    pub(crate) include_stale: bool,
    /// Include every historical version of each memory (including
    /// tombstones from deleted memories) in the search space. Default is
    /// latest-version-only.
    #[arg(long)]
    pub(crate) history: bool,
    /// Emit the query result as JSON.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Verify memory source provenance against the filesystem.",
    after_help = VERIFY_PROVENANCE_AFTER_HELP
)]
pub(in crate::commands) struct VerifyProvenanceArgs {
    /// Project slug whose memory sources should be verified.
    #[arg(long)]
    pub(crate) project: Option<String>,
    /// Repository root used to resolve relative source paths.
    #[arg(long)]
    pub(crate) repo_root: Option<PathBuf>,
    /// Check provenance without storing verification results.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Emit the verification response as JSON.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Show the full version history for a memory, including tombstones.",
    after_help = HISTORY_AFTER_HELP
)]
pub(in crate::commands) struct HistoryArgs {
    /// Any version's id (including a tombstone). The chain resolves via
    /// canonical_id so passing any version id returns the same history.
    pub(crate) memory_id: Uuid,
    /// Emit the result as JSON.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Prune tombstoned canonical memories and superseded versions older than the configured thresholds.",
    after_help = PRUNE_HISTORY_AFTER_HELP
)]
pub(in crate::commands) struct PruneHistoryArgs {
    /// Limit the sweep to one project. Defaults to every project in the DB.
    #[arg(long)]
    pub(crate) project: Option<String>,
    /// Duration (e.g. 30d, 12h) after which a tombstoned canonical's rows
    /// are deleted entirely. Overrides config.retention.tombstone_after.
    #[arg(long, value_parser = humantime::parse_duration)]
    pub(crate) tombstone_after: Option<std::time::Duration>,
    /// Duration after which non-latest, non-tombstone versions are
    /// deleted. Overrides config.retention.superseded_after.
    #[arg(long, value_parser = humantime::parse_duration)]
    pub(crate) superseded_after: Option<std::time::Duration>,
    /// Preview counts without touching the database.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Emit the result as JSON.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Import and inspect git commit history for a project.",
    after_help = COMMITS_GROUP_AFTER_HELP
)]
pub(in crate::commands) struct CommitsArgs {
    #[command(subcommand)]
    pub(crate) command: CommitsCommand,
}

#[derive(Debug, Subcommand)]
pub(in crate::commands) enum CommitsCommand {
    #[command(about = "Import git commits into the project backend.", after_help = COMMITS_SYNC_AFTER_HELP)]
    Sync(CommitSyncArgs),
    #[command(about = "List imported commits for a project.", after_help = COMMITS_LIST_AFTER_HELP)]
    List(CommitListArgs),
    #[command(about = "Show one imported commit in detail.", after_help = COMMITS_SHOW_AFTER_HELP)]
    Show(CommitShowArgs),
}

#[derive(Debug, Args)]
pub(in crate::commands) struct CommitSyncArgs {
    /// Project slug to sync into; defaults to the current repo when available.
    #[arg(long)]
    pub(crate) project: Option<String>,
    /// Limit imported commits to those after this timestamp or revision marker.
    #[arg(long)]
    pub(crate) since: Option<String>,
    /// Cap the number of commits scanned from git.
    #[arg(long)]
    pub(crate) limit: Option<usize>,
    /// Preview the sync without persisting commits.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Emit the sync preview or result as JSON.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(in crate::commands) struct CommitListArgs {
    /// Project slug to list commits for; defaults to the current repo when available.
    #[arg(long)]
    pub(crate) project: Option<String>,
    /// Maximum number of imported commits to return.
    #[arg(long, default_value_t = 20)]
    pub(crate) limit: i64,
    /// Number of imported commits to skip before listing.
    #[arg(long, default_value_t = 0)]
    pub(crate) offset: i64,
    /// Emit the commit list as JSON.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(in crate::commands) struct CommitShowArgs {
    /// Commit SHA or imported commit identifier to show.
    pub(crate) commit: String,
    /// Project slug to read from; defaults to the current repo when available.
    #[arg(long)]
    pub(crate) project: Option<String>,
    /// Emit the commit detail as JSON.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Build and inspect the repository index used by scan and analysis flows.",
    after_help = REPO_GROUP_AFTER_HELP
)]
pub(in crate::commands) struct RepoArgs {
    #[command(subcommand)]
    pub(crate) command: RepoCommand,
}

#[derive(Debug, Subcommand)]
pub(in crate::commands) enum RepoCommand {
    #[command(about = "Build or refresh the local repository index.", after_help = REPO_INDEX_AFTER_HELP)]
    Index(IndexRepoArgs),
    #[command(about = "Show local repository index status and analyzer coverage.", after_help = REPO_STATUS_AFTER_HELP)]
    Status(IndexStatusArgs),
}

#[derive(Debug, Args)]
pub(in crate::commands) struct IndexRepoArgs {
    /// Project slug to index; defaults to the current repo when available.
    #[arg(long)]
    pub(crate) project: Option<String>,
    /// Limit indexing to changes after this timestamp or revision marker.
    #[arg(long)]
    pub(crate) since: Option<String>,
    /// Preview indexing work without writing the local index.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Emit the index preview or result as JSON.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(in crate::commands) struct IndexStatusArgs {
    /// Project slug to inspect; defaults to the current repo when available.
    #[arg(long)]
    pub(crate) project: Option<String>,
    /// Emit the status report as JSON.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Extract and inspect the code graph produced from parser-backed repository analysis.",
    after_help = GRAPH_GROUP_AFTER_HELP
)]
pub(in crate::commands) struct GraphArgs {
    #[command(subcommand)]
    pub(crate) command: GraphCommand,
}

#[derive(Debug, Subcommand)]
pub(in crate::commands) enum GraphCommand {
    #[command(about = "Extract code graph facts from the local repository index.", after_help = GRAPH_EXTRACT_AFTER_HELP)]
    Extract(GraphExtractArgs),
    #[command(about = "Show the latest persisted code graph extraction status.", after_help = GRAPH_STATUS_AFTER_HELP)]
    Status(GraphStatusArgs),
}

#[derive(Debug, Args)]
pub(in crate::commands) struct GraphExtractArgs {
    /// Project slug to extract; defaults to the current repo when available.
    #[arg(long)]
    pub(crate) project: Option<String>,
    /// Limit the repository index context to changes after this timestamp or revision marker.
    #[arg(long)]
    pub(crate) since: Option<String>,
    /// Rebuild the local repository index before extracting graph facts.
    #[arg(long)]
    pub(crate) rebuild_index: bool,
    /// Create a fresh extraction run even when an identical completed run exists.
    #[arg(long)]
    pub(crate) force: bool,
    /// Preview extraction without writing database rows or the local index.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Print a human-readable summary instead of the default JSON output.
    #[arg(long)]
    pub(crate) text: bool,
}

#[derive(Debug, Args)]
pub(in crate::commands) struct GraphStatusArgs {
    /// Project slug to inspect; defaults to the current repo when available.
    #[arg(long)]
    pub(crate) project: Option<String>,
    /// Print a human-readable summary instead of the default JSON output.
    #[arg(long)]
    pub(crate) text: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Export and import portable memory bundles.",
    after_help = BUNDLE_GROUP_AFTER_HELP
)]
pub(in crate::commands) struct BundleArgs {
    #[command(subcommand)]
    pub(crate) command: BundleCommand,
}

#[derive(Debug, Subcommand)]
pub(in crate::commands) enum BundleCommand {
    #[command(about = "Export a project memory bundle to a zip archive.", after_help = BUNDLE_EXPORT_AFTER_HELP)]
    Export(ExportArgs),
    #[command(about = "Import a project memory bundle from a zip archive.", after_help = BUNDLE_IMPORT_AFTER_HELP)]
    Import(ImportArgs),
}

#[derive(Debug, Args)]
pub(in crate::commands) struct ExportArgs {
    /// Project slug to export from.
    #[arg(long)]
    pub(crate) project: String,
    /// Output bundle path.
    #[arg(long)]
    pub(crate) out: PathBuf,
    /// Include archived memories in the bundle.
    #[arg(long)]
    pub(crate) include_archived: bool,
    /// Include source file paths in the bundle provenance.
    #[arg(long)]
    pub(crate) include_source_file_paths: bool,
    /// Include git commit identifiers in the bundle provenance.
    #[arg(long)]
    pub(crate) include_git_commits: bool,
    /// Include source excerpts in the bundle provenance.
    #[arg(long)]
    pub(crate) include_source_excerpts: bool,
    /// Preview the bundle contents without writing the output file.
    #[arg(long)]
    pub(crate) dry_run: bool,
}

#[derive(Debug, Args)]
pub(in crate::commands) struct ImportArgs {
    /// Project slug to import into.
    #[arg(long)]
    pub(crate) project: String,
    /// Bundle file to import.
    pub(crate) bundle: PathBuf,
    /// Preview the import without writing memories.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Emit the import preview or result as JSON.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Build a project resume pack from checkpoints, timeline, and durable memory.",
    after_help = RESUME_AFTER_HELP
)]
pub(in crate::commands) struct ResumeArgs {
    /// Project slug to resume; defaults to the current repo when available.
    #[arg(long)]
    pub(crate) project: Option<String>,
    /// Emit the resume pack as JSON.
    #[arg(long)]
    pub(crate) json: bool,
    /// Include the optional LLM summary in the resume output.
    #[arg(long, default_value_t = true)]
    pub(crate) include_llm_summary: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "List persisted project activity events.",
    after_help = ACTIVITIES_AFTER_HELP
)]
pub(in crate::commands) struct ActivitiesArgs {
    /// Project slug; defaults to the current repo when available.
    #[arg(long)]
    pub(crate) project: Option<String>,
    /// Maximum number of activities to return.
    #[arg(long, default_value_t = 50)]
    pub(crate) limit: usize,
    /// Filter by activity kind, for example query, plan, curate, or briefing.
    #[arg(long)]
    pub(crate) kind: Option<String>,
    /// Emit a human-readable text view instead of JSON.
    #[arg(long)]
    pub(crate) text: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Generate a new-agent get-up-to-speed briefing.",
    after_help = UP_TO_SPEED_AFTER_HELP
)]
pub(in crate::commands) struct UpToSpeedArgs {
    /// Project slug; defaults to the current repo when available.
    #[arg(long)]
    pub(crate) project: Option<String>,
    /// Maximum number of recent activities to use.
    #[arg(long, default_value_t = 20)]
    pub(crate) limit: usize,
    /// Ask the configured LLM to synthesize the briefing.
    #[arg(long)]
    pub(crate) llm: bool,
    /// Emit a human-readable text view instead of JSON.
    #[arg(long)]
    pub(crate) text: bool,
}

#[derive(Debug, Args)]
#[command(about = "Run automated Memory quality evaluations.", after_help = EVAL_GROUP_AFTER_HELP)]
pub(in crate::commands) struct EvalArgs {
    #[command(subcommand)]
    pub(crate) command: EvalCommand,
}

#[derive(Debug, Subcommand)]
pub(in crate::commands) enum EvalCommand {
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
pub(in crate::commands) struct EvalDoctorArgs {
    /// Suite directory or suite.toml path to validate.
    #[arg(long)]
    pub(crate) suite: PathBuf,
    /// Emit a human-readable text view instead of JSON.
    #[arg(long)]
    pub(crate) text: bool,
}

#[derive(Debug, Args)]
pub(in crate::commands) struct EvalScaffoldArgs {
    /// Project slug; defaults to the current repo when available.
    #[arg(long)]
    pub(crate) project: Option<String>,
    /// Output directory for suite.toml and items.jsonl.
    #[arg(long)]
    pub(crate) out: PathBuf,
    /// Maximum number of starter items to generate.
    #[arg(long, default_value_t = 12)]
    pub(crate) limit: usize,
    /// Preview files without writing them.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Emit a human-readable text view instead of JSON.
    #[arg(long)]
    pub(crate) text: bool,
}

#[derive(Debug, Args)]
pub(in crate::commands) struct EvalRunArgs {
    /// Suite directory or suite.toml path.
    #[arg(long)]
    pub(crate) suite: PathBuf,
    /// Condition to run. Repeat for paired runs.
    #[arg(long = "condition", default_value = "full-memory")]
    pub(crate) conditions: Vec<String>,
    /// Output directory for run JSON files.
    #[arg(long, default_value = "target/memory-evals")]
    pub(crate) out: PathBuf,
    /// Execution profile: llm for official provider-backed runs, offline for CI-safe dry scoring.
    #[arg(long, default_value = "llm")]
    pub(crate) profile: String,
    /// Number of repeated runs per condition.
    #[arg(long, default_value_t = 1)]
    pub(crate) repeat: usize,
    /// Optional token budget guard for one run group.
    #[arg(long)]
    pub(crate) max_cost: Option<u64>,
    /// Preserve raw answers/transcripts in artifacts. Currently metadata-only; answers are always kept.
    #[arg(long)]
    pub(crate) write_transcripts: bool,
    /// Add LLM judge scores for answer-like items. Deterministic checks still decide success.
    #[arg(long)]
    pub(crate) llm_judge: bool,
    /// Fail when the suite manifest is not marked reviewed.
    #[arg(long)]
    pub(crate) fail_on_unreviewed_labels: bool,
    /// Allow suite-defined shell commands to execute.
    #[arg(long)]
    pub(crate) allow_shell: bool,
    /// Preview work without LLM calls or command execution.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Emit a human-readable text view instead of JSON.
    #[arg(long)]
    pub(crate) text: bool,
}

#[derive(Debug, Args)]
pub(in crate::commands) struct EvalCompareArgs {
    /// Baseline run JSON file or glob. Repeat for multiple run artifacts.
    #[arg(long)]
    pub(crate) baseline: Vec<PathBuf>,
    /// Candidate run JSON file or glob. Repeat for multiple run artifacts.
    #[arg(long)]
    pub(crate) candidate: Vec<PathBuf>,
    /// Optional path to write comparison JSON.
    #[arg(long)]
    pub(crate) out: Option<PathBuf>,
    /// Emit a human-readable text view instead of JSON.
    #[arg(long)]
    pub(crate) text: bool,
}

#[derive(Debug, Args)]
pub(in crate::commands) struct EvalReportArgs {
    /// Comparison JSON file from memory eval compare.
    #[arg(long)]
    pub(crate) comparison: PathBuf,
    /// Optional file to write the rendered report.
    #[arg(long)]
    pub(crate) out: Option<PathBuf>,
    /// Emit a Markdown report instead of comparison JSON.
    #[arg(long)]
    pub(crate) markdown: bool,
    /// Emit a human-readable text view instead of JSON.
    #[arg(long)]
    pub(crate) text: bool,
}

#[derive(Debug, Args)]
pub(in crate::commands) struct EvalGateArgs {
    /// Comparison JSON file from memory eval compare.
    #[arg(long)]
    pub(crate) comparison: PathBuf,
    /// Gate policy TOML file.
    #[arg(long)]
    pub(crate) policy: PathBuf,
    /// Emit a human-readable text view instead of JSON.
    #[arg(long)]
    pub(crate) text: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Manage project checkpoints and plan-backed execution transitions.",
    after_help = CHECKPOINT_GROUP_AFTER_HELP
)]
pub(in crate::commands) struct CheckpointArgs {
    #[command(subcommand)]
    pub(crate) command: CheckpointCommand,
}

#[derive(Debug, Subcommand)]
pub(in crate::commands) enum CheckpointCommand {
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
pub(in crate::commands) struct CheckpointSaveArgs {
    /// Project slug to checkpoint; defaults to the current repo when available.
    #[arg(long)]
    pub(crate) project: Option<String>,
    /// Optional human note describing the checkpoint.
    #[arg(long)]
    pub(crate) note: Option<String>,
    /// Preview the checkpoint payload without writing it.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Emit the checkpoint result as JSON.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(in crate::commands) struct CheckpointShowArgs {
    /// Project slug to inspect; defaults to the current repo when available.
    #[arg(long)]
    pub(crate) project: Option<String>,
}

#[derive(Debug, Args)]
pub(in crate::commands) struct CheckpointStartExecutionArgs {
    /// Project slug to update; defaults to the current repo when available.
    #[arg(long)]
    pub(crate) project: Option<String>,
    /// Optional checkpoint note to store alongside the plan transition.
    #[arg(long)]
    pub(crate) note: Option<String>,
    /// Read the approved plan markdown from a file.
    #[arg(long)]
    pub(crate) plan_file: Option<PathBuf>,
    /// Read the approved plan markdown from stdin.
    #[arg(long)]
    pub(crate) plan_stdin: bool,
    /// Explicit title for the saved plan memory.
    #[arg(long)]
    pub(crate) title: Option<String>,
    /// Stable thread key used to replace later revisions of the same plan.
    #[arg(long)]
    pub(crate) thread_key: Option<String>,
    /// Validate and preview the execution-start flow without writing checkpoint or memory state.
    #[arg(long)]
    pub(crate) dry_run: bool,
}

#[derive(Debug, Args)]
pub(in crate::commands) struct CheckpointStartTaskArgs {
    /// Project slug to update; defaults to the current repo when available.
    #[arg(long)]
    pub(crate) project: Option<String>,
    /// Short title for the direct task.
    #[arg(long)]
    pub(crate) title: String,
    /// Original user instruction or task framing.
    #[arg(long)]
    pub(crate) prompt: String,
    /// Stable task thread key; derived from title/project when omitted.
    #[arg(long)]
    pub(crate) thread_key: Option<String>,
    /// Validate and preview the task-start flow without writing checkpoint or memory state.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Emit the task-start report as JSON.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(in crate::commands) struct CheckpointFinishExecutionArgs {
    /// Project slug to verify; defaults to the current repo when available.
    #[arg(long)]
    pub(crate) project: Option<String>,
    /// Explicit plan thread key when multiple active plans exist.
    #[arg(long)]
    pub(crate) thread_key: Option<String>,
    /// Optional updated plan file to sync before completion verification.
    #[arg(long)]
    pub(crate) plan_file: Option<PathBuf>,
    /// Optional updated plan markdown from stdin to sync before verification.
    #[arg(long)]
    pub(crate) plan_stdin: bool,
    /// Optional explicit summary for the implementation memory recorded after verification.
    #[arg(long)]
    pub(crate) implementation_summary: Option<String>,
    /// Durable implementation detail to include in the recorded implementation memory.
    #[arg(long = "implementation-note")]
    pub(crate) implementation_notes: Vec<String>,
    /// Preview whether verification would pass or fail without writing.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Emit the completion report as JSON.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Capture structured task evidence from a file payload.",
    after_help = CAPTURE_GROUP_AFTER_HELP
)]
pub(in crate::commands) struct CaptureArgs {
    #[command(subcommand)]
    pub(crate) command: CaptureCommand,
}

#[derive(Debug, Subcommand)]
pub(in crate::commands) enum CaptureCommand {
    #[command(about = "Send one structured task capture payload to the backend.", after_help = CAPTURE_TASK_AFTER_HELP)]
    Task(CaptureTaskArgs),
}

#[derive(Debug, Args)]
pub(in crate::commands) struct CaptureTaskArgs {
    /// JSON file containing the capture payload.
    #[arg(long)]
    pub(crate) file: PathBuf,
    /// Validate and preview the capture without writing it.
    #[arg(long)]
    pub(crate) dry_run: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Scan a repository for durable-memory candidates using the local index and analyzers.",
    after_help = SCAN_AFTER_HELP
)]
pub(in crate::commands) struct ScanArgs {
    /// Project slug to scan; defaults to the current repo when available.
    #[arg(long)]
    pub(crate) project: Option<String>,
    /// Limit the scan to files or commits after this timestamp or revision marker.
    #[arg(long)]
    pub(crate) since: Option<String>,
    /// Force a local repository index rebuild before scanning.
    #[arg(long)]
    pub(crate) rebuild_index: bool,
    /// Preview candidate memories without persisting anything.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Emit the scan report as JSON.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Capture recent work and curate it into durable project memory.",
    after_help = REMEMBER_AFTER_HELP
)]
pub(in crate::commands) struct RememberArgs {
    /// Project slug to write into; defaults to the current repo when available.
    #[arg(long)]
    pub(crate) project: Option<String>,
    /// Explicit task title for the remember capture.
    #[arg(long)]
    pub(crate) title: Option<String>,
    /// Memory type to assign (e.g., user, feedback, project, reference, implementation).
    /// When set, overrides the automatic type classification.
    #[arg(long = "type")]
    pub(crate) memory_type: Option<String>,
    /// Original user prompt or task framing to attach to the capture.
    #[arg(long)]
    pub(crate) prompt: Option<String>,
    /// High-level summary of what changed.
    #[arg(long)]
    pub(crate) summary: Option<String>,
    /// Durable note to preserve as evidence for curation.
    #[arg(long = "note")]
    pub(crate) notes: Vec<String>,
    /// File path to attach as changed during the task.
    #[arg(long = "file-changed", visible_alias = "file")]
    pub(crate) files_changed: Vec<String>,
    /// Test name or command that passed.
    #[arg(long = "test-passed")]
    pub(crate) tests_passed: Vec<String>,
    /// Test name or command that failed.
    #[arg(long = "test-failed")]
    pub(crate) tests_failed: Vec<String>,
    /// File containing command output to attach as evidence.
    #[arg(long)]
    pub(crate) command_output_file: Option<PathBuf>,
    /// Auto-detect changed files from git status when possible.
    #[arg(long, default_value_t = true)]
    pub(crate) auto_files: bool,
    /// Preview the derived capture and curate actions without writing them.
    #[arg(long)]
    pub(crate) dry_run: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Curate raw captures into canonical memory entries.",
    after_help = CURATE_AFTER_HELP
)]
pub(in crate::commands) struct CurateArgs {
    /// Project slug to curate.
    #[arg(long)]
    pub(crate) project: String,
    /// Limit the number of raw captures processed in one run.
    #[arg(long)]
    pub(crate) batch_size: Option<i64>,
    /// Preview curation decisions without writing memory state.
    #[arg(long)]
    pub(crate) dry_run: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Review pending memory replacement proposals.",
    after_help = PROPOSALS_GROUP_AFTER_HELP
)]
pub(in crate::commands) struct ProposalsArgs {
    #[command(subcommand)]
    pub(crate) command: ProposalsCommand,
}

#[derive(Debug, Subcommand)]
pub(in crate::commands) enum ProposalsCommand {
    #[command(about = "List pending replacement proposals.", after_help = PROPOSALS_GROUP_AFTER_HELP)]
    List(ProposalsListArgs),
    #[command(about = "Show one pending replacement proposal.", after_help = PROPOSALS_GROUP_AFTER_HELP)]
    Show(ProposalsShowArgs),
    #[command(about = "Approve a pending replacement proposal.", after_help = PROPOSALS_GROUP_AFTER_HELP)]
    Approve(ProposalsResolveArgs),
    #[command(about = "Reject a pending replacement proposal.", after_help = PROPOSALS_GROUP_AFTER_HELP)]
    Reject(ProposalsResolveArgs),
}

#[derive(Debug, Args)]
pub(in crate::commands) struct ProposalsListArgs {
    /// Project slug to inspect.
    #[arg(long)]
    pub(crate) project: String,
    /// Limit the number of proposals printed in text or JSON output.
    #[arg(long)]
    pub(crate) limit: Option<usize>,
    /// Emit the response as JSON.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(in crate::commands) struct ProposalsShowArgs {
    /// Project slug to inspect.
    #[arg(long)]
    pub(crate) project: String,
    /// Pending proposal id.
    #[arg(long)]
    pub(crate) id: Uuid,
    /// Emit the proposal as JSON.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(in crate::commands) struct ProposalsResolveArgs {
    /// Project slug to update.
    #[arg(long)]
    pub(crate) project: String,
    /// Pending proposal id.
    #[arg(long)]
    pub(crate) id: Uuid,
    /// Emit the response as JSON.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Manage embedding indexes and spaces for semantic retrieval.",
    after_help = EMBEDDINGS_GROUP_AFTER_HELP
)]
pub(in crate::commands) struct EmbeddingsArgs {
    #[command(subcommand)]
    pub(crate) command: EmbeddingsCommand,
}

#[derive(Debug, Subcommand)]
pub(in crate::commands) enum EmbeddingsCommand {
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
pub(in crate::commands) struct EmbeddingsActivateArgs {
    /// Name of the configured backend to activate.
    pub(crate) name: String,
}

#[derive(Debug, Args)]
pub(in crate::commands) struct ProjectArgs {
    /// Project slug to operate on.
    #[arg(long)]
    pub(crate) project: String,
}

#[derive(Debug, Args)]
pub(in crate::commands) struct EmbeddingsProjectArgs {
    /// Project slug to operate on.
    #[arg(long)]
    pub(crate) project: String,
    /// Preview the embedding maintenance action without writing it.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Restrict to a single configured backend by name. Omit to
    /// operate on every configured backend so every space stays
    /// covered.
    #[arg(long)]
    pub(crate) backend: Option<String>,
}

#[derive(Debug, Args)]
#[command(
    about = "Archive low-confidence, low-importance memories in a project.",
    after_help = ARCHIVE_AFTER_HELP
)]
pub(in crate::commands) struct ArchiveArgs {
    /// Project slug to archive within.
    #[arg(long)]
    pub(crate) project: String,
    /// Maximum confidence allowed for candidate memories.
    #[arg(long, default_value_t = 0.3)]
    pub(crate) max_confidence: f32,
    /// Maximum importance allowed for candidate memories.
    #[arg(long, default_value_t = 1)]
    pub(crate) max_importance: i32,
    /// Preview archive candidates without changing memory state.
    #[arg(long)]
    pub(crate) dry_run: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Open the terminal UI for browsing memories, querying memory, and inspecting project state.",
    after_help = TUI_AFTER_HELP
)]
pub(in crate::commands) struct TuiArgs {
    /// Project slug to open initially; defaults to the current repo when available.
    #[arg(long)]
    pub(crate) project: Option<String>,
}

#[derive(Debug, Args)]
#[command(
    about = "Inspect or flush automation state for a project.",
    after_help = AUTOMATION_GROUP_AFTER_HELP
)]
pub(in crate::commands) struct AutomationArgs {
    #[command(subcommand)]
    pub(crate) command: AutomationCommand,
}

#[derive(Debug, Subcommand)]
pub(in crate::commands) enum AutomationCommand {
    #[command(about = "Show the current automation state for a project.", after_help = AUTOMATION_STATUS_AFTER_HELP)]
    Status(ProjectArgs),
    #[command(about = "Flush pending automation work into capture and optional curation.", after_help = AUTOMATION_FLUSH_AFTER_HELP)]
    Flush(AutomationFlushArgs),
}

#[derive(Debug, Args)]
pub(in crate::commands) struct AutomationFlushArgs {
    #[command(flatten)]
    pub(crate) project: ProjectArgs,
    /// Run curation after flushing capture state.
    #[arg(long)]
    pub(crate) curate: bool,
    /// Preview the flush without creating capture or automation state.
    #[arg(long)]
    pub(crate) dry_run: bool,
}

pub(super) async fn run() -> Result<()> {
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
            crate::commands::wizard::handle(args).await?;
            return Ok(());
        }
        Command::Init(args) => {
            crate::commands::init::handle(args).await?;
            return Ok(());
        }
        Command::Upgrade(args) => {
            crate::commands::upgrade::handle(args).await?;
            return Ok(());
        }
        Command::Completion(args) => {
            crate::commands::completion::handle(args).await?;
            return Ok(());
        }
        Command::Dev(args) => {
            crate::commands::dev::handle(args).await?;
            return Ok(());
        }
        Command::Service(args) => {
            crate::commands::service::handle(args, cli_config.clone()).await?;
            return Ok(());
        }
        Command::Watcher(args) => {
            if !crate::commands::watcher::handle_pre_config(args, cli_config.clone()).await? {
                return Ok(());
            }
        }
        Command::Doctor(args) => {
            crate::commands::doctor::handle(args, cli_config.clone()).await?;
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
        Command::Upgrade(_) => unreachable!("upgrade is handled before config loading"),
        Command::Completion(_) => unreachable!("completion is handled before config loading"),
        Command::Dev(_) => unreachable!("dev subcommands are handled before config loading"),
        Command::Service(ServiceArgs {
            command: ServiceCommand::Run,
        }) => unreachable!("service run is handled before config loading"),
        Command::Service(_) => unreachable!("service management is handled before config loading"),
        Command::Mcp(args) => crate::commands::mcp::handle(args, config).await?,
        Command::Watcher(WatcherArgs {
            command:
                WatcherCommand::Enable(_) | WatcherCommand::Disable(_) | WatcherCommand::Status(_),
        }) => unreachable!("watcher lifecycle commands are handled before config loading"),
        Command::Doctor(_) => unreachable!("doctor is handled before config loading"),
        Command::Status(args) => {
            crate::commands::status::handle(args, cli_config_path, client, config).await?
        }
        Command::Commits(args) => crate::commands::commits::handle(args, client, config).await?,
        Command::Query(args) => crate::commands::query::handle(args, client, config).await?,
        Command::VerifyProvenance(args) => {
            crate::commands::verify_provenance::handle(args, client, config).await?
        }
        Command::History(args) => crate::commands::history::handle(args, client, config).await?,
        Command::PruneHistory(args) => {
            crate::commands::prune_history::handle(args, client, config).await?
        }
        Command::Repo(args) => crate::commands::repo::handle(args, config).await?,
        Command::Graph(args) => crate::commands::graph::handle(args, client, config).await?,
        Command::Bundle(args) => {
            let api = ApiClient::new(client, config);
            crate::commands::bundle::handle(args, &api).await?;
        }
        Command::Checkpoint(args) => {
            crate::commands::checkpoint::handle(args, client, config, cli_writer_id).await?
        }
        Command::Resume(args) => crate::commands::resume::handle(args, client, config).await?,
        Command::Activities(args) => {
            crate::commands::activities::handle(args, client, config).await?
        }
        Command::UpToSpeed(args) => {
            crate::commands::up_to_speed::handle(args, client, config).await?
        }
        Command::Eval(args) => crate::commands::eval::handle(args, client, config).await?,
        Command::Scan(args) => {
            crate::commands::scan::handle(args, client, config, cli_writer_id).await?
        }
        Command::Capture(args) => {
            crate::commands::capture::handle(args, client, config, cli_writer_id).await?
        }
        Command::Remember(args) => {
            crate::commands::remember::handle(args, client, config, cli_writer_id).await?
        }
        Command::Curate(args) => crate::commands::curate::handle(args, client, config).await?,
        Command::Proposals(args) => {
            let api = ApiClient::new(client, config);
            crate::commands::proposals::handle(args, &api).await?;
        }
        Command::Embeddings(args) => {
            crate::commands::embeddings::handle(args, client, config).await?
        }
        Command::Health => crate::commands::health::handle(client, config).await?,
        Command::Stats => crate::commands::stats::handle(client, config).await?,
        Command::Archive(args) => crate::commands::archive::handle(args, client, config).await?,
        Command::Automation(args) => {
            crate::commands::automation::handle(args, client, config, cli_writer_id).await?
        }
        Command::Watcher(args) => {
            crate::commands::watcher::handle(args, config, cli_config_path, cli_writer_id).await?
        }
        Command::Tui(args) => crate::commands::tui::handle(args, client, config).await?,
    }

    Ok(())
}

pub(crate) fn write_shared_env_file(path: &Path, key: &str, value: &str) -> Result<()> {
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

pub(in crate::commands) const DEV_API_TOKEN: &str = "dev-memory-token";
const SERVICE_API_TOKEN_KEY: &str = "MEMORY_LAYER__SERVICE__API_TOKEN";

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ServiceApiTokenAction {
    Created,
    Rotated,
    Preserved,
}

#[derive(Debug, Serialize)]
pub(crate) struct ServiceApiTokenEnsureResult {
    pub(crate) path: String,
    pub(crate) changed: bool,
    pub(crate) action: ServiceApiTokenAction,
}

impl ServiceApiTokenEnsureResult {
    pub(crate) fn summary_line(&self) -> String {
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

pub(crate) fn is_placeholder_service_api_token(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.is_empty() || trimmed == DEV_API_TOKEN
}

pub(crate) fn generate_service_api_token() -> String {
    format!("ml_{}", Uuid::new_v4().simple())
}

pub(crate) fn ensure_shared_service_api_token(
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

pub(crate) fn preview_shared_service_api_token_for_config(
    config_path: &Path,
    preferred_token: Option<&str>,
    rotate_placeholder: bool,
) -> Result<ServiceApiTokenEnsureResult> {
    let _ = preferred_token;
    plan_shared_service_api_token(&shared_env_path_for_config(config_path), rotate_placeholder)
}

pub(crate) fn plan_shared_service_api_token(
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

pub(crate) fn ensure_shared_service_api_token_for_config(
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

pub(crate) fn shared_env_lookup(path: &Path, key: &str) -> Option<String> {
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

pub(crate) fn shared_env_path_for_config(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("memory-layer.env")
}

pub(crate) fn default_global_config_path() -> PathBuf {
    platform::preferred_global_config_path()
}

pub(crate) fn default_shared_capnp_unix_socket() -> String {
    platform::default_shared_capnp_unix_socket()
}

pub(in crate::commands) fn backend_start_hint(config_path: &Path) -> String {
    if backend_service_available() {
        "memory service enable".to_string()
    } else {
        format!("memory --config {} service run", config_path.display())
    }
}

pub(crate) fn backend_service_available() -> bool {
    platform::backend_service_available()
}

#[cfg(not(target_os = "macos"))]
pub(in crate::commands) fn packaged_service_available() -> bool {
    platform::packaged_system_service_available()
}

#[cfg(not(target_os = "macos"))]
pub(in crate::commands) fn run_systemctl_system<const N: usize>(args: [&str; N]) -> Result<()> {
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

#[cfg(test)]
mod tests;
