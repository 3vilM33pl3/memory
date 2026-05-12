---
name: memory-remember
version: 0.8.6
description: Remember meaningful completed work by capturing task context and curating it into durable project memory with provenance
---

# Memory Remember Skill

Use this skill when:
- meaningful repository work is complete
- the agent has durable facts, conventions, debugging lessons, or decisions to store
- the agent has just explained code, a module, a file, an architecture path, or the whole codebase and the explanation is durable
- the user explicitly asks to store completed work as memory

Do not use this skill for:
- project questions that should use query or resume
- plan approval or plan-completion verification
- trivial temporary notes
- speculative or duplicate code explanations that are not grounded in inspected code or existing memory

## Script

Remember task context automatically:

```bash
go run ./.agents/skills/memory-layer/scripts/main.go remember-task \
  --title "<task title>" \
  --prompt "<user prompt>" \
  --summary "<what changed>" \
  --note "<durable fact>"
```

Remember a distilled code explanation:

```bash
go run ./.agents/skills/memory-layer/scripts/main.go remember-task \
  --type project \
  --title "Explained <file/module/codebase>" \
  --prompt "<user explanation request>" \
  --summary "<short explanation summary>" \
  --note "<stable explanation fact with file/module/symbol provenance>"
```

## Workflow

1. Use the automatic remember workflow after meaningful work is actually complete.
2. Run it after any required plan-completion verification has already succeeded.
3. For direct no-plan actionable tasks, a `task` memory should already have been recorded at execution start; this workflow records the completed `implementation` memory.
4. When `remember-task` is called with `--title` and `--prompt` for default/implementation memory, the shared helper now runs idempotent `checkpoint start-task` first as a safety net. Use `--skip-task-start` only when the work is plan-backed, explanation-only, or the task start was intentionally handled elsewhere.
5. Provide one or more `--note` values for durable facts.
6. Only store verified outcomes and durable lessons.
7. For explanation memories, store a distilled reusable summary, not the whole chat answer.
8. Do not use `--file-changed` for explanation-only turns unless files actually changed.
9. If title, prompt, or summary are omitted, let the helper derive them from the repo state when possible.

## Model Routing

Keep this skill on the stronger engineering path.

## Runtime Requirement

This focused skill uses the shared Go helper under `.agents/skills/memory-layer/scripts/`.
`go` must be available on `PATH` for these helper commands to run.
