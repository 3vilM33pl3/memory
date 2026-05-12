# `memory prune-history`

`memory prune-history` permanently removes old memory versions according to retention thresholds.

Use it only after previewing the effect. This command can delete tombstoned canonical rows and superseded non-latest versions.

## Common Usage

```bash
memory prune-history --project memory --dry-run --json
memory prune-history --project memory --tombstone-after 90d --superseded-after 180d --dry-run
memory prune-history --project memory --tombstone-after 90d --superseded-after 180d
```

## Options

- `--project <slug>` limits the sweep to one project. Without it, the command can inspect every project in the database.
- `--tombstone-after <duration>` overrides the configured tombstone retention threshold.
- `--superseded-after <duration>` overrides the configured superseded-version retention threshold.
- `--dry-run` previews counts without deleting anything.
- `--json` emits structured output for agents and scripts.

Durations use compact values such as `30d` or `12h`.

## Agent Guidance

Agents should always run `--dry-run --json` first and should prefer `--project` unless the user explicitly asked for a global database sweep.

## Related Docs

- [Memory Types Reference](../../developer/architecture/memory-types.md)
- [History Command](history.md)
- [Archive Command](archive.md)
