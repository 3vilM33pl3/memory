# `memory checkpoint`

`memory checkpoint` manages saved project checkpoints, approved-plan execution, and direct no-plan task starts.

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

### Start Task

```bash
memory checkpoint start-task \
  --project memory \
  --title "Fix query input" \
  --prompt "The query input field should activate with Enter and keep query history."

memory checkpoint start-task \
  --project memory \
  --title "Update README" \
  --prompt "Highlight the benchmark report" \
  --dry-run \
  --json
```

Saves the checkpoint and stores the direct user instruction as a `task` memory when execution starts without an approved plan.

Use this for actionable implementation work that begins directly from a user request. Do not use it for pure questions, planning-only turns, trivial read-only checks, or work that already has an approved plan.

The command curates the raw capture it just wrote and verifies that an active `task` memory with the expected `task-thread:<thread_key>` tag exists before reporting success. If that postcondition fails, the command exits with an actionable error instead of silently leaving only raw capture evidence.

After the direct task is complete, use `memory remember --type implementation`, `memory remember --type refactor`, or the repo-local remember skill to record what was actually delivered.

### Finish Execution

```bash
memory checkpoint finish-execution --project memory
memory checkpoint finish-execution --project memory --plan-file /tmp/plan.md --json
memory checkpoint finish-execution --project memory --implementation-summary "Implemented the watcher manager footer refresh"
memory checkpoint finish-execution --project memory --dry-run
```

Verifies that every checkbox item in the active approved plan is complete before the work is presented as finished.

When verification succeeds, `finish-execution` also records a durable `implementation` memory for the completed outcome, or a `refactor` memory when the completed plan is clearly behavior-preserving code restructuring.

Use `--implementation-summary` and repeatable `--implementation-note` flags when you want the recorded implementation or refactor memory to say something more explicit than the default summary derived from the completed checkbox items.

## Related Docs

- [Resume Briefings](resume.md)
- [Project Tab](../tui/project.md)
- [Resume Tab](../tui/resume.md)
- [Memory Types Reference](../../developer/architecture/memory-types.md)
