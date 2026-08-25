# GitHub Actions

Memory Layer uses GitHub Actions for incremental validation, release publishing, dependency upkeep, and optional Codex-agent workflows.

All repository workflows run on project-owned self-hosted runners. The primary
Linux x86_64 runner is `Monolith_Memory`, registered with the labels
`self-hosted`, `memory`, `Linux`, and `X64`. Workflow jobs must include the
`memory` label so they cannot be assigned to unrelated host runners.

## Self-hosted runner operation

On Monolith, the runner is installed in `~/actions-runner-memory` and managed
by:

```bash
systemctl status actions.runner.3vilM33pl3-memory.Monolith_Memory.service
journalctl -u actions.runner.3vilM33pl3-memory.Monolith_Memory.service -f
```

Persistent build caches live outside the disposable Actions checkout:

```text
~/.cache/memory-actions/cargo-target
~/.cache/memory-actions/npm
```

The runner exports these paths from `~/actions-runner-memory/.env`.
The first workflow after provisioning is a cold build; later jobs and runs reuse
the Cargo target and npm download caches. Do not commit the runner registration
token, `.credentials`, `.runner`, or host environment file.

Use `actionlint` before pushing workflow changes:

```bash
go run github.com/rhysd/actionlint/cmd/actionlint@v1.7.12
```

Custom labels are declared in `.github/actionlint.yaml`.

This repository is public, so untrusted fork code must never execute on the
self-hosted runner. CI skips fork pull requests, agent write jobs require a
same-repository branch, and comment-triggered agent work is limited to trusted
repository associations. Self-hosted jobs still execute with the host account's
permissions; changes to workflow triggers and shell commands require security
review.

Repository workflows share the `memory-self-hosted-runners` concurrency group.
This prevents separate workflow runs from assigning overlapping jobs to the
same runner. GitHub keeps at most one pending run in a concurrency group, so a
newer queued run can supersede an older queued run; the active run is never
canceled. Jobs inside the active workflow can still use the available platform
runners in parallel.

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
- `Debian Package Smoke`: builds an amd64 `.deb` package and uploads it as an artifact

The DB integration job runs when a pull request touches `migrations/**`, `mem-graph`, `mem-curate`, `mem-search`, `mem-service`, `mem-api`, or the shared DB test harness. It starts a `pgvector/pgvector:pg16` service and sets:

```bash
MEMORY_LAYER_TEST_DATABASE_URL=postgres://memory:memory@localhost:55432/memory_test
MEMORY_LAYER_TEST_REQUIRE_DB=1
```

Port `55432` avoids colliding with PostgreSQL already running on the host.

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

Release jobs also use self-hosted runners. Before pushing a release tag, confirm
that all of these label combinations are online:

| Artifact | Required runner labels |
| --- | --- |
| Debian amd64, validation, source, publish | `self-hosted`, `memory`, `Linux`, `X64` |
| Debian arm64 | `self-hosted`, `memory`, `Linux`, `ARM64` |
| macOS Intel | `self-hosted`, `memory`, `macOS`, `X64` |
| macOS Apple Silicon | `self-hosted`, `memory`, `macOS`, `ARM64` |
| Windows x86_64 | `self-hosted`, `memory`, `Windows`, `X64` |

Missing runners leave the corresponding release jobs queued indefinitely. List
the repository runner inventory with:

```bash
gh api repos/3vilM33pl3/memory/actions/runners \
  --jq '.runners[] | {name, status, busy, labels: [.labels[].name]}'
```

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

`.github/workflows/nightly.yml` runs once per day and can also be dispatched manually. It performs a broad validation sweep across Rust, web, offline evaluation, Debian amd64 packaging, and dependency audits. Dependency audit steps are allowed to fail so audit drift is visible without hiding build regressions.

## Dependabot

`.github/dependabot.yml` opens weekly update PRs for GitHub Actions, Cargo, and the web frontend npm dependencies.

## Branch Protection

`main` should require pull requests, conversation resolution, and the CI status checks from the incremental CI workflow. The agent resolver can update same-repository PR branches, but it does not replace human review for high-risk changes.
