---
name: memory-layer
version: 0.9.4
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
- `memory-review-proposals`
  - review pending curation replacement proposals interactively
  - explain why each candidate was proposed and gather proof before approve/reject
- `memory-project-init`
  - initialise or refresh repo-local Memory Layer setup for a target project
  - preview `.mem/` and `.agents/` writes before applying setup
- `memory-github-init`
  - guide first-time GitHub repository onboarding
  - inspect remotes, Actions, secrets, variables, branch protection, and Memory setup before dry-run changes
- `memory-plan-execution`
  - save the approved plan and checkpoint at execution start
  - verify all plan checkbox items are complete before claiming the task is finished
- `memory-direct-task-start`
  - save a task memory before direct no-plan implementation work begins
- `memory-remember`
  - remember meaningful completed work after the task is done
  - remember distilled code or codebase explanations after answering explanation requests

Use this umbrella skill when the turn is mixed, ambiguous, or about the Memory Layer workflow itself.

## Shared Invariants

These rules apply across the whole memory skill bundle:

1. Query memory before answering project-specific questions.
2. Use `resume` instead of a generic query for interruption-recovery prompts.
3. Use proposal review for pending curation replacement proposals; approve or reject only after an explicit review decision.
4. Use the project-init workflow for repo-local Memory Layer setup requests.
5. Use the GitHub init workflow for first-time GitHub repository onboarding.
6. Save the approved plan before implementation begins when a planning phase turns into execution.
7. When an actionable user instruction starts execution without an approved plan, save a task memory before implementation begins.
8. Verify plan-backed work is complete before claiming the task is finished.
9. Remember meaningful work after it is actually done.
10. When you explain code, a file, a module, or the whole codebase, remember the distilled reusable explanation after answering.
11. Prefer insufficient evidence over unsupported conclusions.
12. Never invent provenance.

## Mandatory post-task rule

After any meaningful repository work, run the remember workflow before sending the final response unless one of these is true:
- no durable knowledge was produced
- the work was purely trivial
- the user explicitly asked not to store memory

This skill should default to storing durable project knowledge, not waiting for the user to ask again.

## Code explanation memory rule

When the user asks for an explanation of code, a file, a module, an architecture path, or the whole codebase:

1. Query memory before answering, as with any project-specific question.
2. Inspect the relevant code or docs before making claims.
3. Answer the user normally.
4. After answering, run the remember workflow if the explanation produced durable reusable understanding.

Store a distilled memory, not the whole chat answer. Use `--type project`, a title like `Explained <file/module/codebase>`, and notes that capture the stable explanation plus file, module, or symbol provenance. Do not use `--file-changed` unless files actually changed.

Skip remembering when the explanation was trivial, speculative, duplicate, or not grounded in inspected code or existing memory.

## Shared Script Home

The shared scripts and references still live here:

- `./.agents/skills/memory-layer/scripts/`
- `./.agents/skills/memory-layer/references/`

The focused skills call into those shared scripts instead of duplicating them.

The shared helper is now Go-based:

- run helpers with `go run ./.agents/skills/memory-layer/scripts/main.go <command> ...`
- `go` must be available on `PATH`

## Direct Task Start Rule

When the user gives an actionable implementation instruction without first approving a plan, record the start of that work with:

```bash
go run ./.agents/skills/memory-layer/scripts/main.go start-task-execution \
  --project <project-slug> \
  --title "<short task title>" \
  --prompt "<original user request>"
```

Do not use this for pure questions, planning-only turns, trivial read-only answers, or work that already has an approved plan. Completed direct tasks still use `memory-remember` afterward to record what was implemented.

The helper/CLI verifies that the resulting active `task` memory exists. If it fails, stop and surface the failure instead of proceeding silently.

## Model Routing

For docs/admin subtasks about Memory Layer itself, prefer a cheaper docs/admin model when available.

Keep these on the stronger engineering path:

- query and resume
- plan start/finish verification
- remember/capture/curate
- debugging or memory-behavior investigation
