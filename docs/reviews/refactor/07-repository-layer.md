# Repository Layer

## Review Basis

Claude found many scattered `sqlx::query` callsites across curation, graph, search, and service code. Current `main` still has broad SQL spread, which makes schema changes hard to review.

## Goal

Introduce a small, domain-oriented data-access layer that centralizes common project, memory, source, activity, and relation queries.

## PR Shape

Start narrow. Extract duplicated project and memory lookup queries first; do not attempt to abstract every SQL statement in one PR.

## Implementation Notes

- Create a repository module or crate only when there is a clear owner. Recommended first step: an internal `mem-service::repository` module for service-owned reads.
- Use typed result structs for common rows instead of passing raw `sqlx::Row` through handlers.
- Keep transactional write flows in their existing crates until the read layer is stable.
- Avoid hiding important SQL behind generic CRUD helpers; prefer explicit domain methods such as `find_project_id`, `list_project_memories`, and `load_memory_detail`.
- Coordinate with the DB integration test harness before broad extraction.

## Tests

- Add repository unit tests for query construction where possible.
- Add DB integration tests for at least one extracted read flow.
- Run affected crate tests plus `cargo test --workspace --all-targets --locked` before merging broad extraction PRs.

## Acceptance Criteria

- Common project/memory lookup SQL is no longer duplicated in multiple handlers.
- Schema-change PRs have fewer scattered callsites.
- The abstraction improves readability without making SQL behavior opaque.
