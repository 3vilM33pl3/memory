# Refactor Baseline

This document records the stabilization direction for Memory Layer before larger
knowledge graph, code structure graph, specialized curation, and code-analysis
work.

## Baseline Gate

Every baseline refactor should keep these checks green:

- `cargo fmt --check`
- `cargo test --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `npm --prefix web ci`
- `npm --prefix web run build`

If a check cannot be green, the commit or issue must explain the blocker and the
follow-up required to restore it.

## Stabilization Priorities

1. Keep release metadata, package versions, and licensing consistent.
2. Keep memory versioning semantics centralized and applied across reads,
   imports, exports, stats, search, and TUI views.
3. Split large runtime modules only after the behavior they currently own is
   covered by tests.
4. Make the agent-linked watcher manager the primary automation model and keep
   legacy per-project watchers as compatibility mode.
5. Add graph and curation foundations incrementally, with schemas, provenance,
   evaluation fixtures, and migrations before replacing existing behavior.

## Future Architecture Direction

The existing `memory_relations` table is an active-memory relation graph, not a
complete knowledge graph. Future graph work should add first-class concepts for:

- entities and claims
- code symbols and references
- typed edges with confidence and provenance
- version-aware graph updates
- evaluation fixtures for extraction, relation detection, replacement, and
  retrieval quality

The first graph-facing work should be additive. Existing memory APIs and CLI
commands should remain compatible until the replacement path has coverage and a
documented migration.

The detailed graph and curation direction lives in
[Graph And Curation Foundations](architecture/graph-and-curation-foundations.md).
