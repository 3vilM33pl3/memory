# `memory wizard`

Use `wizard` to bootstrap Memory Layer configuration either globally for the machine or locally for the current repository.

## Table of Contents

- [Global vs Repo-Local](#global-vs-repo-local)
- [What It Creates](#what-it-creates)
- [Typical Usage](#typical-usage)
- [How It Differs From `init`](#how-it-differs-from-init)

## Global vs Repo-Local

`memory wizard --global` configures shared machine-level settings such as:

- `database.url`
- optional shared `writer.id`
- optional LLM settings, including local Ollama via `provider = "ollama"`

`database.url` must point at a reachable PostgreSQL database before the backend can become healthy. The target database should already have pgvector enabled:

```bash
psql "$DATABASE_URL" -c "SELECT 1;"
psql "$DATABASE_URL" -c "CREATE EXTENSION IF NOT EXISTS vector;"
psql "$DATABASE_URL" -c "SELECT extversion FROM pg_extension WHERE extname = 'vector';"
```

For local and hosted database setup examples, see [Getting Started: PostgreSQL Requirement](../getting-started.md#postgresql-requirement).

The shared service API token is usually generated automatically into the adjacent `memory-layer.env` file. The wizard only needs an explicit token value if you want to override that generated token.
If you leave `writer.id` unset, Memory Layer derives a stable writer identity automatically at runtime.

`memory wizard` inside a repository is project-first and bootstraps user-local project config plus repo-local agent files such as:

- user-local project `config.toml`
- user-local project `memory-layer.env` when needed
- `.mem/project.toml`
- `.agents/memory-layer.toml`
- `.agents/skills/`

Inside a repository, the wizard defaults to repo-local scope unless you explicitly choose shared/global setup.

## What It Creates

At repo scope, the wizard creates:

- user-local project config/state/cache directories
- `.mem/project.toml` as the repo-local project marker
- `.agents/memory-layer.toml`
- a repo-local copy of the Memory Layer skill bundle

Older `.mem/config.toml`, `.mem/memory-layer.env`, and `.mem/runtime/` layouts remain readable as legacy fallback. Use `memory doctor --fix` to copy missing legacy config/env files into the current user-local project layout.

The bundled skills are created from the packaged `skill-template`, or from the repo-local template during source/dev usage.
That repo-local bundle now uses a shared Go helper under `.agents/skills/memory-layer/scripts/`, so `go` must be available on `PATH` anywhere you expect the repo-local skills to run.
The bundle includes these Memory-owned skills:

- `memory-layer`
- `memory-project-init`
- `memory-github-init`
- `memory-query-resume`
- `memory-plan-execution`
- `memory-direct-task-start`
- `memory-remember`

The installed template lives under `/usr/share/memory-layer/skill-template/` for Debian packages, `~/.local/share/memory-layer/skill-template/` for the local Linux install script, `/usr/local/share/memory-layer/skill-template/` for the macOS `.pkg`, `$(brew --prefix)/share/memory-layer/skill-template/` for Homebrew, and `.agents/skills/` during source/dev usage.

## Typical Usage

First machine setup:

```bash
memory wizard --global
```

Then inside each repo:

```bash
cd /path/to/your-project
memory wizard
```

Preview the resulting file, token, and service actions without writing anything:

```bash
memory wizard --dry-run
memory wizard --global --dry-run
```

## How It Differs From `init`

`init` is the lower-level repo bootstrap command.

For normal interactive setup, prefer `wizard`.

Use `init` when you want:

- a more scriptable bootstrap path
- a non-interactive setup flow
- direct control over the generated repo-local files

Both `memory wizard` and `memory init` support `--dry-run` for preview-only setup.

For the full onboarding flow, see [Getting Started](../getting-started.md).
