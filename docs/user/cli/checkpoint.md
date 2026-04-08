# `memory checkpoint`

`memory checkpoint` manages saved project checkpoints and the plan-backed execution flow.

## Subcommands

### Save

```bash
memory checkpoint save --project memory
memory checkpoint save --project memory --note "Waiting on review"
memory checkpoint save --project memory --dry-run
```

Saves a local checkpoint for later resume use.

### Show

```bash
memory checkpoint show --project memory
```

Shows the currently saved checkpoint for a project.

### Start Execution

```bash
memory checkpoint start-execution --project memory --plan-file /tmp/plan.md
memory checkpoint start-execution --project memory --plan-stdin --thread-key task-123
```

Saves the checkpoint and stores the approved plan as a `plan` memory. The plan must contain Markdown checkbox items.

### Finish Execution

```bash
memory checkpoint finish-execution --project memory
memory checkpoint finish-execution --project memory --plan-file /tmp/plan.md --json
memory checkpoint finish-execution --project memory --dry-run
```

Verifies that every checkbox item in the active approved plan is complete before the work is presented as finished.

## Related Docs

- [Resume Briefings](resume.md)
- [Project Tab](../tui/project.md)
- [Resume Tab](../tui/resume.md)
