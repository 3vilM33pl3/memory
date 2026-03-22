# Memory Layer

Memory Layer is a local knowledge base built for coding agents such as Codex CLI and Claude Code.

It lets you save useful facts about a codebase, search them later, and view them in a terminal UI or browser. It is designed for both humans and coding agents, so project knowledge does not get lost in chat history, terminal scrollback, or old commits.

![Memory Layer TUI](docs/img/tui-overview.png)

## What It Does

- stores project memories in PostgreSQL
- can run as a primary service with PostgreSQL or as a relay service that forwards to a database-connected peer on the local network
- keeps memories separated per project
- combines lexical search with optional embedding-based recall and related-memory links
- lets you search and browse them in a TUI or browser
- can capture useful work automatically while you code
- can scan an existing repository and suggest durable knowledge
- can import git commit history as searchable project evidence

## The Main Parts

- `mem-service`: the shared backend service
- `mem-cli`: the command-line tool and TUI
- `memory-watch`: the optional background watcher
- web UI served by `mem-service`
- `.agents/skills/memory-layer/`: the repo-local Codex skill installed into each project

## Fastest Install: Debian Package

If you just want to use the tool, this is the easiest path.

1. Download the latest `.deb` from the GitHub Releases page.
2. Install it:

```bash
sudo dpkg -i memory-layer_<version>_amd64.deb
```

3. Run the setup wizard inside the project you want to use:

```bash
cd /path/to/your-project
mem-cli wizard
```

4. Start the shared backend:

```bash
sudo systemctl enable --now memory-layer.service
```

5. Open the UI you want:

```bash
mem-cli tui
```

## Fastest Install: macOS

For macOS, use the Homebrew formula and `launchd`.

1. Install from the formula in this repo or your tap:

```bash
brew install --HEAD ./packaging/macos/homebrew/memory-layer.rb
```

2. Run the setup wizard inside the project you want to use:

```bash
cd /path/to/your-project
mem-cli wizard
```

3. Start the shared backend LaunchAgent:

```bash
mem-cli service enable
```

4. Open the TUI:

```bash
mem-cli tui
```

or open:

```text
http://127.0.0.1:4040/
```

## What You Need Before Setup

- a PostgreSQL database connection string
- a unique `agent.id` for each coding agent that will write memory
- a project folder where you want Memory Layer enabled
- optional: an OpenAI-compatible API key if you want to use `mem-cli scan`

If you do not want to use `scan`, you can ignore the LLM settings.

Current versions of Memory Layer store chunk embeddings with `pgvector`, so your PostgreSQL server needs the `pgvector` extension installed and enabled in the target database.

On Debian or Ubuntu, that is typically:

```bash
sudo apt install postgresql-<your-version>-pgvector
```

## Setup In Plain English

The wizard is the normal way to set things up:

```bash
mem-cli wizard
```

It walks you through:

- the database connection
- the write API token used by the local tools
- optional LLM settings for repository scanning
- repo-local setup in `.mem/`
- optional background watcher setup

### Where Settings Live

Memory Layer uses two configuration levels:

- shared/global config:
  - `/etc/memory-layer/memory-layer.toml` for Debian installs
  - `~/Library/Application Support/memory-layer/memory-layer.toml` for macOS installs
  - `~/.config/memory-layer/memory-layer.toml` for local installs
- repo-local config:
  - `.mem/config.toml` inside each project

Use the global config for shared values such as:

- `database.url`
- `service.api_token`
- `[cluster]` discovery settings if you want relay mode on a LAN
- `[llm]` settings
- `[embeddings]` settings if you want semantic recall
- optional `service.web_root` override if you want `mem-service` to serve web assets from a non-standard location

Use `.mem/config.toml` for project-specific overrides such as:

- project-local backend ports
- watcher behavior
- repo-local database override if needed
- repo-local `agent.id` override if this repository should use a different agent identity

Secrets can also live in env files:

- shared: `/etc/memory-layer/memory-layer.env` or `~/.config/memory-layer/memory-layer.env`
- shared on macOS: `~/Library/Application Support/memory-layer/memory-layer.env`
- repo-local override: `.mem/memory-layer.env`

Example:

```bash
OPENAI_API_KEY=your-api-key-here
```

### Agent IDs

Every write-capable workflow now needs a unique agent ID. This lets multiple agents work on the same project without collapsing their raw captures together before curation.

You can set it in config:

```toml
[agent]
id = "codex-cli-main"
name = "Codex CLI"
```

or with an environment variable:

```bash
export MEMORY_LAYER_AGENT_ID=codex-cli-main
```

### Primary And Relay Mode

`mem-service` now has two runtime modes:

- `primary`: connects to PostgreSQL, runs migrations, stores/query memories directly
- `relay`: cannot reach PostgreSQL, discovers a primary on the local network, and proxies the normal HTTP and browser WebSocket API to it

This is useful when one machine on a LAN can reach the database and another cannot. Relay mode uses UDP multicast discovery by default.

## First Run In A Project

After the wizard completes:

1. start the backend if it is not already running
2. optionally enable the watcher
3. open the TUI

Commands:

```bash
mem-cli service status
mem-cli health
mem-cli watch enable --project my-project
mem-cli tui
```

If you are developing Memory Layer itself from this repository, you can also run it from source:

```bash
cargo run --bin mem-service
cargo run --bin mem-cli -- tui --project memory
```

## Common Commands

Open the TUI:

```bash
mem-cli tui
```

Open the web UI:

```text
http://127.0.0.1:4040/
```

Check the backend:

```bash
mem-cli service status
mem-cli health
mem-cli doctor
```

Search for a memory:

```bash
mem-cli query --project my-project --question "How is deployment handled here?"
```

Remember a completed task:

```bash
mem-cli remember --project my-project --note "Deployment uses a systemd service and local PostgreSQL."
```

Scan an existing repository:

```bash
mem-cli scan --project my-project --dry-run
mem-cli scan --project my-project
```

Import and inspect project commit history:

```bash
mem-cli commits sync --project my-project
mem-cli commits list --project my-project
mem-cli commits show --project my-project <commit-hash>
```

After pgvector is installed, enable semantic recall by configuring `[embeddings]` and rebuilding chunks:

```bash
mem-cli doctor
mem-cli reindex --project my-project
```

## Upgrade Existing Installs

If you are upgrading from an older release, use this order:

1. install the new package or binary
2. install PostgreSQL `pgvector` for your PostgreSQL version
3. enable the extension in the target database:

```sql
CREATE EXTENSION IF NOT EXISTS vector;
```

4. start or restart `mem-service` so it can run the new migrations
5. run:

```bash
mem-cli service enable
mem-cli doctor
mem-cli reindex --project my-project
```

Important notes:

- existing memories stay in the database
- semantic embeddings for existing chunks are rebuilt by `reindex`
- if `vector` is not installed, `mem-service` will fail on startup migrations and `mem-cli doctor` will report the missing extension

## TUI Tabs

- `Memories`: browse all stored memories for the project
- `Query`: ask a question and inspect the memories returned
- `Activity`: see recent queries and backend activity
- `Project`: see health, counts, and configuration summary

## Web UI

The browser UI is served directly by `mem-service` and mirrors the same day-to-day surfaces as the TUI:

- `Memories`
- `Query`
- `Activity`
- `Project`

The web UI uses:

- normal HTTP endpoints for reads and actions
- a browser WebSocket on `/ws` for live project, activity, and watcher updates

By default `mem-service` looks for built web assets in:

- `web/dist` when run from the repository
- `~/.local/share/memory-layer/web` for local installs
- `/usr/share/memory-layer/web` for Debian installs

You can override that with `service.web_root`.

## Automatic Capture

`memory-watch` is optional.

When enabled, it can:

- notice meaningful work while you are coding
- create raw captures during work
- curate those captures into durable memory later

Default automatic behavior:

- capture after 10 minutes of stable meaningful changes
- curate after 3 raw captures
- curate immediately on explicit flush

Enable it:

```bash
mem-cli watch enable --project my-project
```

## Development Setup For This Repository

This repository supports a repo-local parallel dev backend so it does not clash with an installed shared backend.

Recommended flow from this repo root:

```bash
cargo run --bin mem-cli -- wizard
```

In the wizard, set `Local backend endpoints` to `parallel dev`.

Then run:

```bash
cargo run --bin mem-service
cargo run --bin mem-cli -- tui --project memory
```

Optional watcher:

```bash
cargo run --bin memory-watch -- run --project memory
```

## More Documentation

- [Getting Started](docs/getting-started.md)
- [Scan Command](docs/cli/scan.md)
- [Commit History](docs/cli/commits.md)
- [How It Works](docs/architecture/how-it-works.md)
- [Architecture Overview](docs/architecture/overview.md)
- [Hidden Memory Daemon](docs/architecture/hidden-memory-daemon.md)

## Building From Source

Prerequisites:

- Rust with `cargo`
- PostgreSQL

Then:

```bash
cargo build
cargo test
```

To build a Debian package:

```bash
./packaging/build-deb.sh
```
