---
name: memory-layer-codex
description: Use Memory Layer MCP tools and the installed memory CLI to retrieve project context, resume work safely, and preserve completed work in Codex.
---

# Memory Layer for Codex

Use this skill for project-specific questions, interruption recovery, Memory
Layer setup, or durable work capture in a repository that uses Memory Layer.

## Prerequisites

This plugin requires the `memory` CLI on `PATH` and a reachable Memory Layer
service. It does not install the CLI, change service configuration, or manage
credentials. If a tool or CLI command reports a missing binary, unavailable
service, or authentication error, explain the prerequisite and stop before
changing system configuration.

## Read and resume workflow

1. For a project-specific question, call `memory_query` before answering.
2. For an interrupted task, call `memory_resume`; do not replace it with a
   generic query.
3. For a newly assigned or unfamiliar repository, call `memory_up_to_speed`.
4. For work that might belong to another repository, call
   `memory_search_all` first. Use the returned `project` and `repo_root` to
   select the repository before making changes.
5. Treat Memory Layer results as evidence. Prefer insufficient evidence over
   unsupported conclusions.

## Project setup

Use `memory wizard --dry-run` before initialising a repository, then review
the proposed changes. `memory init` or `memory wizard` installs the canonical
repo-local `.agents/skills/` bundle. Those repo-local skills remain
authoritative; this desktop plugin does not replace or update them.

## Write workflow

For direct implementation without an approved plan, record the start before
editing:

```bash
memory checkpoint start-task --title "<short title>" --prompt "<user request>"
```

For approved plan execution, save the checked plan and checkpoint before
editing:

```bash
memory checkpoint start-execution --plan-file /tmp/approved-plan.md
```

Before claiming an approved plan is complete, run:

```bash
memory checkpoint finish-execution
```

After meaningful completed work, capture the verified outcome with
`memory remember`, including a concise summary, durable note, changed files,
and relevant validation results. Use `--type refactor` only for an intentional
behavior-preserving restructuring.

## Safety

Use the existing MCP tools only. Do not add, bypass, or infer authorization.
Memory Layer loop operations remain subject to their existing approval and
role checks.
