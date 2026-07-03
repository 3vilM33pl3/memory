# Release Compatibility And Known Limitations

Memory Layer v1.0 is intended to be a stable local-first release. It preserves
the documented install, service, query, TUI, web UI, watcher, skill, and MCP
workflows while keeping newer automation surfaces conservative.

## Compatibility promise

The v1 line aims to preserve:

- documented CLI commands and `--json` response shapes for core workflows
- global config, project config, and repo-local skill locations
- append-only database migrations; already-applied migrations must not be edited
- read-only MCP query, search, resume, resource, and prompt tools
- packaged service behavior for Debian and Homebrew installs
- source/dev isolation from the installed service profile

Compatibility does not mean every experimental feature is frozen. Advanced
surfaces may change if the docs mark them as experimental or advanced.

## Advanced surfaces

Treat these as advanced in v1.0:

- Loop automation: approval-gated and local-first; risky actions must stop for
  human review.
- Code graph visualization: useful for navigation, but WebGL support and graph
  extraction quality depend on the browser, repository, and extractor coverage.
- Evaluation research extensions: useful for release discipline, but benchmark
  claims still depend on reviewed held-out suites.
- Browser demo data: shows product behavior without a backend, but it is not a
  substitute for a live service.

## Upgrade guidance

Before upgrading:

```bash
memory status --project <project-slug>
memory doctor
pg_dump "$DATABASE_URL" > memory-layer-before-upgrade.sql
```

After upgrading:

```bash
memory service restart-all
memory doctor
memory health
memory status --project <project-slug>
memory upgrade --dry-run
```

Run `memory upgrade` only after reviewing the dry run because it can refresh
repo-local `.agents/` skills and instructions.

## Known v1.0 release-candidate blockers

- Plan-memory completion can fail if `/v1/curate` times out.
- Repositories with several active plan memories may need explicit
  `--thread-key` when finishing a plan.
- Final v1.0 should only be tagged after packaged install and upgrade testing
  passes for the release candidate.

## Next

Read [Getting Started](getting-started.md), [Doctor Diagnostics](cli/doctor.md), or [Skill Upgrade](cli/upgrade.md).
