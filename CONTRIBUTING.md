# Contributing

Thanks for helping improve Memory Layer.

## License

By submitting a contribution, you agree that your contribution is licensed under
the same open source license as this repository, the GNU Affero General Public
License v3.0 or later.

You also agree that the project maintainer may offer the overall project under
separate commercial terms.

In plain English:

- your pull request stays open source under AGPL
- the maintainer keeps the right to dual-license the project commercially
- you confirm you have the right to submit the code you contribute

If you are contributing on behalf of an employer, make sure you have permission
to do so.

## Before Coding

Start with the code map: [docs/developer/architecture/code-map.md](docs/developer/architecture/code-map.md).

Pick the smallest PR shape that proves the change:

- CLI command or output: `crates/mem-cli`, focused command tests, and the matching page under `docs/user/cli/`
- service route or API behavior: `crates/mem-service`, `crates/mem-api`, route tests, and API/client docs
- search or retrieval behavior: `crates/mem-search`, query diagnostics tests, and query docs
- curation behavior: `crates/mem-curate`, replacement-proposal tests, and memory type docs
- TUI tab: `crates/mem-cli/src/tui.rs` or the `crates/mem-cli/src/tui/` helpers, with TUI tests
- browser tab: `web/src/features/<feature>/`, a component smoke test, and `npm --prefix web run build`
- eval suite or scoring: `crates/mem-eval`, `memory eval doctor`, and `memory eval run --dry-run`
- migrations or DB behavior: a migration, repository/service tests, and pgvector-backed integration coverage when behavior depends on Postgres

The refactor roadmap lives in [docs/reviews/refactor/README.md](docs/reviews/refactor/README.md).
Use it as guidance for keeping future PRs reviewable.

## Local Setup

Install the normal Rust and Node toolchains, then check the repository:

```bash
cargo fmt --check
cargo test --workspace --all-targets --locked
npm --prefix web ci
npm --prefix web run test
npm --prefix web run build
```

For a smaller Rust loop, test only the affected crates:

```bash
cargo test -p mem-cli --all-targets --locked
cargo test -p mem-service --all-targets --locked
cargo test -p mem-eval --all-targets --locked
```

## Database Tests

Most unit tests do not require a database. Pgvector-backed integration tests run
when `MEMORY_LAYER_TEST_DATABASE_URL` is set.

Use a dedicated disposable database with the `vector` extension installed:

```bash
export MEMORY_LAYER_TEST_DATABASE_URL='postgresql://memory:memory@127.0.0.1:5432/memory_test'
export MEMORY_LAYER_TEST_REQUIRE_DB=1
cargo test -p mem-test-support -p mem-graph -p mem-curate --locked
```

If your change touches migrations, SQL, graph persistence, curation persistence,
or service repository queries, include the database test result in the PR.

## Eval Safety

Eval suites are code execution inputs. Suites with `command_task`,
`agent_build_task`, or `agent_build_sequence` execute shell commands and require
explicit consent:

```bash
memory eval run --suite evals/examples/app-build-smoke --profile offline --allow-shell
```

Use `--dry-run` first when reviewing a new suite:

```bash
memory eval run --suite evals/examples/memory-smoke --profile offline --dry-run
```

## PR Expectations

- Keep one behavioral change per PR.
- Keep mechanical moves separate from behavior changes.
- Include a short validation section in the PR description.
- Update docs when command names, user workflows, config, or output contracts change.
- Add or adjust tests in proportion to risk.
- Do not hide unrelated cleanup in a feature PR.
- Call out migrations, install changes, service restarts, token/auth changes, and shell-executing evals explicitly.

## Developer Certificate Of Origin

By submitting a contribution, you certify that:

1. The contribution was created in whole or in part by you and you have the right
   to submit it under the open source license indicated in this file.
2. The contribution is based on previous work that, to the best of your
   knowledge, is covered under an appropriate open source license and you have
   the right under that license to submit it.
3. You understand that this project is public and that contributions may be
   redistributed under the project licenses.

## No Warranty

All contributions are provided as-is, without warranty of any kind.
