# `memory doctor`

Use `doctor` when Memory Layer is installed but something is not working the way you expect.

`memory status` is the recommended first diagnostic command because it aggregates the service, watcher, MCP, and doctor views. Use `doctor` when you want the deeper setup checklist or when `status` points at a fix.

## Table of Contents

- [What It Checks](#what-it-checks)
- [Typical Usage](#typical-usage)
- [Common Failures](#common-failures)

## What It Checks

`memory doctor` checks the current setup for common problems such as:

- missing or placeholder database URL
- missing or placeholder service API token
- unexpected auto-derived or overridden writer identity
- backend connectivity issues
- Ollama reachability and missing local LLM models when `[llm].provider = "ollama"`
- missing `pgvector`
- project bootstrap problems such as missing user-local project config, missing `.mem/project.toml`, or legacy `.mem` config that has not been migrated
- repo-local Memory skill-bundle version and missing/outdated skill files
- repo-local service or watcher configuration issues

The exact output is meant to be actionable, not just descriptive.

## Typical Usage

Run it any time setup looks suspicious:

```bash
memory doctor
```

It is especially useful after:

- first install
- upgrades
- changing database config
- enabling embeddings
- bootstrapping a new repo

## Common Failures

Typical remediation paths are:

- unexpected writer identity
  - set `[writer].id` or `MEMORY_LAYER_WRITER_ID` if you want a custom stable label instead of the auto-derived default
- missing `pgvector`
  - install pgvector on the PostgreSQL server, then enable `vector` in the specific Memory Layer database with `CREATE EXTENSION IF NOT EXISTS vector;`
  - verify the same database URL Memory Layer uses with `psql "$DATABASE_URL" -c "SELECT extversion FROM pg_extension WHERE extname = 'vector';"`
  - see [Getting Started: PostgreSQL Requirement](../getting-started.md#postgresql-requirement) for local Debian, local macOS, and hosted PostgreSQL examples
- repo not initialized
  - run `memory wizard` or `memory init`
- outdated or unversioned repo-local skills
  - run `memory upgrade --dry-run`, then `memory upgrade`
- backend unreachable
  - start the shared backend service or the local development backend
- Ollama unreachable or model missing
  - start Ollama with `ollama serve` and pull the configured model with `ollama pull <model>`
- placeholder service API token
  - run `memory service ensure-api-token --rotate-placeholder` or `memory wizard --global`

Use `doctor` first before assuming the memory database or watcher system is broken.
