# Memory Repo Instructions

## Version control
- Keep commits small and incremental.
- Always commit at the end of a task.
- Use expressive commit prefixes when they fit:
  - `Feat:` for user-visible features or capabilities
  - `Fix:` for bugs, regressions, and broken behavior
  - `Docs:` for documentation-only changes
  - `Build:` for packaging, release, or dependency/build-system changes
  - `Refactor:` for internal code reshaping without intended behavior change
  - `Test:` for test-only changes
  - `Chore:` for maintenance work that does not fit the categories above
- Prefer the most specific prefix instead of defaulting to `Chore:`.

## Memory Layer workflows

This project uses Memory Layer to persist durable project knowledge. The `memory` CLI
must be on PATH (or use `cargo run --bin memory --` from the repo root).

### Shared invariants
1. Query memory before answering project-specific questions.
2. Use `resume` instead of a generic query for interruption-recovery prompts.
3. Save the approved plan before implementation begins when a planning phase turns into execution.
4. Verify plan-backed work is complete before claiming the task is finished.
5. Remember meaningful work after it is actually done.
6. Prefer insufficient evidence over unsupported conclusions.
7. Never invent provenance.

### Query and resume
Use when: the user asks a project-specific question or returns after an interruption.

```bash
memory query --project memory --question "<question>"
memory resume --project memory
```

### Plan execution
Use when: a planning session ends and the user approves execution.

Save checkpoint and plan at execution start:
```bash
memory checkpoint start-execution --project memory --plan-file /tmp/approved-plan.md
```

Verify all plan items are complete before claiming finished:
```bash
memory checkpoint finish-execution --project memory
```

### Remember completed work (mandatory post-task rule)
**After any meaningful repository work, run the remember workflow before sending the
final response** unless one of these is true:
- no durable knowledge was produced
- the work was purely trivial
- the user explicitly asked not to store memory

```bash
memory remember --project memory \
  --title "<task title>" \
  --summary "<what changed>" \
  --note "<durable fact 1>" \
  --note "<durable fact 2>" \
  --file-changed "<path>"
```

This should default to storing durable project knowledge, not waiting for the user to ask.
