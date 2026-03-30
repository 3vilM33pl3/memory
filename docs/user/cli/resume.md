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

If LLM configuration is available, Memory Layer synthesizes a concise briefing from that structured resume pack. If not, the deterministic resume pack is still returned.

## TUI

The TUI has a dedicated `Resume` tab.

If a checkpoint exists and the project changed since that checkpoint, the TUI opens on `Resume` first so you can get back into flow immediately.
