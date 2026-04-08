---
name: memory-plan-execution
description: Save the approved plan and checkpoint when execution starts, and verify all plan checkbox items are complete before claiming the task is finished
---

# Memory Plan Execution Skill

Use this skill when:
- a planning session has ended and the user approved execution
- plan-backed work is underway and the agent needs to verify completion
- the approved plan changed materially and needs to be resaved for the same thread

Do not use this skill for:
- general project questions
- interruption recovery without a plan transition
- post-task remembering once the work is already verified complete

## Scripts

Save the approved plan and checkpoint together:

```bash
./.agents/skills/memory-layer/scripts/start-plan-execution.sh \
  --project <project-slug> \
  --plan-file /tmp/approved-plan.md
```

Verify the approved plan is fully executed:

```bash
./.agents/skills/memory-layer/scripts/finish-plan-execution.sh \
  --project <project-slug>
```

Optional explicit checkpoint-only helper:

```bash
./.agents/skills/memory-layer/scripts/checkpoint-project.sh \
  --project <project-slug> \
  --note "Plan approved; starting implementation"
```

## Workflow

1. When the user approves a real plan, run `start-plan-execution.sh` before implementation starts.
2. The approved plan must contain Markdown checkbox items so completion can be verified later.
3. If the plan changes materially during execution, save the revised approved plan first with the same thread key.
4. Before claiming plan-backed work is finished, run `finish-plan-execution.sh`.
5. Do not present the task as finished if any checkbox item remains unchecked.

## Model Routing

Keep this skill on the stronger engineering path.
