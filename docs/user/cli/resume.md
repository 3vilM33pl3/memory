# `mem-cli resume`

Use `resume` when you return to a project after an interruption and want a fast briefing instead of manually reading activity, commits, and memory state.

## Commands

Save a checkpoint before leaving:

```bash
mem-cli checkpoint save --project my-project --note "Waiting on agent review"
```

For agent-driven workflows, a good checkpoint moment is right after a planning session ends and execution is approved:

```bash
mem-cli checkpoint save --project my-project --note "Plan approved; starting implementation"
```

Show the current checkpoint:

```bash
mem-cli checkpoint show --project my-project
```

Generate a resume briefing:

```bash
mem-cli resume --project my-project
```

JSON output for tools and agents:

```bash
mem-cli resume --project my-project --json
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
