---
name: memory-remember
description: Remember meaningful completed work by capturing task context and curating it into durable project memory with provenance
---

# Memory Remember Skill

Use this skill when:
- meaningful repository work is complete
- the agent has durable facts, conventions, debugging lessons, or decisions to store
- the user explicitly asks to store completed work as memory

Do not use this skill for:
- project questions that should use query or resume
- plan approval or plan-completion verification
- trivial temporary notes

## Script

Remember task context automatically:

```bash
go run ./.agents/skills/memory-layer/scripts/main.go remember-task \
  --title "<task title>" \
  --prompt "<user prompt>" \
  --summary "<what changed>" \
  --note "<durable fact>"
```

## Workflow

1. Use the automatic remember workflow after meaningful work is actually complete.
2. Run it after any required plan-completion verification has already succeeded.
3. Provide one or more `--note` values for durable facts.
4. Only store verified outcomes and durable lessons.
5. If title, prompt, or summary are omitted, let the helper derive them from the repo state when possible.

## Model Routing

Keep this skill on the stronger engineering path.

## Runtime Requirement

This focused skill uses the shared Go helper under `.agents/skills/memory-layer/scripts/`.
`go` must be available on `PATH` for these helper commands to run.
