# `memory ingest`

Turn any directory of documents into durable, queryable memory — no git repository required.

```bash
memory ingest ~/notes --project notes --dry-run
memory ingest ./papers --project research --type documentation --tag corpus-2026
```

Each text document becomes one memory candidate: the first heading (or file name) as the summary, a capped excerpt as the canonical text, and the file itself as verifiable provenance. Candidates are captured in batches and curated like any other memory — deterministic, keyless, and gated by the same curation pipeline.

Built for the non-repository cases: research-paper corpora, note vaults (Obsidian and friends), documentation trees, world-building notes for a game.

## Options

| Flag | Meaning |
|---|---|
| `--project <slug>` | Project to ingest into (created on first write). |
| `--type <memory-type>` | Memory type for the documents (default `reference`). |
| `--tag <tag>` | Attach a tag to every ingested memory (repeatable) — useful for filtering or bulk-archiving the corpus later. |
| `--ext <extension>` | Extensions to include (repeatable). Default: `md`, `markdown`, `txt`, `rst`, `org`, `adoc`. |
| `--max-files <n>` | Cap per run (default 200). |
| `--dry-run` | List what would be ingested without writing. |

## Limits and behavior

- Files over 256 KB and non-UTF-8 files are skipped (a memory is a distillation surface, not a blob store); hidden directories, `node_modules`, and `target` are never walked.
- Re-ingesting unchanged documents re-observes the same facts (raising confidence) rather than duplicating them — the exact-re-observation rule in curation.
- Repo-centric features (`scan`, code graph, commit sync) still require git; `query`, `resume`, `tui`, and `bundle` all work on ingested projects.

## Related Docs

- [Capture Command](capture.md)
- [Curate Command](curate.md)
- [Query Command](query.md)
- [Bundles](bundles.md)
