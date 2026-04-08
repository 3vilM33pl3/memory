# `memory repo`

`memory repo` manages the local repository index used by `memory scan` and related analysis flows.

## Subcommands

### Index

```bash
memory repo index --project memory
memory repo index --project memory --since 2026-04-01
memory repo index --project memory --dry-run
```

Builds or refreshes the local index.

### Status

```bash
memory repo status --project memory
memory repo status --project memory --json
```

Shows analyzer coverage, fact counts, and local index status.

## Related Docs

- [Scan Command](scan.md)
