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
- `Offline Eval Smoke`: dry-runs the bundled offline memory evaluation suite
- `Web Build`: installs and builds the TUI/web frontend
- `Debian Package Smoke`: builds the `.deb` package and uploads it as an artifact

## Release Publishing

`.github/workflows/release.yml` runs when a `v*` tag is pushed.

It validates that the tag version matches `Cargo.toml`, `Cargo.lock`, `web/package.json`, and `web/package-lock.json`, then runs Rust validation, builds the Debian package, creates a SHA256 checksum, and publishes a GitHub Release with generated notes.

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

`.github/workflows/nightly.yml` runs once per day and can also be dispatched manually. It performs a broad validation sweep across Rust, web, offline evaluation, Debian packaging, and dependency audits. Dependency audit steps are allowed to fail so audit drift is visible without hiding build regressions.

## Dependabot

`.github/dependabot.yml` opens weekly update PRs for GitHub Actions, Cargo, and the web frontend npm dependencies.

## Branch Protection

`main` should require pull requests, conversation resolution, and the CI status checks from the incremental CI workflow. The agent resolver can update same-repository PR branches, but it does not replace human review for high-risk changes.
