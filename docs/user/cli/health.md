# `memory health` and `memory stats`

These two commands are lightweight operational checks.

For first-line diagnosis, prefer [`memory status`](status.md). It aggregates service reachability, config, watcher state, MCP status, and doctor checks. `health` and `stats` remain compatibility commands for narrow scripts.

## `memory health`

```bash
memory health
```

Returns backend service health, database status, instance identity, and version information.

## `memory stats`

```bash
memory stats
```

Shows a compact project and service summary, useful for quick inspection or scripting.

## Related Docs

- [Status Command](status.md)
- [Stats Command](stats.md)
- [Service Commands](service.md)
- [Doctor Diagnostics](doctor.md)
