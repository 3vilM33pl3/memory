# `memory automation`

`memory automation` inspects or flushes watcher-driven automation state for a project.

## Subcommands

### Status

```bash
memory automation status --project memory
```

Shows the current automation state for the project.

### Flush

```bash
memory automation flush --project memory
memory automation flush --project memory --curate
memory automation flush --project memory --curate --dry-run
```

Flushes pending automation work into capture state and optionally runs curation afterward.

## Related Docs

- [Watcher Health](watchers.md)
- [Remember Command](remember.md)
