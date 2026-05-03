# `memory remember`

Use `remember` when you want to store a durable project fact or task outcome directly from the CLI.

`remember` is the simplest write path into Memory Layer. It captures task context and runs curation so the result becomes queryable without a separate manual curate step.

## Table of Contents

- [When To Use It](#when-to-use-it)
- [Requirements](#requirements)
- [Basic Examples](#basic-examples)
- [What It Writes](#what-it-writes)
- [Troubleshooting](#troubleshooting)
- [When Not To Use It](#when-not-to-use-it)

## When To Use It

Use `remember` for:

- implemented outcomes that should be easy to find later
- architecture decisions
- conventions and workflow rules
- durable debugging lessons
- environment facts that will matter later
- verified task outcomes worth keeping
- distilled explanations of code, files, modules, architecture paths, or the whole codebase

For direct no-plan work, agents should record a `task` memory with
`memory checkpoint start-task` when execution starts, then use `remember` after
completion to record the `implementation` memory.

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

Store a distilled code explanation after answering an explanation request:

```bash
memory remember \
  --project my-project \
  --type project \
  --title "Explained crates/mem-cli/src/main.rs" \
  --prompt "Explain how CLI help is generated." \
  --summary "Explained the Clap-based help constants and command metadata." \
  --note "CLI help is generated from Clap command metadata and after_help constants in crates/mem-cli/src/main.rs."
```

Explanation memories should capture stable reusable understanding, not the full chat answer. Do not use `--file-changed` for explanation-only turns unless files actually changed.

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

## Troubleshooting

`remember` performs two writes: capture, then curate. If the command times out,
is interrupted, or appears to hang, do not assume nothing happened. The capture
may already be stored, and curation may already have produced queryable memory.

Check for a captured task:

```bash
memory activities --project my-project --kind capture_task --limit 10
```

If you see the capture but no queryable memory yet, run a bounded curation pass:

```bash
memory curate --project my-project --batch-size 5
```

Then verify with a targeted query:

```bash
memory query \
  --project my-project \
  --question "What did we just remember about deployment?" \
  --limit 5 \
  --json
```

Use `--dry-run` to isolate argument and payload problems without writing:

```bash
memory remember --project my-project --note "Deployment uses systemd." --dry-run
```

Interpretation:

- `activities` shows `capture_task`: raw evidence was written.
- `curate` reports `input_count = 0`: there may be no pending raw captures, or
  the previous `remember` already consumed them.
- `query` returns the new memory: the remember operation succeeded even if the
  original process did not return cleanly.

## When Not To Use It

Do not use `remember` for:

- temporary scratch notes
- unverified guesses
- duplicate low-value chatter
- speculative or trivial explanations that are not grounded in inspected code or existing memory
- large repo bootstrap work where `scan` is the better tool

For project-wide repository extraction, use [Scan Command](scan.md).
For agent-driven task capture, use the repo-local skill workflow described in the developer docs.
For the full developer explanation of each curated memory category, use [Memory Types Reference](../../developer/architecture/memory-types.md).
