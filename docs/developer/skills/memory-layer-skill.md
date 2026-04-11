# Memory Layer Skill Bundle

This page explains the current repo-local Memory Layer skill bundle. The canonical live umbrella skill is:

- [`.agents/skills/memory-layer/SKILL.md`](/home/olivier/Projects/memory/.agents/skills/memory-layer/SKILL.md)

The focused skills that sit beside it are:

- [`.agents/skills/memory-query-resume/SKILL.md`](/home/olivier/Projects/memory/.agents/skills/memory-query-resume/SKILL.md)
- [`.agents/skills/memory-plan-execution/SKILL.md`](/home/olivier/Projects/memory/.agents/skills/memory-plan-execution/SKILL.md)
- [`.agents/skills/memory-remember/SKILL.md`](/home/olivier/Projects/memory/.agents/skills/memory-remember/SKILL.md)

Use this page to understand how the bundle is intended to behave without reading every raw skill file line by line.

## Table of Contents

- [Canonical Source](#canonical-source)
- [What The Skill Is For](#what-the-skill-is-for)
- [Current Workflow](#current-workflow)
- [Helper Scripts](#helper-scripts)
- [What Changed From The Older Flow](#what-changed-from-the-older-flow)
- [Related Docs](#related-docs)

## Canonical Source

The repo-local Memory Layer skill bundle in `.agents/skills/` is the source of truth.

That matters because the skill evolves with the product. The packaged `skill-template` and the example skill are derived from this workflow, but the live repo-local bundle is what the agent actually uses in this repository.

## What The Skill Is For

The `memory-layer` umbrella skill is the main driver for coding agent interaction with Memory Layer.

The bundle as a whole tells the agent when to:

- query project memory before answering repo-specific questions
- use `resume` to get back into flow after an interruption
- save the approved plan and checkpoint when a planning phase turns into approved execution
- remember meaningful completed work automatically

The umbrella skill handles broad or mixed memory turns. The focused skills handle the narrow workflow phases. The shared Go helper under `.agents/skills/memory-layer/scripts/` remains the shared execution path that actually calls `memory`.

## Current Workflow

The current live bundle workflow is:

1. `memory-query-resume` handles query-first answers and interruption recovery.
2. `memory-plan-execution` handles execution start and plan-completion verification.
3. `memory-remember` handles post-task remembering once work is actually complete.
4. The umbrella `memory-layer` skill keeps the shared invariants and covers mixed or ambiguous memory turns.
5. Prefer insufficient evidence over unsupported conclusions.
6. Never invent provenance.

The important change is that the bundle is no longer centered on one broad skill or on a manual `memory capture task -> curate-memory` sequence for normal work. The preferred path is now a focused workflow plus `remember-task`, which captures and curates in one step.

## Shared Go Helper

The repo-local skill bundle now uses one shared Go helper:

- `go run ./.agents/skills/memory-layer/scripts/main.go <command> ...`

The supported helper commands are:

- `query-memory`
  - query existing curated project memory before answering
- `resume-project`
  - build a resume briefing from the saved checkpoint and project timeline
- `checkpoint-project`
  - save a checkpoint explicitly when you want to mark a point in time without storing a plan
- `start-plan-execution`
  - save the checkpoint and store the full approved execution plan as `plan` memory before implementation starts
- `finish-plan-execution`
  - verify that every checkbox item in the active approved plan is complete before the agent can claim the task is finished, and record the verified implemented outcome as `implementation` memory
- `remember-task`
  - capture completed work and curate it immediately into durable memory, with `implementation` as the normal completed-work outcome type
- `capture-task`
- `curate-memory`

`go` must be available on `PATH` anywhere the repo-local skill bundle is expected to run. That requirement is specific to the skill helper runtime; the `memory` CLI itself is still the underlying command surface.

## What Changed From The Older Flow

Older documentation in this repo focused on:

- query memory
- build a capture payload manually
- run `memory capture task`
- run `curate-memory`

The current skill bundle has moved beyond that. It now also covers:

- interruption recovery with `resume`
- plan-to-execution checkpointing with approved-plan capture
- strict plan-completion verification before the agent may conclude plan-backed work
- automatic post-task remembering with `remember-task`

If you are updating other documentation or examples, align them to the live skill instead of treating the older manual capture/curate flow as the default.

## Related Docs

- [How Skills Work](how-skills-work.md)
- [Skills And Agent Material](README.md)
- [Architecture Overview](../architecture/overview.md)
- [How Memory Layer Works](../architecture/how-it-works.md)
- [Agent Example Skill](../examples/agent-example/skills/memory-layer/SKILL.md)
