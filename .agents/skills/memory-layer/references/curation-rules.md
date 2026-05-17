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
- `documentation`
- `task`
- `plan`
- `implementation`
- `refactor`
- `user`
- `feedback`
- `project`
- `reference`

Use the full reference here:

- `docs/developer/architecture/memory-types.md`

Important current distinctions:

- `plan` is the approved execution plan captured at start-execution time.
- `implementation` is the verified delivered outcome, including finish-execution results and normal completed-work remember flows.
- `refactor` is behavior-preserving code restructuring. Use it for moves, renames, extracted helpers, reorganized modules, cleanup, and mechanical reshaping when there is no intended functional change.
- `documentation` is durable docs work or documentation-system knowledge. Do not use it merely because the source file lives under `docs/`; use `environment`, `convention`, `architecture`, or `domain_fact` when the remembered fact belongs there.
- `debugging` is the durable troubleshooting lesson, not the same thing as the implemented outcome.
