# v1.0 Release Readiness

Use this checklist for the `v1.0.0-rc.1` and final `v1.0.0` releases. The goal
is a stability release, not another feature train.

## Release contract

The v1 compatibility promise covers:

- stable documented CLI commands and JSON output for core workflows
- stable global and project config file ownership
- append-only database migrations from the latest `0.9.x` release
- read-only MCP tools and resource/prompt surfaces
- packaged service behavior for Debian, Homebrew, and source/dev modes

Experimental or advanced surfaces must be documented as such before v1.0:
loop automation, code graph visualization, browser demo data, and research eval
extensions.

## RC checklist

Before tagging `v1.0.0-rc.1`:

1. Start from a clean pushed `main`, then create `release/v1.0.0-rc.1`.
2. Bump Cargo, web, and docs-site metadata to `1.0.0-rc.1`.
3. Confirm `Cargo.lock`, `web/package-lock.json`, and
   `docs-site/package-lock.json` match their package manifests.
4. Run:

   ```bash
   cargo fmt --check
   cargo test --workspace --all-targets --locked
   cargo clippy --workspace --all-targets --locked -- -D warnings
   npm --prefix web test
   npm --prefix web run build
   npm --prefix docs-site run build
   ```

5. Run pgvector-backed database tests with:

   ```bash
   export MEMORY_LAYER_TEST_DATABASE_URL=postgres://memory:memory@localhost:5432/memory_test
   export MEMORY_LAYER_TEST_REQUIRE_DB=1
   cargo test -p mem-test-support -p mem-graph -p mem-curate -p mem-search -p mem-service --locked
   ```

6. Run eval release checks:

   ```bash
   memory eval doctor --suite evals/suites/research-v1 --text
   memory eval gate --comparison target/memory-evals/comparison.json --policy evals/gates/research-v1.toml --text
   ```

7. Smoke test fresh install and upgrade paths for Debian, Homebrew, and source
   dev mode.
8. Verify core workflows manually: service, status, doctor, TUI, web UI,
   watcher manager, stdio MCP, HTTP MCP, `memory_query`, and
   `memory_search_all`.

## Known release blockers

- `/v1/curate` timeout can block plan-memory closure in the current dev setup.
- Multiple active plan memories can confuse plan completion unless a thread key
  is passed.
- The final release must not be tagged until the release branch passes GitHub
  Actions and one packaged install/upgrade cycle.

## Final promotion

Promote `v1.0.0-rc.1` to `v1.0.0` only after the RC has been installed and used
locally, all release blockers are closed or explicitly deferred in release
notes, and the eval gate report is archived under `docs/developer/evaluation-runs/`.
