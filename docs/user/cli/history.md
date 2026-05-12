# `memory history`

`memory history` shows every version in one canonical memory chain, including tombstones.

Use it when a memory looks wrong, replaced, deleted, or superseded and you need to inspect how it changed over time.

## Common Usage

```bash
memory history <memory-id>
memory history <memory-id> --json
```

`<memory-id>` can be any version in the chain. Memory Layer resolves the canonical id and returns the same history for current, superseded, or tombstone versions.

## What It Shows

- each memory version id
- canonical id and version number
- tombstone status
- summary, type, status, timestamps, and provenance where available

Use `--json` when an agent or script needs to compare versions programmatically.

## Related Docs

- [Memories Tab](../tui/memories.md)
- [Memory Types Reference](../../developer/architecture/memory-types.md)
- [Prune History](prune-history.md)
