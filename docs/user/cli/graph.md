# `memory graph`

`memory graph` extracts parser-backed code structure into graph tables. It is additive: current memory query and TUI behavior do not depend on it unless future graph retrieval is explicitly enabled.

## Extract

```bash
memory graph extract --project memory
memory graph extract --project memory --dry-run
memory graph extract --project memory --rebuild-index
memory graph extract --project memory --force --text
```

The command reuses the local repository index when possible, resolves symbols and references conservatively, then stores code symbols, code references, graph nodes, graph edges, and evidence.

JSON is the default output. Use `--text` for a human-readable summary.

Important flags:

- `--dry-run` previews counts and sample unresolved references without writing database rows or index files.
- `--rebuild-index` refreshes `.mem/runtime/index/*-repo-index.json` before extraction.
- `--force` creates a fresh extraction run even when an identical completed run already exists.
- `--since` is passed through to the repository index context.

## Status

```bash
memory graph status --project memory
memory graph status --project memory --text
```

Shows the latest completed extraction run, analyzer version, strategy version, symbol counts, reference counts, graph node/edge counts, evidence counts, and unresolved/ambiguous reference counts.

## Resolution Semantics

The resolver only creates graph edges when both source and target symbols are resolved. Unresolved and ambiguous references are still stored in `code_references` so diagnostics can improve over time without inventing graph edges.

Resolution statuses:

- `resolved` means the reference matched one symbol identity.
- `unresolved` means no matching symbol was found.
- `ambiguous` means multiple plausible symbols matched.

## Stored Data

The graph baseline writes:

- `graph_extraction_runs` for immutable extraction metadata.
- `code_symbols` for parser-backed symbols.
- `code_references` for imports, calls, references, and test links.
- `graph_nodes` for code symbol nodes.
- `graph_edges` for resolved relationships.
- `graph_evidence` for source file/span provenance.

Every graph node and edge is tied to an extraction run and file-span evidence.

## Related Docs

- [Repository Index](repo.md)
- [Scan Command](scan.md)
- [Graph And Curation Foundations](../../developer/architecture/graph-and-curation-foundations.md)
