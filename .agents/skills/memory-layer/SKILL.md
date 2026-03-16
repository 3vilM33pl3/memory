---
name: memory-layer
description: Query project memory before answering project-specific questions; capture completed task context; curate raw captures into durable canonical memory with provenance.
---

# Memory Layer Skill

Use this skill when:
- the user asks how this repository works
- you discover a durable convention, decision, or debugging lesson
- you complete work that should be captured for later retrieval
- the user explicitly asks to store or query memory

Do not use this skill for:
- generic questions with no project-specific context
- speculative facts without provenance
- trivial temporary notes

## Scripts

Query memory:
```bash
./.agents/skills/memory-layer/scripts/query-memory.sh "<question>"
```

Capture task context:
```bash
./.agents/skills/memory-layer/scripts/capture-task.sh <payload.json>
```

Curate pending captures:
```bash
./.agents/skills/memory-layer/scripts/curate-memory.sh
```

## Workflow

1. Query memory before answering project-specific questions.
2. Capture meaningful task outcomes once work is complete.
3. Curate after capture so durable memory becomes searchable.
4. Prefer insufficient evidence over unsupported conclusions.
5. Never invent provenance.

## Capture guidance

Payloads should include:
- `project`
- `task_title`
- `user_prompt`
- `agent_summary`
- `files_changed`
- `tests`
- `notes`

Only capture verified outcomes and durable lessons.
