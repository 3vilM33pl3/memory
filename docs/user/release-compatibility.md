# Release Compatibility And Known Limitations

Memory Layer v2.0.1 is the current stable release. It retains the v2.0.0
application behavior and repairs release-artifact compatibility with databases
that already applied migrations 33 through 52. The v2 line is a breaking major
release that simplifies the runtime and prepares the record model for future
federation work. Existing PostgreSQL databases migrate in place, and bundle v1
imports remain supported, but v1 clients and configuration should be reviewed
before upgrading.

Read the [v2.0.0 release](https://github.com/3vilM33pl3/memory/releases/tag/v2.0.0)
and [changelog](../../CHANGELOG.md) before updating a v1 production installation.

## v2.0.1 schema compatibility repair

Release `v2.0.1` restores migrations 33 through 52 to every release artifact.
Some `v2.0.0` artifacts only embedded migrations through version 32, so they
refused to start when pointed at a database that had already applied the newer
migrations. The `v2.0.1` application code is otherwise the `v2.0.0` release.

Upgrade the application package to `v2.0.1`; do not downgrade the database,
delete rows from `_sqlx_migrations`, or edit an applied migration. Existing
databases at migration 52 are accepted without changing their migration ledger,
while databases below migration 52 are upgraded normally.

After installing `v2.0.1`, restart the service and verify both the binary and
database health:

```bash
memory --version
memory service restart-all
memory doctor
memory health
```

## The v2 compatibility boundary

The v2 line preserves these documented boundaries:

- the current core CLI workflows and their documented `--json` response shapes
- append-only database migrations; already-applied migrations are never edited
- the OpenAPI operations marked `x-stability: core`; control-plane operations
  marked `internal` may evolve between minor releases
- read-only MCP query, search, resume, resource, and prompt tools
- deterministic bundle schema v2 exports and backward-compatible bundle v1 imports
- packaged operation on Debian amd64/arm64, Homebrew, macOS Intel/Apple Silicon,
  and Windows x86_64
- source/dev isolation from the installed service profile

Advanced surfaces remain deliberately conservative. Loop automations are
approval-gated, graph quality depends on repository/extractor coverage, and
evaluation claims require reviewed suites and passing gates.

## Upgrading from v1

Review these breaking changes before restarting the service:

- Remove `capnp_unix_socket` and `capnp_tcp_addr`; clients now receive live
  updates from the `/ws` WebSocket stream.
- Replace `memory automation flush` with `memory watcher flush`,
  `memory capture task` with `memory capture`, and `memory dev init` with
  `memory dev`. The duplicate `memory setup` command is gone; use
  `memory wizard`.
- Update direct API integrations against the running service's
  `GET /v1/openapi.yaml`. v2 removed `/v1/stats`, `/v1/offline/pending`, and the
  browser auth-token handoff, and consolidated several loop/activity routes.
- Treat `--writer-id` and `[writer]` as advisory labels. Durable authorship is
  derived from the authenticated principal.
- Reissue least-privilege service tokens where appropriate. Role names remain
  convenient presets, but authorization is enforced as explicit permission
  sets rather than an ordinal role ladder.
- Re-export shared bundles when practical. New exports use deterministic,
  content-addressed schema v2; existing schema v1 bundles still import.

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

The v2.0.1 service embeds migrations through version 52 and applies any pending
migrations when it starts. Do not downgrade the binary against a migrated
database; restore the pre-upgrade database backup if you must roll back. Run
`memory upgrade` only after reviewing the dry run because it can refresh
repo-local `.agents/` skills and instructions.

## Release artifacts

GitHub Releases publish the supported native installer set:

- Debian x86_64: `memory-layer_<version>_amd64.deb`
- Debian arm64: `memory-layer_<version>_arm64.deb`
- macOS Intel: `memory-layer-<version>-macos-x86_64.pkg`
- macOS Apple Silicon: `memory-layer-<version>-macos-aarch64.pkg`
- Windows x86_64: `memory-layer-<version>-windows-x86_64.msi`
- Windows x86_64 portable archive: `memory-layer-<version>-windows-x86_64.zip`
- Homebrew source archive: `memory-<version>.tar.gz`

For v2.0.1, replace `<version>` with `2.0.1`. Every published package has a
matching `.sha256` checksum file.

The Debian arm64 package targets 64-bit ARM Linux, including Raspberry Pi 4/5
systems running a 64-bit Debian-family OS. Windows ARM64 and 32-bit Raspberry Pi
OS are not release targets yet.

## Current limitations

- Run `memory doctor` and `memory health` after every package upgrade.
- Review `memory upgrade --dry-run` before refreshing repo-local skills or
  instructions.
- Verify the installer architecture before installing: Debian publishes `amd64`
  and `arm64`, macOS publishes Intel and Apple Silicon packages, and Windows
  publishes x86_64 packages.
- The code graph UI depends on browser WebGL support and extractor coverage.
- The interactive browser demo uses static sample data; it does not prove that
  your local backend, database, or authentication is healthy.
- Loop automations remain approval-gated by design, and risky actions must stop
  for human review.

## Next

Read [Getting Started](getting-started.md), [Doctor Diagnostics](cli/doctor.md), or [Skill Upgrade](cli/upgrade.md).
