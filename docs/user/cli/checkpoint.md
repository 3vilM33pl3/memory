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
memory checkpoint finish-execution --project memory --implementation-summary "Implemented the watcher manager footer refresh"
memory checkpoint finish-execution --project memory --dry-run
```

Verifies that every checkbox item in the active approved plan is complete before the work is presented as finished.

When verification succeeds, `finish-execution` also records a durable `implementation` memory for the completed outcome.

Use `--implementation-summary` and repeatable `--implementation-note` flags when you want the recorded implementation memory to say something more explicit than the default summary derived from the completed checkbox items.

## Related Docs

- [Resume Briefings](resume.md)
- [Project Tab](../tui/project.md)
- [Resume Tab](../tui/resume.md)
- [Memory Types Reference](../../developer/architecture/memory-types.md)
