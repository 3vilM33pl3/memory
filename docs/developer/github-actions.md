# GitHub Actions

Memory Layer uses GitHub Actions for incremental validation, release publishing, dependency upkeep, and optional Codex-agent workflows.

## Required Repository Secrets

- `OPENAI_API_KEY`: Enables Codex review, resolver, discussion, and feature-agent workflows. The workflows log the Codex CLI in with `codex login --with-api-key` before running agent commands. If it is missing, PR review and discussion jobs post a skip message and the manually dispatched feature-agent job fails early.

## Optional Repository Variables

- `CODEX_REVIEW_MODEL`: Defaults to `gpt-5.4-mini`.
- `CODEX_RESOLVE_MODEL`: Defaults to `gpt-5.4`.
- `CODEX_DISCUSS_MODEL`: Defaults to `gpt-5.4-mini`.

## Continuous Integration

`.github/workflows/ci.yml` runs on pull requests and pushes to `main` or `release/**`.

The workflow starts with a path filter and only runs the jobs affected by the changed files:

- `Rust Format`: `cargo fmt --check`
- `Rust Tests`: `cargo test --workspace --all-targets --locked`
- `Rust Clippy`: `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `DB Integration`: runs pgvector-backed migration, graph, and curation smoke tests
- `Offline Eval Smoke`: dry-runs the bundled offline memory evaluation suite
- `Web Build`: installs and builds the TUI/web frontend
- `Debian Package Smoke`: builds amd64 and arm64 `.deb` packages and uploads them as artifacts

The DB integration job runs when a pull request touches `migrations/**`, `mem-graph`, `mem-curate`, `mem-search`, `mem-service`, `mem-api`, or the shared DB test harness. It starts a `pgvector/pgvector:pg16` service and sets:

```bash
MEMORY_LAYER_TEST_DATABASE_URL=postgres://memory:memory@localhost:5432/memory_test
MEMORY_LAYER_TEST_REQUIRE_DB=1
```

Local `cargo test --workspace --all-targets --locked` remains usable without PostgreSQL. DB tests return early when `MEMORY_LAYER_TEST_DATABASE_URL` is absent, unless `MEMORY_LAYER_TEST_REQUIRE_DB=1` is also set.

To run the same DB smoke tests locally, start any PostgreSQL instance with the `vector` extension available, create a test database, then run:

```bash
export MEMORY_LAYER_TEST_DATABASE_URL=postgres://memory:memory@localhost:5432/memory_test
export MEMORY_LAYER_TEST_REQUIRE_DB=1
cargo test -p mem-test-support -p mem-graph -p mem-curate --locked
```

## Release Publishing

`.github/workflows/release.yml` runs when a `v*` tag is pushed.

It validates that the tag version matches `Cargo.toml`, `Cargo.lock`,
`web/package.json`, `web/package-lock.json`, `docs-site/package.json`, and
`docs-site/package-lock.json`, then runs Rust validation once.
Native package jobs build and checksum the supported installer set:

- Debian amd64 `.deb`
- Debian arm64 `.deb`
- macOS Intel `.pkg`
- macOS Apple Silicon `.pkg`
- Windows x86_64 `.zip` and `.msi`
- `memory-<version>.tar.gz` source archive for Homebrew

Each package job uploads workflow artifacts. A single final publish job downloads
those artifacts, generates release notes, creates the GitHub Release, and
uploads every package plus its `.sha256` file. Homebrew formula updates happen
after the release archive exists, because the formula checksum must match the
published tarball.

## Agent PR Workflow

`.github/workflows/agent-pr.yml` has three jobs:

- `Codex Review Agent`: reviews non-draft pull requests and posts actionable findings as a PR comment.
- `Codex Resolver Agent`: for same-repository PR branches, attempts minimal fixes for review findings and obvious CI failures, then pushes to the PR branch.
- `Codex Discussion Agent`: responds to PR comments that start with `/agent-discuss`.

The resolver deliberately does not run for forked PR branches because it needs write access to push fixes.

## Feature-Agent Workflow

`.github/workflows/agent-task.yml` is manually dispatched from the Actions UI. It creates an `agent/<task-id>/<slug>` branch, runs Codex with write access in the checkout, commits the result, pushes the branch, and opens a draft PR.

Use it for parallel implementation work where a task can be isolated and reviewed independently. The prompt should include the desired outcome, constraints, and any issue ID that must appear in commits or PR text.

## Nightly Sweep

`.github/workflows/nightly.yml` runs once per day and can also be dispatched manually. It performs a broad validation sweep across Rust, web, offline evaluation, Debian amd64/arm64 packaging, and dependency audits. Dependency audit steps are allowed to fail so audit drift is visible without hiding build regressions.

## Dependabot

`.github/dependabot.yml` opens weekly update PRs for GitHub Actions, Cargo, and the web frontend npm dependencies.

## Branch Protection

`main` should require pull requests, conversation resolution, and the CI status checks from the incremental CI workflow. The agent resolver can update same-repository PR branches, but it does not replace human review for high-risk changes.
