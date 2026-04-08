# Getting Started

This guide is written for someone who just wants Memory Layer working with as little setup friction as possible.

## Table of Contents

- [Prerequisites](#prerequisites)
- [Fast Install: Debian](#fast-install-debian)
- [Fast Install: macOS](#fast-install-macos)
- [What The Wizard Will Ask For](#what-the-wizard-will-ask-for)
- [File Locations](#file-locations)
- [What To Put Where](#what-to-put-where)
- [Writer ID](#writer-id)
- [Primary And Relay Services](#primary-and-relay-services)
- [Daily Use](#daily-use)
- [Optional Background Watcher](#optional-background-watcher)
- [Upgrading An Existing Install](#upgrading-an-existing-install)
- [Using `scan`](#using-scan)
- [Web UI Notes](#web-ui-notes)
- [Importing Commit History](#importing-commit-history)
- [Running From Source](#running-from-source)

## Prerequisites

Before you install or run the wizard, have these ready:

- a PostgreSQL connection string
- optional: an OpenAI-compatible API key if you want `memory scan`
- PostgreSQL with `pgvector` installed if you want semantic retrieval
- `go` on `PATH` if you plan to use the repo-local Memory Layer skills through `go run`

You do not need to invent a Memory Layer service token yourself for normal installs. Setup generates a machine-local token automatically in `memory-layer.env`, and local write-capable tools use that token to authenticate to `mem-service`.

## Fast Install: Debian

1. Download the latest `.deb` package from the GitHub Releases page.
2. Install it:

```bash
sudo dpkg -i memory-layer_<version>_amd64.deb
```

3. Configure the shared/global settings once on this machine:

```bash
memory wizard --global
```

This is where you set the shared database URL. The shared service API token is provisioned automatically if it is missing or still using the development placeholder. A writer ID is optional; if you do not set one, Memory Layer derives a stable writer identity automatically.

4. Go to the project you want to use:

```bash
cd /path/to/your-project
```

5. Run the repo-local setup wizard:

```bash
memory wizard
```

The repo-local skill bundle that `memory wizard` installs uses a shared Go helper under `.agents/skills/memory-layer/scripts/`, so agent-driven skill usage in that repository requires `go` to be available on `PATH`.

Most mutating `memory` commands also support `--dry-run`, so you can preview setup, write, indexing, bundle, and checkpoint operations before they touch local files, services, or backend state.

6. Start the backend service:

```bash
sudo systemctl enable --now memory-layer.service
```

7. Open the UI you prefer:

```bash
memory tui
```

## Fast Install: macOS

1. Install from the local Homebrew formula or your tap:

```bash
brew install --HEAD ./packaging/macos/homebrew/memory-layer.rb
```

2. Configure the shared/global settings once on this machine:

```bash
memory wizard --global
```

3. Go to the project you want to use:

```bash
cd /path/to/your-project
```

4. Run the repo-local setup wizard:

```bash
memory wizard
```

The repo-local skill bundle that `memory wizard` installs uses a shared Go helper under `.agents/skills/memory-layer/scripts/`, so agent-driven skill usage in that repository requires `go` to be available on `PATH`.

5. Start the backend LaunchAgent:

```bash
memory service enable
```

6. Open the TUI:

```bash
memory tui
```

or in a browser:

```text
http://127.0.0.1:4040/
```

## What The Wizard Will Ask For

The wizard can set up:

- shared/global settings when that scope is enabled:
  - the PostgreSQL database URL
  - the shared service API token override, if you want to replace the auto-generated one
  - an optional shared `writer.id`
- optional LLM settings for `scan`
- repo-local `.mem/` files
- optional watcher setup
- the repo-local memory skill bundle, which uses a shared Go helper under `.agents/skills/memory-layer/scripts/`

Important detail:

- inside a repository, `memory wizard` is local-first by default
- use `memory wizard --global` when you want to edit the shared/global config
- or enable `shared/global setup` in the first wizard step

## File Locations

### Shared configuration

Debian install:

- `/etc/memory-layer/memory-layer.toml`
- `/etc/memory-layer/memory-layer.env`

macOS install:

- `~/Library/Application Support/memory-layer/memory-layer.toml`
- `~/Library/Application Support/memory-layer/memory-layer.env`

Local install:

- `~/.config/memory-layer/memory-layer.toml`
- `~/.config/memory-layer/memory-layer.env`

### Per-project configuration

Inside each project:

- `.mem/config.toml`
- `.mem/project.toml`
- `.mem/memory-layer.env`
- `.mem/runtime/`
- `.agents/memory-layer.toml`

## What To Put Where

### Shared/global config

Use this for values shared by many repos:

- `database.url`
- `[cluster]` settings if you want relay discovery on a local network
- `[llm]` settings

The shared service API token normally lives in the adjacent `memory-layer.env` file and is provisioned automatically during setup.

### Repo-local config

Use this for project-specific overrides:

- watcher settings
- local backend ports
- project-specific DB override if needed
- repo-specific `writer.id` override if one project should write under a different custom writer identity

### Project memory behavior

Use `.agents/memory-layer.toml` for project-owned behavior that should be easy to adapt without digging through service config:

- include and ignore path hints for repository scans
- enabled analyzers
- curation replacement policy for memory updates
- future graph and plugin controls

Example:

```toml
[curation]
replacement_policy = "balanced"
```

Available policies are `conservative`, `balanced`, and `aggressive`. `balanced` is the default.

### Env files

Use these for secrets such as:

```bash
MEMORY_LAYER__SERVICE__API_TOKEN=auto-generated-or-manually-overridden
OPENAI_API_KEY=your-api-key-here
```

## Writer ID

Each coding agent or tool that writes memory gets a writer ID.

If you do nothing, Memory Layer derives one automatically from:

- the writing tool
- the local user
- the local host name

That gives stable defaults such as:

- `memory-olivier-monolith`
- `memory-watcher-olivier-monolith`

For most setups, that automatic writer identity is enough.

You can configure it in TOML:

```toml
[writer]
id = "codex-cli-main"
name = "Codex CLI"
```

or with an environment variable:

```bash
export MEMORY_LAYER_WRITER_ID=codex-cli-main
```

Use an explicit writer ID only when you want a custom stable label shared across tools or machines.

## Primary And Relay Services

If a machine can reach PostgreSQL, `mem-service` runs as a `primary`.

If a machine cannot reach PostgreSQL but can see another Memory Layer service on the local network, `mem-service` can run as a `relay`. In relay mode it discovers a primary over UDP multicast and forwards the normal HTTP API and browser WebSocket traffic to it.

## Daily Use

Open the TUI:

```bash
memory tui
```

For a visual walkthrough of each tab, use the [TUI Guide](tui/README.md).

Open the web UI:

```text
http://127.0.0.1:4040/
```

Check health:

```bash
memory service status
memory health
memory doctor
```

Save a useful project fact:

```bash
memory remember --project my-project --note "Deployment uses a systemd service."
```

Search project memory:

```bash
memory query --project my-project --question "How is deployment handled here?"
```

Export a shareable memory bundle:

```bash
memory bundle export --project my-project --out my-project.mlbundle.zip
```

For semantic-search maintenance commands such as `memory embeddings reindex`, `memory embeddings reembed`, and `memory embeddings prune`, see [Embedding Operations](cli/embeddings.md).
For project memory backup and restore, see [Memory Bundles](cli/bundles.md).
For watcher health states, restart behavior, and recovery signals in the TUI, see [Watcher Health](cli/watchers.md).
For the direct write command, see [Remember Command](cli/remember.md).
For service management and setup diagnostics, see [Service Commands](cli/service.md) and [Doctor Diagnostics](cli/doctor.md).
For bootstrap behavior, see [Wizard And Bootstrap](cli/wizard.md).

For getting back into flow after an interruption, see [Resume Briefings](cli/resume.md).

## Optional Background Watcher

If you want Memory Layer to capture useful work in the background:

```bash
memory watcher enable --project my-project
```

When the backend service restarts, service-managed watchers will restart too so they reconnect cleanly to the new backend instance.

Check it:

```bash
memory watcher status --project my-project
```

In the TUI:

- the `Watchers` tab shows each watcher's health, restart attempts, and last heartbeat
- the `Activity` tab shows watcher-health transitions such as `stale`, `restarting`, `failed`, and recovery back to `healthy`
- recovery events now show what state the watcher recovered from and, when relevant, how many restart attempts happened before recovery

Disable it later:

```bash
memory watcher disable --project my-project
```

## Upgrading An Existing Install

If you already use Memory Layer and are upgrading to a newer release:

1. install the new `.deb`
2. make sure PostgreSQL has `pgvector` installed for your server version
3. enable the extension in your target database:

```sql
CREATE EXTENSION IF NOT EXISTS vector;
```

4. restart the backend service:

```bash
sudo systemctl restart memory-layer.service
```

5. verify the setup:

```bash
memory doctor
```

6. rebuild embeddings for existing project memories:

```bash
memory embeddings reindex --project my-project
```

If you later switch the embedding model, Memory Layer keeps the old embedding space instead of overwriting it. Use:

```bash
memory embeddings reembed --project my-project
```

to materialize vectors for the newly active space, and:

```bash
memory embeddings prune --project my-project
```

only when you want to delete non-active embedding spaces explicitly.

For the command-level explanation of when to use each of those operations, see [Embedding Operations](cli/embeddings.md).

If `memory doctor` reports that `pgvector` is missing, install the PostgreSQL package first and rerun the check.

On Debian, upgrades should preserve local edits to:

- `/etc/memory-layer/memory-layer.env`
- `/etc/memory-layer/memory-layer.toml`

Those files are treated as package-managed configuration files rather than being overwritten
with package defaults on every upgrade.

## Using `scan`

`scan` reads a repository, sends a structured summary to the configured LLM, and writes useful durable memories back into Memory Layer.

Full command documentation:

- [Memory Bundles](cli/bundles.md)
- [Scan Command](cli/scan.md)

Try it safely first:

```bash
memory scan --project my-project --dry-run
```

Then write the results:

```bash
memory scan --project my-project
```

If `scan` fails, the two most common causes are:

- missing `[llm].model` in config
- missing `OPENAI_API_KEY`

## Web UI Notes

The browser UI is served by `mem-service` itself. In a normal install it should work automatically once the service is running.

If you build from source, build the frontend first:

```bash
npm --prefix web ci
npm --prefix web run build
```

Then start the backend:

```bash
cargo run --bin memory -- service run
```

## Importing Commit History

Memory Layer can also store git commits as project evidence without turning every commit into canonical memory.

Import recent or full history:

```bash
memory commits sync --project my-project
```

Browse imported commits:

```bash
memory commits list --project my-project
memory commits show --project my-project <commit-hash>
```

If `memory doctor` reports that no commit history has been imported yet, the fix is:

```bash
memory commits sync --project my-project
```

## Running From Source

If you are developing Memory Layer itself:

```bash
cargo run --bin memory -- wizard
cargo run --bin memory -- service run
cargo run --bin memory -- tui --project memory
```

Optional watcher:

```bash
cargo run --bin memory -- watcher run --project memory
```

## Related Docs

- [User Documentation](README.md)
- [Memory Bundles](cli/bundles.md)
- [Scan Command](cli/scan.md)
- [Commit History](cli/commits.md)
- [Developer Documentation](../developer/README.md)
