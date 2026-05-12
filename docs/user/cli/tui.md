# `memory tui`

`memory tui` opens the terminal UI for browsing, querying, operating, and diagnosing Memory Layer.

Use it for human inspection. Agents and scripts should prefer CLI commands with `--json` when they need parseable output.

## Common Usage

```bash
memory tui
memory tui --project memory
```

`--project <slug>` opens a specific project first. Without it, Memory Layer tries to infer the current repository project.

## What You Can Do

- browse and filter memories
- ask cited questions and inspect returned memories
- review activities, errors, watchers, agents, and embeddings
- switch embedding backends
- inspect project health and get resume briefings

## Related Docs

- [TUI Guide](../tui/README.md)
- [Memories Tab](../tui/memories.md)
- [Query Tab](../tui/query.md)
- [Errors Tab](../tui/errors.md)
