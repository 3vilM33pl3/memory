# Changelog

## 1.0.0 - 2026-07-05

First stable release, cut locally on monolith from the v1.0 stabilization
line plus the memory reinforcement & validation system.

### Added

- Memory reinforcement and validation system (`mem-reinforce`): access-driven
  activation scoring with spreading activation over memory relations, time
  decay, and volatility tracking; activation-aware search ranking with
  needs-review penalties; a threshold-triggered, evidence-backed LLM
  validation pipeline (opt-in, dry-run first) with human-gated corrections
  and full audit trails; `memory scores`, `memory validate`, and
  `memory review` CLI commands plus matching HTTP endpoints. See
  `docs/developer/architecture/memory-reinforcement.md`.

### Stabilization focus

- Lock the documented v1 compatibility contract for CLI, config, migrations,
  MCP read tools, and local-first service operation.
- Validate fresh installs and upgrades for Debian packages, Homebrew installs,
  and source/dev runs.
- Run the release validation gate: formatting, workspace tests, clippy, web
  tests/builds, pgvector-backed database tests, and eval gate reports.
- Keep loop automation, graph visualization, and eval research workflows
  documented as advanced surfaces where behavior is still intentionally
  conservative.

### Known issues carried into 1.0.0

- Fix the local `/v1/curate` timeout that can prevent plan-memory closure.
- Close or intentionally document stale active plan memories.
- Verify `memory doctor --fix` repairs missing or outdated Memory-owned skills
  from GitHub and falls back to the installed template when offline.
- Publish the RC from a clean pushed `main`, then promote to final only after
  packaged install, upgrade, TUI, web UI, watcher, MCP, and eval gates pass.
