# Install and Onboarding Section

## Purpose

Help a new user install Memory Layer, configure storage, initialise a project, and confirm that the service works.

## Section Navigation

```text
Install overview
  Install
  Requirements

Local install
  Linux / Debian
  macOS / Homebrew
  From source

Database
  PostgreSQL and pgvector

Configuration
  Global wizard
  Project wizard
  Service setup

Maintenance
  Update
  Uninstall
```

## Install Overview Page

Give users the shortest path to success while routing them to platform-specific details.

```mdx
# Install

Install Memory Layer, configure PostgreSQL with pgvector, and initialise your first project.

## Recommended path

1. Install the package.
2. Configure global settings with `memory wizard --global`.
3. Configure a project with `memory wizard`.
4. Start the service.
5. Run `memory doctor` and `memory health`.
6. Open the TUI or web UI.
```

## Requirements Page

Cover supported OS, PostgreSQL, pgvector, optional embedding providers, optional LLM providers, Go requirement for repo-local skills, Git requirement for commit sync, network requirements, and disk/storage expectations.

## Linux / Debian Page

Include:

```bash
sudo dpkg -i memory-layer_<version>_amd64.deb
memory wizard --global
cd /path/to/project
memory wizard --dry-run
memory wizard
sudo systemctl enable --now memory-layer.service
memory doctor
memory health
memory tui
```

Also include how to find the latest `.deb`, verify installation, view service logs, restart the service, and handle common `dpkg` issues.

## macOS / Homebrew Page

Include:

```bash
brew tap 3vilM33pl3/memory https://github.com/3vilM33pl3/memory
brew install 3vilM33pl3/memory/memory-layer
memory wizard --global
cd /path/to/project
memory wizard --dry-run
memory wizard
memory service enable
memory doctor
memory health
memory tui
```

For unreleased changes:

```bash
brew install --HEAD 3vilM33pl3/memory/memory-layer
```

## PostgreSQL and pgvector Page

This deserves its own detailed page.

Cover local vs hosted PostgreSQL, creating a dedicated user and database, creating the extension, testing the connection, permission errors, and extension errors.

Example:

```bash
sudo -u postgres createuser --pwprompt memory_layer
sudo -u postgres createdb --owner=memory_layer memory_layer

export DATABASE_URL='postgres://memory_layer:<password>@127.0.0.1:5432/memory_layer'

psql "$DATABASE_URL" -c "SELECT 1;"
psql "$DATABASE_URL" -c "CREATE EXTENSION IF NOT EXISTS vector;"
psql "$DATABASE_URL" -c "SELECT extversion FROM pg_extension WHERE extname = 'vector';"
```

## Global Wizard Page

Explain what global configuration means, where the file is stored, required values, optional values, secrets, and how to re-run safely.

## Project Wizard Page

Explain the difference between global and repo-local config, why `.mem/project.toml` exists, why `.agents/` files exist, how to preview changes with `--dry-run`, and how to preserve existing config.

## Service Setup Page

Cover Linux `systemd`, macOS service setup, running manually, checking status, restarting, and viewing logs.

## Update Page

Cover updating packages, running migrations, checking health, handling breaking changes, and backing up before upgrades.

## Uninstall Page

Cover package removal, disabling service, preserving or deleting database data, global config, and repo-local config. Warn that uninstalling the app should not automatically delete PostgreSQL data unless explicitly requested.
