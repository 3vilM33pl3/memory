---
name: memory-layer
description: Umbrella entrypoint for Memory Layer workflows; use for broad memory-related turns, shared invariants, and docs/admin work, while focused query/resume, plan-execution, and remember skills handle the narrow workflows
---

# Memory Layer Umbrella Skill

Use this skill when:
- the turn is broadly about Memory Layer behavior rather than one narrow workflow phase
- the user explicitly asks about the memory system or how the memory skills work
- the task spans multiple memory phases in one turn
- the task is docs/admin oriented and clearly about Memory Layer itself

Do not use this skill for:
- generic questions with no project-specific context
- speculative facts without provenance
- turns that clearly belong to one focused memory skill

## Focused Skills

Prefer the focused skills when the task is clearly one of these:

- `memory-query-resume`
  - query project memory before repo-specific answers
  - resume a project after an interruption
- `memory-plan-execution`
  - save the approved plan and checkpoint at execution start
  - verify all plan checkbox items are complete before claiming the task is finished
- `memory-remember`
  - remember meaningful completed work after the task is done

Use this umbrella skill when the turn is mixed, ambiguous, or about the Memory Layer workflow itself.

## Shared Invariants

These rules apply across the whole memory skill bundle:

1. Query memory before answering project-specific questions.
2. Use `resume` instead of a generic query for interruption-recovery prompts.
3. Save the approved plan before implementation begins when a planning phase turns into execution.
4. Verify plan-backed work is complete before claiming the task is finished.
5. Remember meaningful work after it is actually done.
6. Prefer insufficient evidence over unsupported conclusions.
7. Never invent provenance.

## Mandatory post-task rule

After any meaningful repository work, run the remember workflow before sending the final response unless one of these is true:
- no durable knowledge was produced
- the work was purely trivial
- the user explicitly asked not to store memory

This skill should default to storing durable project knowledge, not waiting for the user to ask again.

## Shared Script Home

The shared scripts and references still live here:

- `./.agents/skills/memory-layer/scripts/`
- `./.agents/skills/memory-layer/references/`

The focused skills call into those shared scripts instead of duplicating them.

The shared helper is now Go-based:

- run helpers with `go run ./.agents/skills/memory-layer/scripts <command> ...`
- `go` must be available on `PATH`

## Model Routing

For docs/admin subtasks about Memory Layer itself, prefer a cheaper docs/admin model when available.

Keep these on the stronger engineering path:

- query and resume
- plan start/finish verification
- remember/capture/curate
- debugging or memory-behavior investigation
