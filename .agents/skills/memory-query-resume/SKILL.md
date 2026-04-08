---
name: memory-query-resume
description: Query curated project memory before answering repo-specific questions, and use resume to get back into flow after interruptions
---

# Memory Query And Resume Skill

Use this skill when:
- the user asks how this repository works
- the user asks a project-specific question that should be grounded in stored memory
- the user returns after an interruption and wants to get back into flow
- the user asks what changed since they were last here

Do not use this skill for:
- plan approval or execution-start transitions
- plan-completion verification
- post-task memory capture after work is done

## Scripts

Query curated project memory:

```bash
./.agents/skills/memory-layer/scripts/query-memory.sh "<question>"
```

Resume a project after an interruption:

```bash
./.agents/skills/memory-layer/scripts/resume-project.sh [project-slug]
```

## Workflow

1. Query memory before answering project-specific questions.
2. For interruption-recovery prompts, use `resume-project.sh` instead of a generic query.
3. Use the returned evidence or resume pack in your answer.
4. Prefer insufficient evidence over unsupported conclusions.
5. Never invent provenance.

## Model Routing

Keep this skill on the stronger engineering path.
