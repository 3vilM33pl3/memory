# `memory graph`

`memory graph` extracts parser-backed code structure into graph tables. It is additive: Memory Layer still works without graph data, but query and TUI search can use the latest completed graph extraction to improve retrieval explanations and ranking.

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
- `--rebuild-index` refreshes the user-local project repository index before extraction.
- `--force` creates a fresh extraction run even when an identical completed run already exists.
- `--since` is passed through to the repository index context.

## Status

```bash
memory graph status --project memory
memory graph status --project memory --text
```

Shows the latest completed extraction run, analyzer version, strategy version, symbol counts, reference counts, graph node/edge counts, evidence counts, and unresolved/ambiguous reference counts.

## Use In Query

After a completed extraction exists, `memory query` and the TUI `Query` tab automatically use it. The query pipeline looks for code symbols, references, and one-hop graph neighbors that match the question, then maps those graph hits back to memories through `memory_sources.file_path` provenance.

Graph matches are used as ranking evidence:

- direct symbol and reference matches receive the strongest boost
- one-hop neighbor matches receive a smaller boost
- total graph boost per memory is capped
- graph diagnostics report whether graph retrieval was `active`, `no_graph`, `no_terms`, or `error`

Graph hits do not bypass memory curation. Answers are still generated from returned memories, and graph connections are shown as explainability metadata so a human can see why a memory was retrieved.

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
- [Query Command](query.md)
- [Graph And Curation Foundations](../../developer/architecture/graph-and-curation-foundations.md)
