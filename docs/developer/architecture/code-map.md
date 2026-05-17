# Code Map

Use this map to find the right PR boundary before opening the large files.

## Crates

- `crates/mem-api`: shared request/response types, config loading, validation helpers, and public API contracts.
- `crates/mem-cli`: the `memory` binary, CLI command routing, TUI, setup wizard, eval runner orchestration, and local service tooling.
- `crates/mem-service`: HTTP service runtime, routes, repository reads, migrations entrypoint, activity persistence, and service-mounted MCP.
- `crates/mem-search`: query retrieval, lexical/semantic/graph merging, answer synthesis, citations, and query diagnostics.
- `crates/mem-curate`: raw capture curation, memory replacement proposals, curation policies, and memory write shaping.
- `crates/mem-graph`: repository index graph extraction, symbol/reference storage, and graph status.
- `crates/mem-watch`: watcher daemon and automation capture logic.
- `crates/mem-mcp`: read-only MCP protocol adapter over the service HTTP API.
- `crates/mem-eval`: eval suite schemas, scoring, comparisons, gates, and report rendering.
- `crates/mem-test-support`: pgvector-backed integration test harness.

## Frontends

- `crates/mem-cli/src/tui.rs`: TUI orchestration and tab rendering. Pure helpers are being split under `crates/mem-cli/src/tui/`.
- `web/src/App.tsx`: browser UI shell, project selection, polling, and feature composition.
- `web/src/features/`: extracted browser feature tabs.
- `web/src/api.ts`: browser API boundary. Keep fetch details here instead of scattering endpoint strings.

## Common Change Paths

- Add a CLI command: update `crates/mem-cli/src/main.rs` or `crates/mem-cli/src/commands/`, add help metadata, add a `docs/user/cli/*.md` page, and extend command-help tests.
- Add a service endpoint: add types in `mem-api`, route/service logic in `mem-service`, tests in the touched crate, and client wiring where needed.
- Change query ranking: update `mem-search`, preserve diagnostics, and add tests for result ordering or explanation fields.
- Change memory curation: update `mem-curate`, replacement proposal behavior, memory type docs, and database tests when persistence changes.
- Change the TUI: keep tab behavior localized; extract pure rendering helpers before moving state.
- Change the web UI: extract feature components under `web/src/features/<feature>/`, add a component smoke test, then build.
- Change eval behavior: update `mem-eval` for schema/scoring and `mem-cli` for execution. Use `--allow-shell` for real shell suites and keep production query code separate from research adapters.
- Add a migration: create a new numbered file in `migrations/`; never edit a migration that may already have been applied.

## Validation

Start with the narrowest useful command:

```bash
cargo test -p mem-cli --all-targets --locked
cargo test -p mem-service --all-targets --locked
cargo test -p mem-eval --all-targets --locked
npm --prefix web run test
npm --prefix web run build
```

Before release or broad refactors:

```bash
cargo fmt --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

For DB-sensitive work, also run the pgvector-backed tests described in
[CONTRIBUTING.md](../../../CONTRIBUTING.md#database-tests).
