# v1.0.0-rc.1 Validation Record - 2026-07-03

This record tracks the local validation pass for preparing the v1.0 release
candidate. It is not a final release certificate until the release branch and
packaged install/upgrade checks also pass.

## Environment

- Repository: local `memory` checkout (repo root)
- Baseline branch: `main`
- Target version: `1.0.0-rc.1`
- Initial state: `main` was ahead of `origin/main` by 2 commits and had an
  unrelated untracked `trace` file.

## Results

| Check | Result | Notes |
|---|---|---|
| Plan checkpoint | Partial | Checkpoint saved, but plan curation timed out on `/v1/curate`. |
| Plan finish workflow | Failed | `checkpoint finish-execution` timed out while curating the updated plan before finish verification. |
| `cargo fmt --check` | Pass | Completed locally. |
| `npm --prefix web test` | Pass | 17 files, 61 tests. |
| `npm --prefix web run build` | Pass | Existing large `GraphTab` chunk warning. |
| `npm --prefix docs-site run build` | Pass | Static docs generation completed. |
| `cargo test --workspace --all-targets --locked` | Pass | Full workspace all-target test suite completed after updating a `mem-watch` test fixture for `OfflineConfig`. |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Pass | Fixed a test helper argument lint in `mem-graph` and a needless borrow in `mem-search`. |
| `cargo test -p mem-search --all-targets --locked` | Pass | Focused rerun after the `mem-search` clippy fix. |
| `cargo test -p mem-graph --all-targets --locked` | Pass | Focused rerun after the `mem-graph` test helper cleanup. |
| `npm --prefix docs-site run lint:links` | Pass | Checked docs-site links. |
| `npm --prefix docs-site run check:assets` | Pass | Checked 14 image references. |
| pgvector-backed release DB smoke | Not run | Workspace DB tests passed through the normal test harness, but no external `MEMORY_LAYER_TEST_DATABASE_URL` release database was configured. |
| Eval release gate | Not run | Requires a fresh comparison artifact at `target/memory-evals/comparison.json`. |
| Packaged install/upgrade smoke | Not run | Requires built RC package/Homebrew formula flow. |

## Blockers before final v1.0.0

- Fix or diagnose `/v1/curate` timeout so plan-memory start and finish
  workflows can curate and close normally.
- Push current `main`, create `release/v1.0.0-rc.1`, and run GitHub CI.
- Run release database smoke against PostgreSQL with pgvector.
- Run eval gate from a fresh comparison artifact.
- Install and upgrade the RC package on Debian and Homebrew before final tag.
