# DB Integration Test Harness

## Review Basis

Claude flagged that database behavior is weakly covered: `mem-graph` and `mem-curate` have thin tests, migrations are not exercised in CI, and DB tests are skipped unless local environment variables are present.

## Goal

Make database-backed behavior testable and visible in pull requests, especially migrations, curation writes, graph extraction, and graph-assisted retrieval.

## PR Shape

First PR creates the harness and one smoke test. Follow-up PRs add focused `mem-graph` and `mem-curate` scenarios.

## Implementation Notes

- Use a CI Postgres service with pgvector enabled, or a documented container image that already includes pgvector.
- Standardize the test URL as `MEMORY_LAYER_TEST_DATABASE_URL`.
- Add a small helper for creating isolated projects and cleaning rows between tests.
- Add a migration smoke test that runs all migrations against a blank DB.
- Add `crates/mem-graph/tests/db.rs` for extract/store/query flow.
- Add `crates/mem-curate/tests/db.rs` for raw capture to canonical memory flow, including replacement proposal behavior.

## Tests

- CI job must fail if migrations do not apply.
- DB integration tests must run in CI, not silently skip.
- Local fallback should still allow `cargo test --workspace --all-targets --locked` without a local DB by keeping DB tests gated to the integration job.

## Acceptance Criteria

- Pull requests touching `migrations/**`, `mem-graph`, `mem-curate`, or DB-heavy service code run at least one pgvector-backed test job.
- Test setup is documented in contributor docs.
- No production code path is changed except where needed for testability.
