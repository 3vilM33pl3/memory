# Repository Layer — Extend Across Crates

## Review Basis

The 2026-05-18 audit (`AUDIT-2026-05-18.md`) found that plan `07-repository-layer.md` landed cosmetically. `crates/mem-service/src/repository.rs` (1,181 LOC, 33 `sqlx::query` calls) extracted service-owned reads, but the scatter the plan was meant to fix is largely untouched outside mem-service:

- mem-curate: **35** direct `sqlx::query` calls
- mem-graph: **11** direct `sqlx::query` calls
- mem-search: **5+** direct `sqlx::query` calls

Writes were not extracted at all. A schema change still requires touching N callsites across three crates.

## Goal

Extend the repository pattern to every crate that holds SQL today. Each crate that owns a domain (curation, graph, search) gets its own `repository.rs` (or `db/` module tree if size warrants), with all `sqlx::query`/`sqlx::query_as` calls flowing through it.

## PR Shape

One PR per crate. Reads first (less risky), writes second. Within each crate, group queries by entity (project, memory, chunk, edge, ranking_signal, etc.).

1. **mem-search**: extract `crates/mem-search/src/repository.rs`. Smallest surface (5+ calls); good warm-up.
2. **mem-graph**: extract `crates/mem-graph/src/repository.rs`. Adds graph-entity reads/writes used by retrieval boosts.
3. **mem-curate**: extract `crates/mem-curate/src/repository.rs`. Largest surface (35 calls). Consider `crates/mem-curate/src/repository/{proposal,memory,project}.rs` if a single file exceeds ~1,500 LOC.
4. **mem-service writes**: extend the existing `crates/mem-service/src/repository.rs` to cover handler writes too; today it only covers reads.

## Implementation Notes

- Mirror the existing `crates/mem-service/src/repository.rs` style: domain methods (`get_project_by_slug`, `insert_chunk`, `latest_memory_version`) rather than thin SQL wrappers.
- Keep transaction boundaries explicit. Callers pass a `&mut sqlx::PgConnection` or `&sqlx::PgPool` to repository methods, just as today.
- Reuse `crates/mem-test-support` (from plan `03`) for repository-level integration tests. Each new repository module should ship a `tests/db_<crate>.rs` covering the moved queries.
- Do not introduce a trait abstraction for the repository in this pack. Concrete structs are enough; trait-based dependency inversion can come later if call sites prove they need it.
- Migrations are out of scope; this pack only relocates query code.

## Tests

- `cargo test -p mem-search -p mem-graph -p mem-curate -p mem-service --all-targets --locked` after each PR (DB env var required for the integration tests).
- `cargo clippy -p <crate> --all-targets --locked -- -D warnings` per moved crate.
- Spot-check: `grep -rn 'sqlx::query' crates/<crate>/src/` should show only repository module hits after the pack.

## Acceptance Criteria

- **SQL containment budget**: outside each crate's `repository*` module, `grep -rn 'sqlx::query' crates/<crate>/src/` returns **zero hits** for mem-curate, mem-graph, mem-search, mem-service.
- Schema-change PRs touch the matching repository module and (where needed) migrations — nothing else.
- Each crate has at least one DB integration test under `tests/` using `crates/mem-test-support`.
- No retrieval, ranking, or curation behavior changes.
