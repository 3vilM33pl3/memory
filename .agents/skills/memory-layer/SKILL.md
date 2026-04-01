---
name: memory-layer
description: Query project memory before answering project-specific questions; capture completed task context; curate raw captures into durable canonical memory with provenance; checkpoint plans before execution; use memory to get back into flow after interruptions
---

# Memory Layer Skill

Use this skill when:
- the user asks how this repository works
- you discover a durable convention, decision, or debugging lesson
- you complete meaningful work in this repository
- the user explicitly asks to store or query memory
- the user returns after an interruption and wants to get back into flow
- you transition from planning to execution and want to checkpoint the plan for later resumption

Do not use this skill for:
- generic questions with no project-specific context
- speculative facts without provenance
- trivial temporary notes

## Scripts

Query memory:
```bash
./.agents/skills/memory-layer/scripts/query-memory.sh "<question>"
```

Resume a project after an interruption:
```bash
./.agents/skills/memory-layer/scripts/resume-project.sh [project-slug]
```

Save a checkpoint after a planning session transitions into approved execution:
```bash
./.agents/skills/memory-layer/scripts/checkpoint-project.sh \
  --project <project-slug> \
  --note "Plan approved; starting implementation"
```

Remember task context automatically:
```bash
./.agents/skills/memory-layer/scripts/remember-task.sh \
  --title "<task title>" \
  --prompt "<user prompt>" \
  --summary "<what changed>" \
  --note "<durable fact>"
```

## Workflow

1. Query memory before answering project-specific questions.
2. For "get me back into flow" or "what changed since I was last here?" prompts, use the resume script instead of a generic query.
3. If you produce a proposed plan and the user approves execution, save a checkpoint immediately before starting implementation.
4. Use the automatic remember workflow once work is complete.
5. The remember workflow captures and curates in one step.
6. Prefer insufficient evidence over unsupported conclusions.
7. Never invent provenance.

## Mandatory post-task rule

After any meaningful repository work, run the remember workflow before sending the final response unless one of these is true:
- no durable knowledge was produced
- the work was purely trivial
- the user explicitly asked not to store memory

This skill should default to storing durable project knowledge, not waiting for the user to ask again.

## Planning transition rule

When a turn has a real planning phase and the user then approves execution, save a checkpoint before implementation starts.

Use a short note that explains the transition, for example:
- `Plan approved; starting implementation`
- `Plan approved; beginning refactor`
- `Plan approved; moving to execution`

This makes `mem-cli resume` useful when the user returns after delegating or switching projects.

## Remember guidance

The automatic remember workflow should be used after meaningful work. It:
- defaults the project slug from the current directory
- auto-detects changed files from `git status` when possible
- captures task context
- immediately curates it into canonical memory

Provide:
- one or more `--note` values for durable facts

Optionally provide:
- `--title`
- `--prompt`
- `--summary`
- `--test-passed "<command>"`
- `--test-failed "<command>"`
- `--command-output-file <path>`

Only store verified outcomes and durable lessons.

If title, prompt, or summary are omitted, the remember command derives sensible defaults from the current project and changed files. Use that defaulting so memory capture stays lightweight and automatic.
