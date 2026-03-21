# Memory Layer

Memory Layer is a local knowledge base for software projects.

It lets you save useful facts about a codebase, search them later, and view them in a terminal UI. It is designed for both humans and coding agents, so project knowledge does not get lost in chat history, terminal scrollback, or old commits.

![Memory Layer TUI](docs/img/tui-overview.png)

## What It Does

- stores project memories in PostgreSQL
- keeps memories separated per project
- lets you search and browse them in a TUI
- can capture useful work automatically while you code
- can scan an existing repository and suggest durable knowledge

## The Main Parts

- `mem-service`: the shared backend service
- `mem-cli`: the command-line tool and TUI
- `memory-watch`: the optional background watcher
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

5. Open the TUI:

```bash
mem-cli tui
```

## What You Need Before Setup

- a PostgreSQL database connection string
- a project folder where you want Memory Layer enabled
- optional: an OpenAI-compatible API key if you want to use `mem-cli scan`

If you do not want to use `scan`, you can ignore the LLM settings.

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
  - `~/.config/memory-layer/memory-layer.toml` for local installs
- repo-local config:
  - `.mem/config.toml` inside each project

Use the global config for shared values such as:

- `database.url`
- `service.api_token`
- `[llm]` settings

Use `.mem/config.toml` for project-specific overrides such as:

- project-local backend ports
- watcher behavior
- repo-local database override if needed

Secrets can also live in env files:

- shared: `/etc/memory-layer/memory-layer.env` or `~/.config/memory-layer/memory-layer.env`
- repo-local override: `.mem/memory-layer.env`

Example:

```bash
OPENAI_API_KEY=your-api-key-here
```

## First Run In A Project

After the wizard completes:

1. start the backend if it is not already running
2. optionally enable the watcher
3. open the TUI

Commands:

```bash
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

Check the backend:

```bash
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

## TUI Tabs

- `Memories`: browse all stored memories for the project
- `Query`: ask a question and inspect the memories returned
- `Activity`: see recent queries and backend activity
- `Project`: see health, counts, and configuration summary

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
