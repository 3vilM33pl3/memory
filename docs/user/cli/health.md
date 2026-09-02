# `memory health`

These two commands are lightweight operational checks.

For first-line diagnosis, prefer [`memory status`](status.md). It aggregates service reachability, config, watcher state, MCP status, and doctor checks. `health` remains useful for narrow service-health scripts; there is no separate `memory stats` command.

## `memory health`

```bash
memory health
```

Returns backend service health, database status, instance identity, and version information.


## Related Docs

- [Status Command](status.md)
- [Service Commands](service.md)
- [Doctor Diagnostics](doctor.md)
