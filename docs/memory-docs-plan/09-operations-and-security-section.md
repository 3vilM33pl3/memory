# Operations and Security Section

## Purpose

Help users run Memory Layer safely and reliably. This section makes the project feel production-minded, even if many users run it locally.

## Section Navigation

```text
Operations
  Overview
  Service operations
  Database operations
  Backups and restore
  Observability
  Performance
  Security and privacy
  Multi-project use
```

## Operations Overview Page

> Memory Layer is local-first, but it still has operational concerns: service health, database availability, backups, logs, privacy, and upgrade safety.

Route users to common tasks.

## Service Operations Page

Cover starting, stopping, restarting, checking status, running in foreground, Linux service management, macOS service management, and logs.

Commands to include where accurate:

```bash
memory service run
memory service enable
memory health
memory doctor
```

Linux-specific commands should be verified against the current package.

## Database Operations Page

Cover PostgreSQL connection string, creating database and user, enabling pgvector, migrations, connection failures, permissions, and local vs hosted database.

## Backups and Restore Page

Cover what must be backed up:

- PostgreSQL database.
- Global config.
- Repo-local `.mem/` config.
- `.agents/` integration files if needed.

Also cover what should not be committed: secrets, credentials, and local-only database URLs.

Example:

```bash
pg_dump "$DATABASE_URL" > memory-layer-backup.sql
psql "$DATABASE_URL" < memory-layer-backup.sql
```

## Observability Page

Cover health checks, doctor command, logs, watcher heartbeats, agent session status, common warning signs, and a future support bundle.

## Performance Page

Cover retrieval performance, database indexes, embedding provider latency, local vs remote PostgreSQL, project size, code graph extraction cost, and evaluation runtime.

## Security and Privacy Page

This should be prominent. Cover:

- Local-first posture.
- What data is stored.
- What data may be sent to LLM/embedding providers.
- How to disable external providers.
- Secret handling.
- MCP exposure.
- Watcher capture scope.
- Logs and redaction.
- Database access control.
- Backups and encryption.

Suggested warnings:

```mdx
<Warning>
Do not expose the MCP HTTP server to the public internet unless you have added an authentication and network control layer.
</Warning>
```

```mdx
<Note>
Memory Layer can be local-first, but embedding or LLM providers may receive text depending on your configuration.
</Note>
```

## Multi-Project Use Page

Cover one shared backend, multiple project configs, project slug, cross-project isolation, query scoping, moving projects, and archiving old projects.

## Operational Checklists

### Before upgrade

- Backup database.
- Record current version.
- Check release notes.
- Stop service if required.
- Run migration.
- Run health checks.
- Open TUI/web UI.
- Query a known memory.

### Before sharing logs

- Remove secrets.
- Remove database URLs.
- Remove API keys.
- Check project names if confidential.
- Check captured prompts or file paths.

### Before enabling an agent integration

- Confirm project config.
- Confirm MCP server binding.
- Confirm read/write permissions.
- Confirm watcher capture scope.
- Run a simple memory query.
