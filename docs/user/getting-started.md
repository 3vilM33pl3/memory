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
- an API token string for local write-capable tools
- a unique writer ID for the coding agent or tool that will write memory, for example `codex-cli-main`
- optional: an OpenAI-compatible API key if you want `mem-cli scan`
- PostgreSQL with `pgvector` installed if you want semantic retrieval

## Fast Install: Debian

1. Download the latest `.deb` package from the GitHub Releases page.
2. Install it:

```bash
sudo dpkg -i memory-layer_<version>_amd64.deb
```

3. Configure the shared/global settings once on this machine:

```bash
mem-cli wizard --global
```

This is where you set the shared database URL, API token, and a default `writer.id`.

4. Go to the project you want to use:

```bash
cd /path/to/your-project
```

5. Run the repo-local setup wizard:

```bash
mem-cli wizard
```

6. Start the backend service:

```bash
sudo systemctl enable --now memory-layer.service
```

7. Open the UI you prefer:

```bash
mem-cli tui
```

## Fast Install: macOS

1. Install from the local Homebrew formula or your tap:

```bash
brew install --HEAD ./packaging/macos/homebrew/memory-layer.rb
```

2. Configure the shared/global settings once on this machine:

```bash
mem-cli wizard --global
```

3. Go to the project you want to use:

```bash
cd /path/to/your-project
```

4. Run the repo-local setup wizard:

```bash
mem-cli wizard
```

5. Start the backend LaunchAgent:

```bash
mem-cli service enable
```

6. Open the TUI:

```bash
mem-cli tui
```

or in a browser:

```text
http://127.0.0.1:4040/
```

## What The Wizard Will Ask For

The wizard can set up:

- shared/global settings when that scope is enabled:
  - the PostgreSQL database URL
  - the write API token
  - the default `writer.id`
- optional LLM settings for `scan`
- repo-local `.mem/` files
- optional watcher setup

Important detail:

- inside a repository, `mem-cli wizard` is local-first by default
- use `mem-cli wizard --global` when you want to edit the shared/global config
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
- `service.api_token`
- `[cluster]` settings if you want relay discovery on a local network
- `[llm]` settings

### Repo-local config

Use this for project-specific overrides:

- watcher settings
- local backend ports
- project-specific DB override if needed
- repo-specific `writer.id` override if one project should write under a different writer identity

### Project memory behavior

Use `.agents/memory-layer.toml` for project-owned behavior that should be easy to adapt without digging through service config:

- include and ignore path hints for repository scans
- enabled analyzers
- future graph and plugin controls

### Env files

Use these for secrets such as:

```bash
OPENAI_API_KEY=your-api-key-here
```

## Writer ID

Each coding agent or tool that writes memory should have a unique writer ID.

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

If you do not configure this, write-capable commands such as `remember`, `scan`, and `memory-watch` will fail.

## Primary And Relay Services

If a machine can reach PostgreSQL, `mem-service` runs as a `primary`.

If a machine cannot reach PostgreSQL but can see another Memory Layer service on the local network, `mem-service` can run as a `relay`. In relay mode it discovers a primary over UDP multicast and forwards the normal HTTP API and browser WebSocket traffic to it.

## Daily Use

Open the TUI:

```bash
mem-cli tui
```

Open the web UI:

```text
http://127.0.0.1:4040/
```

Check health:

```bash
mem-cli service status
mem-cli health
mem-cli doctor
```

Save a useful project fact:

```bash
mem-cli remember --project my-project --note "Deployment uses a systemd service."
```

Search project memory:

```bash
mem-cli query --project my-project --question "How is deployment handled here?"
```

## Optional Background Watcher

If you want Memory Layer to capture useful work in the background:

```bash
mem-cli watch enable --project my-project
```

When the backend service restarts, service-managed watchers will restart too so they reconnect cleanly to the new backend instance.

Check it:

```bash
mem-cli watch status --project my-project
```

Disable it later:

```bash
mem-cli watch disable --project my-project
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
mem-cli doctor
```

6. rebuild embeddings for existing project memories:

```bash
mem-cli reindex --project my-project
```

If `mem-cli doctor` reports that `pgvector` is missing, install the PostgreSQL package first and rerun the check.

On Debian, upgrades should preserve local edits to:

- `/etc/memory-layer/memory-layer.env`
- `/etc/memory-layer/memory-layer.toml`

Those files are treated as package-managed configuration files rather than being overwritten
with package defaults on every upgrade.

## Using `scan`

`scan` reads a repository, sends a structured summary to the configured LLM, and writes useful durable memories back into Memory Layer.

Full command documentation:

- [Scan Command](cli/scan.md)

Try it safely first:

```bash
mem-cli scan --project my-project --dry-run
```

Then write the results:

```bash
mem-cli scan --project my-project
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
cargo run --bin mem-service
```

## Importing Commit History

Memory Layer can also store git commits as project evidence without turning every commit into canonical memory.

Import recent or full history:

```bash
mem-cli commits sync --project my-project
```

Browse imported commits:

```bash
mem-cli commits list --project my-project
mem-cli commits show --project my-project <commit-hash>
```

If `mem-cli doctor` reports that no commit history has been imported yet, the fix is:

```bash
mem-cli commits sync --project my-project
```

## Running From Source

If you are developing Memory Layer itself:

```bash
cargo run --bin mem-cli -- wizard
cargo run --bin mem-service
cargo run --bin mem-cli -- tui --project memory
```

Optional watcher:

```bash
cargo run --bin memory-watch -- run --project memory
```

## Related Docs

- [User Documentation](README.md)
- [Scan Command](cli/scan.md)
- [Commit History](cli/commits.md)
- [Developer Documentation](../developer/README.md)
