# Memory Layer Skill

This page explains the current repo-local `memory-layer` skill workflow. The canonical live skill is:

- [`.agents/skills/memory-layer/SKILL.md`](/home/olivier/Projects/memory/.agents/skills/memory-layer/SKILL.md)

Use this page to understand how the live skill is intended to behave without reading the raw skill file line by line.

## Table of Contents

- [Canonical Source](#canonical-source)
- [What The Skill Is For](#what-the-skill-is-for)
- [Current Workflow](#current-workflow)
- [Helper Scripts](#helper-scripts)
- [What Changed From The Older Flow](#what-changed-from-the-older-flow)
- [Related Docs](#related-docs)

## Canonical Source

The repo-local skill in `.agents/skills/memory-layer/` is the source of truth.

That matters because the skill evolves with the product. The packaged `skill-template` and the example skill are derived from this workflow, but the live repo-local skill is the one the agent actually uses in this repository.

## What The Skill Is For

The `memory-layer` skill is the main driver for coding agent interaction with Memory Layer.

It tells the agent when to:

- query project memory before answering repo-specific questions
- use `resume` to get back into flow after an interruption
- save the approved plan and checkpoint when a planning phase turns into approved execution
- remember meaningful completed work automatically

The skill decides **when** to use memory. The helper scripts under `.agents/skills/memory-layer/scripts/` are the execution path that actually calls `memory`.

## Current Workflow

The current live workflow is:

1. Query memory before answering project-specific questions.
2. If the user is returning after an interruption, use the resume workflow instead of a generic query.
3. If a plan was approved and execution is about to start, run `memory checkpoint start-execution` through the skill helper so the checkpoint and approved plan are both stored before implementation begins.
4. After meaningful work is complete, use the automatic remember workflow.
5. Prefer insufficient evidence over unsupported conclusions.
6. Never invent provenance.

The important change is that the skill is no longer centered on a manual `memory capture task -> curate-memory` sequence for normal work. The preferred path is now `remember-task.sh`, which captures and curates in one step.

## Helper Scripts

The main scripts are:

- `query-memory.sh`
  - query existing curated project memory before answering
- `resume-project.sh`
  - build a resume briefing from the saved checkpoint and project timeline
- `checkpoint-project.sh`
  - save a checkpoint explicitly when you want to mark a point in time without storing a plan
- `start-plan-execution.sh`
  - save the checkpoint and store the approved execution plan as `plan` memory before implementation starts
- `remember-task.sh`
  - capture completed work and curate it immediately into durable memory

Older lower-level scripts still exist:

- `capture-task.sh`
- `curate-memory.sh`

Those are still useful as lower-level building blocks, but they are not the main workflow the current live skill pushes agents toward.

## What Changed From The Older Flow

Older documentation in this repo focused on:

- query memory
- build a capture payload manually
- run `memory capture task`
- run `curate-memory`

The current skill has moved beyond that. It now also covers:

- interruption recovery with `resume`
- plan-to-execution checkpointing with approved-plan capture
- automatic post-task remembering with `remember-task.sh`

If you are updating other documentation or examples, align them to the live skill instead of treating the older manual capture/curate flow as the default.

## Related Docs

- [How Skills Work](how-skills-work.md)
- [Skills And Agent Material](README.md)
- [Architecture Overview](../architecture/overview.md)
- [How Memory Layer Works](../architecture/how-it-works.md)
- [Agent Example Skill](../examples/agent-example/skills/memory-layer/SKILL.md)
