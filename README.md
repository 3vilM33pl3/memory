# Memory Layer

Memory Layer is a local-first memory system for coding agents and the humans working with them. Its goal is to make project-specific knowledge durable and queryable instead of leaving it scattered across past chats, terminal scrollback, commit history, and ad hoc notes.

The system stores two kinds of information:
- raw task captures, which preserve what happened during a piece of work
- curated canonical memory, which turns those captures into concise, searchable facts with provenance

That split matters because it keeps the system auditable. You can keep the original task context, summaries, tests, and changed files, then derive cleaner long-lived memory entries from that material without losing where the information came from.

The repository is built around four working parts:
- `mem-service`: Axum backend over PostgreSQL
- `mem-cli`: local CLI for query/capture/curate flows
- `memory-watch`: optional background watcher for conservative automatic memory capture
- `.agents/skills/memory-layer/`: repo-local Codex skill and wrapper scripts

In practice, the intended workflow is:
1. query memory before answering project-specific questions
2. capture the result of meaningful work as structured task data
3. curate those captures into durable project memory
4. query that memory later by project

The current implementation is designed for local development and experimentation. It runs against PostgreSQL, exposes a localhost HTTP compatibility API through `mem-service`, and now also exposes a persistent Cap'n Proto transport for live client updates. `mem-cli`, the TUI, and the repo-local skill scripts all sit on top of that backend.

## Prerequisites

- Rust toolchain with `cargo`
- PostgreSQL running and reachable

## Setup

Recommended first step:

```bash
cargo run --bin mem-cli -- wizard
```

The wizard guides you through:
- setup scope
- shared/global config such as database URL, API token, and LLM model
- repo-local project and automation config
- optional service/actions like watcher, scan, and doctor
- final review and apply

It is a step-by-step TUI. Fixed choices use menu-style cycling/toggles, and only free-form values like URLs, model names, tokens, and path lists use text input.

When you run it inside a repository, it defaults to local repo files only. Shared/global config is opt-in inside the wizard, or you can preselect it with:

```bash
cargo run --bin mem-cli -- wizard --global
```

Manual setup is still available below.

1. Create or edit the shared global config.

This is optional if you only want to bootstrap the repository first. The wizard leaves shared files alone unless you opt into them.

Local install path:

```bash
mkdir -p "${XDG_CONFIG_HOME:-$HOME/.config}/memory-layer"
cp memory-layer.toml.example "${XDG_CONFIG_HOME:-$HOME/.config}/memory-layer/memory-layer.toml"
```

Packaged/system path:

```bash
/etc/memory-layer/memory-layer.toml
```

Set shared values there:
- `database.url`
- `service.api_token`
- `[llm]` configuration for `memctl scan`

Set shared environment variables for CLI and services:
- local/user installs: `${XDG_CONFIG_HOME:-$HOME/.config}/memory-layer/memory-layer.env`
- Debian/system installs: `/etc/memory-layer/memory-layer.env`

Example:

```bash
OPENAI_API_KEY=your-api-key-here
```

2. Initialize the repository:

```bash
cargo run --bin mem-cli -- init
```

This creates a local `.mem/` directory with:
- `.mem/config.toml`
- `.mem/project.toml`
- `.mem/memory-layer.env` when you set repo-local secret overrides
- `.mem/runtime/`

It also installs the repo-local skill under:
- `.agents/skills/memory-layer/`

The generated repo-local config contains project-specific overrides. It can also override shared settings like `database.url`. Repo-local secret overrides such as an LLM API key live in `.mem/memory-layer.env`. `.mem/` is ignored by git.

3. Optional: edit `.mem/config.toml` for repo-specific overrides such as automation paths or repo root.

Global config example:

```toml
[service]
bind_addr = "127.0.0.1:4040"
capnp_unix_socket = "/tmp/memory-layer.capnp.sock"
capnp_tcp_addr = "127.0.0.1:4041"
api_token = "dev-memory-token"
request_timeout = "30s"

[database]
url = "postgresql://memory:YOUR_PASSWORD@localhost:5432/memory"

[features]
llm_curation = false

[llm]
provider = "openai_compatible"
base_url = "https://api.openai.com/v1"
api_key_env = "OPENAI_API_KEY"
model = ""
temperature = 0.0
max_input_bytes = 120000
max_output_tokens = 3000
```

4. Start the shared backend:

```bash
cargo run --bin mem-service
```

When `mem-service` is started with an explicit config file path, it watches that file and restarts itself in place after the file changes. That lets you update values like `service.api_token`, automation settings, or the bind address without manually killing and relaunching the backend.

5. Optional: enable the per-repo watcher as a `systemd --user` service:

```bash
cargo run --bin mem-cli -- watch enable --project memory
```

Check or disable it later with:

```bash
cargo run --bin mem-cli -- watch status --project memory
cargo run --bin mem-cli -- watch disable --project memory
```

The generated watcher unit reads shared environment variables from:

```bash
${XDG_CONFIG_HOME:-$HOME/.config}/memory-layer/memory-layer.env
```

So putting `OPENAI_API_KEY=...` there makes it available to the watcher without exporting it in every shell.

6. In another shell, verify the backend is up:

```bash
cargo run --bin mem-cli -- health
```

## Install

### Local install

Install both binaries into `~/.local/bin`:

```bash
./scripts/install-local.sh
```

Then run:

```bash
# from the target repository root
~/.local/bin/mem-cli wizard
~/.local/bin/mem-service
~/.local/bin/mem-cli watch enable --project memory
~/.local/bin/mem-cli tui
```

### Debian package build

Build a `.deb` package:

```bash
./packaging/build-deb.sh
```

The package will be written under `target/debian/`.

Install it on a Debian machine:

```bash
sudo dpkg -i target/debian/memory-layer_0.1.0_amd64.deb
```

That installs:
- `mem-cli`
- `mem-service`
- `memory-watch`
- shared config in `/etc/memory-layer/`
- shared environment file in `/etc/memory-layer/memory-layer.env`
- the skill template in `/usr/share/memory-layer/skill-template`

Recommended Debian workflow for another repo:

```bash
cd /path/to/another-project
mem-cli wizard
sudo systemctl enable --now memory-layer.service
mem-cli watch enable --project another-project
mem-cli tui
```

`mem-cli init` now creates both `.mem/` and `.agents/skills/memory-layer/` in the target repository.

## Common Commands

Query memory:

```bash
cargo run --bin mem-cli -- query \
  --project memory \
  --question "How is project memory stored?"
```

Scan an existing repository and write initial durable memory:

```bash
cargo run --bin mem-cli -- scan --project memory
```

Preview the scan without writing:

```bash
cargo run --bin mem-cli -- scan --project memory --dry-run
```

Capture a completed task:

```bash
cargo run --bin mem-cli -- capture-task --file payload.json
```

Automatically capture and curate a completed task:

```bash
cargo run --bin mem-cli -- remember \
  --project memory \
  --note "The remember command captures and curates memory in one step." \
  --test-passed "cargo check"
```

Curate raw captures into canonical memory:

```bash
cargo run --bin mem-cli -- curate --project memory
```

Reindex a project:

```bash
cargo run --bin mem-cli -- reindex --project memory
```

Show service stats:

```bash
cargo run --bin mem-cli -- stats
```

Run setup diagnostics:

```bash
cargo run --bin mem-cli -- doctor
cargo run --bin mem-cli -- doctor --fix
```

Launch the TUI:

```bash
cargo run --bin mem-cli -- tui --project memory
```

The TUI opens a persistent connection to the backend. When the Cap'n Proto listener is available it subscribes to project and memory updates, so new memories and overview changes appear without pressing `r`. `r` still forces a full HTTP resync.

Tabs:
- `Memories`: browse the stored corpus
- `Query`: run a question and inspect the memories returned for that question
- `Log`: inspect query prompts and the returned answers/errors
- `Activity`: inspect streamed capture/curate/reindex/archive/delete events
- `Project`: view project-level health and counts

Inspect or flush automation state:

```bash
cargo run --bin mem-cli -- automation status --project memory
cargo run --bin mem-cli -- automation flush --project memory
```

TUI controls:
- `Tab`, `h`, `l`: switch tabs
- `j`, `k`: move selection
- `/`: text search filter
- `?`: open query input and run a question in the Query tab
- typing in the `Query` tab starts question input directly
- `g`: tag filter
- `s`: cycle status filter
- `t`: cycle memory-type filter
- `x`: clear filters
- `r`: force resync
- `c`: curate project
- `i`: reindex search chunks
- `a`: archive low-value memories
- `D`: delete the selected memory
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
3. Optional: `mem-cli watch enable --project <slug>`
4. `remember` the completed task
4. Query the resulting memory

The `remember` command auto-detects changed files from `git status` when possible, creates a capture payload for you, sends it to the backend, and then runs curation immediately. If you omit `--title`, `--prompt`, or `--summary`, it derives defaults automatically.

The runtime config model is layered:
1. explicit `--config`, if provided
2. global shared config
3. repo-local `.mem/config.toml`
4. `MEMORY_LAYER__...` environment variables

Transport defaults:
- HTTP compatibility API: `service.bind_addr`
- Cap'n Proto Unix socket: `service.capnp_unix_socket`
- Cap'n Proto localhost TCP fallback: `service.capnp_tcp_addr`

The backend starts both listeners by default. Local clients prefer the Unix socket when it exists and fall back to the TCP listener otherwise.

The `doctor` command checks the repo-local `.mem/` bootstrap, merged config validity, backend reachability, LLM config needed for `scan`, and automation/runtime state. By default it reports issues and suggests exact fixes. With `--fix`, it only applies safe local repairs such as creating missing `.mem/` files or adding `/.mem` to `.gitignore`.

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

Debian/systemd assets live under `packaging/debian/`. You can build a `.deb` manually with:

```bash
./packaging/build-deb.sh
```

The current primary development workflow is still running from source with `cargo`, but Debian packaging is available when you want a packaged install artifact.
