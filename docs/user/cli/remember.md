# `memory remember`

Use `remember` when you want to store a durable project fact or task outcome directly from the CLI.

`remember` is the simplest write path into Memory Layer. It captures task context and runs curation so the result becomes queryable without a separate manual curate step.

## Table of Contents

- [When To Use It](#when-to-use-it)
- [Requirements](#requirements)
- [Basic Examples](#basic-examples)
- [What It Writes](#what-it-writes)
- [When Not To Use It](#when-not-to-use-it)

## When To Use It

Use `remember` for:

- implemented outcomes that should be easy to find later
- architecture decisions
- conventions and workflow rules
- durable debugging lessons
- environment facts that will matter later
- verified task outcomes worth keeping

This is the normal direct write command for users. Agents often use the higher-level repo-local helper through:

```bash
go run ./.agents/skills/memory-layer/scripts/main.go remember-task ...
```

That helper ultimately drives the same write path.

## Requirements

`remember` always writes with a writer ID, but you usually do not need to configure one manually.

By default, Memory Layer derives a stable writer identity from:

- the writing tool
- the local user
- the local host name

Examples:

- `memory-olivier-monolith`
- `memory-watcher-olivier-monolith`

Set an explicit writer only when you want a custom shared label across tools or machines.

You can configure one with:

```toml
[writer]
id = "codex-cli-main"
```

or:

```bash
export MEMORY_LAYER_WRITER_ID=codex-cli-main
```

CLI and environment overrides still take precedence over config and the derived fallback.

## Basic Examples

Store one durable fact:

```bash
memory remember --project my-project --note "Deployment uses a systemd service."
```

Preview the capture and curate steps without writing:

```bash
memory remember --project my-project --note "Deployment uses a systemd service." --dry-run
```

Store a more explicit remembered task:

```bash
memory remember \
  --project my-project \
  --title "Document deploy convention" \
  --summary "Captured the deploy convention for later reuse." \
  --note "Production deploys are done through a systemd unit restart."
```

## What It Writes

`remember` does not write canonical memory rows directly.

It:

1. creates a normal raw capture
2. runs curation
3. produces canonical durable memory with provenance

That means the result still follows the normal Memory Layer data model:

- raw capture as evidence
- curated memory as the searchable durable result

For normal completed work, `remember` now records an explicit `implementation` memory by default so the shipped outcome is visible in the memory list and query results.

If your notes also contain debugging lessons, decisions, or conventions, curation can still keep those as separate secondary memories when they are justified.

## When Not To Use It

Do not use `remember` for:

- temporary scratch notes
- unverified guesses
- duplicate low-value chatter
- large repo bootstrap work where `scan` is the better tool

For project-wide repository extraction, use [Scan Command](scan.md).
For agent-driven task capture, use the repo-local skill workflow described in the developer docs.
