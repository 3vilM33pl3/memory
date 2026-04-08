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

The umbrella skill handles broad or mixed memory turns. The focused skills handle the narrow workflow phases. The helper scripts under `.agents/skills/memory-layer/scripts/` remain the shared execution path that actually calls `memory`.

## Current Workflow

The current live bundle workflow is:

1. `memory-query-resume` handles query-first answers and interruption recovery.
2. `memory-plan-execution` handles execution start and plan-completion verification.
3. `memory-remember` handles post-task remembering once work is actually complete.
4. The umbrella `memory-layer` skill keeps the shared invariants and covers mixed or ambiguous memory turns.
5. Prefer insufficient evidence over unsupported conclusions.
6. Never invent provenance.

The important change is that the bundle is no longer centered on one broad skill or on a manual `memory capture task -> curate-memory` sequence for normal work. The preferred path is now a focused workflow plus `remember-task.sh`, which captures and curates in one step.

## Helper Scripts

The shared scripts are:

- `query-memory.sh`
  - query existing curated project memory before answering
- `resume-project.sh`
  - build a resume briefing from the saved checkpoint and project timeline
- `checkpoint-project.sh`
  - save a checkpoint explicitly when you want to mark a point in time without storing a plan
- `start-plan-execution.sh`
  - save the checkpoint and store the full approved execution plan as `plan` memory before implementation starts
- `finish-plan-execution.sh`
  - verify that every checkbox item in the active approved plan is complete before the agent can claim the task is finished
- `remember-task.sh`
  - capture completed work and curate it immediately into durable memory

Older lower-level scripts still exist in the shared script directory:

- `capture-task.sh`
- `curate-memory.sh`

Those are still useful as lower-level building blocks, but they are not the main workflow the current live skill pushes agents toward.

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
- automatic post-task remembering with `remember-task.sh`

If you are updating other documentation or examples, align them to the live skill instead of treating the older manual capture/curate flow as the default.

## Related Docs

- [How Skills Work](how-skills-work.md)
- [Skills And Agent Material](README.md)
- [Architecture Overview](../architecture/overview.md)
- [How Memory Layer Works](../architecture/how-it-works.md)
- [Agent Example Skill](../examples/agent-example/skills/memory-layer/SKILL.md)
