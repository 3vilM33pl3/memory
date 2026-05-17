# `memory verify-provenance`

`memory verify-provenance` checks whether memory source citations still point at files in the repository.

Use it after refactors, file moves, deletes, or cleanup work. It does not mutate memory text; it records verification metadata for source rows unless `--dry-run` is passed.

## Common Usage

```bash
memory verify-provenance --project memory --dry-run --json
memory verify-provenance --project memory --repo-root . --json
memory verify-provenance --project memory
```

## Useful Flags

- `--project` selects the project slug. If omitted, the CLI tries the current repo project marker.
- `--repo-root` overrides the repository root used to resolve relative source paths.
- `--dry-run` checks sources without storing verification results.
- `--json` returns the full response for automation.

## Output

The text output shows checked, verified, missing-file, missing-symbol, unverifiable, stale, and stored counts. It also lists warnings and up to 25 non-verified sources.

The JSON output includes each source id, memory id, memory summary, source kind, file path, resolved path, status, and reason.

## How Results Are Used

Stored verification results are surfaced in memory detail responses, the TUI memory detail view, and query source output. Query diagnostics include provenance warnings when a result cites a source previously marked `missing_file`, `missing_symbol`, or `stale`.

`unverifiable` means the source row does not point at a concrete file path, such as external URLs or note-only provenance. It is not treated as stale by query diagnostics.

## Related Docs

- [Query Command](query.md)
- [TUI Memories Tab](../tui/memories.md)
- [Code Graph Extraction](graph.md)
- [Memory Types Reference](../../developer/architecture/memory-types.md)
