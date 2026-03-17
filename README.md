# Memory Layer

Memory Layer is a local-first memory system for coding agents and the humans working with them. Its goal is to make project-specific knowledge durable and queryable instead of leaving it scattered across past chats, terminal scrollback, commit history, and ad hoc notes.

The system stores two kinds of information:
- raw task captures, which preserve what happened during a piece of work
- curated canonical memory, which turns those captures into concise, searchable facts with provenance

That split matters because it keeps the system auditable. You can keep the original task context, summaries, tests, and changed files, then derive cleaner long-lived memory entries from that material without losing where the information came from.

The repository is built around three working parts:
- `mem-service`: Axum backend over PostgreSQL
- `mem-cli`: local CLI for query/capture/curate flows
- `.agents/skills/memory-layer/`: repo-local Codex skill and wrapper scripts

In practice, the intended workflow is:
1. query memory before answering project-specific questions
2. capture the result of meaningful work as structured task data
3. curate those captures into durable project memory
4. query that memory later by project

The current implementation is designed for local development and experimentation. It runs against PostgreSQL, exposes a localhost HTTP API through `mem-service`, and lets agents or users interact with that API through `mem-cli` or the repo-local skill scripts.

## Prerequisites

- Rust toolchain with `cargo`
- PostgreSQL running and reachable

## Setup

1. Initialize the repository:

```bash
cargo run --bin mem-cli -- init
```

This creates a local `.mem/` directory with:
- `.mem/config.toml`
- `.mem/project.toml`
- `.mem/runtime/`

The generated config contains placeholders and keeps secrets local to the repo. `.mem/` is ignored by git.

2. Edit `.mem/config.toml` and set:
- `database.url`
- `service.api_token`

Example:

```toml
[service]
bind_addr = "127.0.0.1:4040"
api_token = "dev-memory-token"
request_timeout = "30s"

[database]
url = "postgresql://memory:YOUR_PASSWORD@localhost:5432/memory"

[features]
llm_curation = false
```

3. Start the backend:

```bash
cargo run --bin mem-service -- .mem/config.toml
```

When `mem-service` is started with an explicit config file path, it watches that file and restarts itself in place after the file changes. That lets you update values like `service.api_token`, automation settings, or the bind address without manually killing and relaunching the backend.

4. Optional: start the hidden automation watcher:

```bash
cargo run --bin memory-watch -- --config .mem/config.toml run --project memory
```

5. In another shell, verify the backend is up:

```bash
cargo run --bin mem-cli -- --config .mem/config.toml health
```

## Install

### Local install

Install both binaries into `~/.local/bin`:

```bash
./scripts/install-local.sh
```

Then run:

```bash
~/.local/bin/mem-cli init
~/.local/bin/mem-service .mem/config.toml
~/.local/bin/memory-watch --config .mem/config.toml run --project memory
~/.local/bin/mem-cli --config .mem/config.toml tui
```

### Debian package build

Build a `.deb` package:

```bash
./packaging/build-deb.sh
```

The package will be written under `target/debian/`.

## Common Commands

Query memory:

```bash
cargo run --bin mem-cli -- --config .mem/config.toml query \
  --project memory \
  --question "How is project memory stored?"
```

Capture a completed task:

```bash
cargo run --bin mem-cli -- --config .mem/config.toml capture-task --file payload.json
```

Automatically capture and curate a completed task:

```bash
cargo run --bin mem-cli -- --config .mem/config.toml remember \
  --project memory \
  --note "The remember command captures and curates memory in one step." \
  --test-passed "cargo check"
```

Curate raw captures into canonical memory:

```bash
cargo run --bin mem-cli -- --config .mem/config.toml curate --project memory
```

Reindex a project:

```bash
cargo run --bin mem-cli -- --config .mem/config.toml reindex --project memory
```

Show service stats:

```bash
cargo run --bin mem-cli -- --config .mem/config.toml stats
```

Launch the TUI:

```bash
cargo run --bin mem-cli -- --config .mem/config.toml tui --project memory
```

Inspect or flush automation state:

```bash
cargo run --bin mem-cli -- --config .mem/config.toml automation status --project memory
cargo run --bin mem-cli -- --config .mem/config.toml automation flush --project memory
```

TUI controls:
- `Tab`, `h`, `l`: switch tabs
- `j`, `k`: move selection
- `/`: text search filter
- `g`: tag filter
- `s`: cycle status filter
- `t`: cycle memory-type filter
- `x`: clear filters
- `r`: refresh
- `c`: curate project
- `i`: reindex search chunks
- `a`: archive low-value memories
- `q`: quit

## Capture Payload Example

```json
{
  "project": "memory",
  "task_title": "Build memory layer backend",
  "user_prompt": "Implement the memory layer service and CLI",
  "agent_summary": "Added the Rust workspace, Axum service, CLI, PostgreSQL migrations, and repo-local skill wrappers.",
  "files_changed": [
    "crates/mem-service/src/main.rs",
    "crates/mem-cli/src/main.rs",
    "migrations/0001_init.sql"
  ],
  "tests": [
    {
      "command": "cargo test",
      "status": "passed"
    }
  ],
  "notes": [
    "Project memory is stored in PostgreSQL and queried through full-text search over canonical memory entries and chunks."
  ]
}
```

Typical workflow:
1. Run `memctl init`
2. Start `mem-service`
3. `remember` the completed task
4. Query the resulting memory

The `remember` command auto-detects changed files from `git status` when possible, creates a capture payload for you, sends it to the backend, and then runs curation immediately. If you omit `--title`, `--prompt`, or `--summary`, it derives defaults automatically.

When `[automation].enabled = true`, `memory-watch` observes repo activity and can either:
- `suggest` memory writes by logging candidate work without persisting
- `auto` persist high-confidence work through the same remember flow

See:
- `docs/architecture/hidden-memory-daemon.md`
- `docs/plans/hidden-memory-daemon-plan.md`

## Skill Usage

The shipped repo-local skill is at:
- `.agents/skills/memory-layer/SKILL.md`

Helper scripts:
- `.agents/skills/memory-layer/scripts/query-memory.sh`
- `.agents/skills/memory-layer/scripts/capture-task.sh`
- `.agents/skills/memory-layer/scripts/curate-memory.sh`
- `.agents/skills/memory-layer/scripts/remember-task.sh`
- `.agents/skills/memory-layer/scripts/remember-current-work.sh`

Examples:

```bash
./.agents/skills/memory-layer/scripts/query-memory.sh "How is project memory stored?" memory
./.agents/skills/memory-layer/scripts/capture-task.sh payload.json
./.agents/skills/memory-layer/scripts/curate-memory.sh memory
./.agents/skills/memory-layer/scripts/remember-task.sh \
  --note "The remember workflow captures and curates memory in one step."
./.agents/skills/memory-layer/scripts/remember-current-work.sh \
  --note "Store the durable workflow automatically after meaningful work."
```

The scripts default to running the CLI with `cargo run`, so they work from source as long as the backend is already running.

## Development

Format:

```bash
cargo fmt
```

Test:

```bash
cargo test
```

## Packaging

Debian/systemd assets live under `packaging/debian/`, but this repo does not yet build a `.deb` automatically. The current supported path is running from source with `cargo`.
