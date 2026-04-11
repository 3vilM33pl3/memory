# Curation Rules

- No canonical memory without provenance.
- Prefer durable, verified statements.
- Deduplicate by normalized canonical text before inserting a new memory entry.
- Capture raw task payloads first, then curate into canonical memory.

## Current Memory Types

The live application currently supports:

- `architecture`
- `convention`
- `decision`
- `incident`
- `debugging`
- `environment`
- `domain_fact`
- `plan`
- `implementation`

Use the full reference here:

- `docs/developer/architecture/memory-types.md`

Important current distinctions:

- `plan` is the approved execution plan captured at start-execution time.
- `implementation` is the verified delivered outcome, including finish-execution results and normal completed-work remember flows.
- `debugging` is the durable troubleshooting lesson, not the same thing as the implemented outcome.
