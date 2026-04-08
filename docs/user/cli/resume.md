# `memory resume`

Use `resume` when you return to a project after an interruption and want a fast briefing instead of manually reading activity, commits, and memory state.

## Commands

Save a checkpoint before leaving:

```bash
memory checkpoint save --project my-project --note "Waiting on agent review"
```

Preview the checkpoint payload and storage path without saving it:

```bash
memory checkpoint save --project my-project --note "Waiting on agent review" --dry-run
```

For agent-driven workflows, a good checkpoint moment is right after a planning session ends and execution is approved:

```bash
memory checkpoint start-execution \
  --project my-project \
  --plan-file /tmp/approved-plan.md
```

Preview the checkpoint, plan capture, and curate actions without persisting them:

```bash
memory checkpoint start-execution \
  --project my-project \
  --plan-file /tmp/approved-plan.md \
  --dry-run
```

That command saves the checkpoint first and then stores the full approved plan as `plan` memory for the active work thread.

Use Markdown checkbox items in the approved plan:

```md
- [ ] implement the API
- [ ] update the skill
- [ ] verify the tests
```

Before plan-backed work is treated as finished, verify the active approved plan:

```bash
memory checkpoint finish-execution --project my-project
```

Check whether the active plan would verify cleanly without syncing or logging anything:

```bash
memory checkpoint finish-execution --project my-project --dry-run
```

If the plan changed during execution, sync the updated checkbox state and verify in one step:

```bash
memory checkpoint finish-execution \
  --project my-project \
  --plan-file /tmp/approved-plan.md
```

Show the current checkpoint:

```bash
memory checkpoint show --project my-project
```

Generate a resume briefing:

```bash
memory resume --project my-project
```

JSON output for tools and agents:

```bash
memory resume --project my-project --json
```

## What it uses

The resume briefing combines:

- your last explicit checkpoint
- recent project timeline events
- stored commit history
- changed memories since the checkpoint
- current project warnings and health
- durable high-signal context that still matters

## What you get back

The resume output is task-oriented. It is meant to answer "how do I get back into flow?" rather than dump raw counts.

The main sections are:

- `Current thread`
  - the most likely active work thread since your checkpoint
- `Next step`
  - the one action Memory Layer thinks you should take first
- `What changed`
  - the most important recent project activity, commits, or memory changes
- `Needs attention`
  - actionable blockers or review items
- `Keep in mind`
  - durable context that is still relevant to the current thread

The CLI and TUI also keep the supporting timeline, warnings, and raw project state available below the briefing.

If LLM configuration is available, Memory Layer synthesizes a concise briefing from that structured resume pack. If not, the deterministic resume pack is still returned.

## TUI

The TUI has a dedicated `Resume` tab.

If a checkpoint exists and the project changed since that checkpoint, the TUI opens on `Resume` first so you can get back into flow immediately.

The `Resume` tab renders the task-oriented sections directly, then shows recent timeline items underneath for deeper inspection.
