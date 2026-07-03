# Changelog

## 1.0.0-rc.1 - Unreleased

This release candidate is the v1.0 stabilization line. It should not accept broad
new features after the branch is cut; only bug fixes, docs, packaging,
migration, validation, and release-blocking polish should land.

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

### Release blockers to clear before final v1.0.0

- Fix the local `/v1/curate` timeout that can prevent plan-memory closure.
- Close or intentionally document stale active plan memories.
- Verify `memory doctor --fix` repairs missing or outdated Memory-owned skills
  from GitHub and falls back to the installed template when offline.
- Publish the RC from a clean pushed `main`, then promote to final only after
  packaged install, upgrade, TUI, web UI, watcher, MCP, and eval gates pass.
