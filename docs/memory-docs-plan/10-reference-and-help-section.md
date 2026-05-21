# Reference and Help Section

## Purpose

Provide a complete lookup area for commands, configuration, files, errors, and troubleshooting.

## Reference Navigation

```text
Reference
  Overview
  CLI
  Global config
  Project config
  Database schema
  Environment variables
  Exit codes
  Files and directories
```

## Help Navigation

```text
Help
  Overview
  Troubleshooting
  FAQ
  Common errors
  Doctor and health
  Support bundle
  Contributing docs
```

## CLI Reference Page

Create one page initially, then split later if it becomes too long.

Suggested sections:

```mdx
# CLI reference

## Global options

## Commands

### `memory wizard`
### `memory doctor`
### `memory health`
### `memory service`
### `memory tui`
### `memory scan`
### `memory commits`
### `memory eval`
### `memory mcp`
```

For each command include synopsis, description, options, examples, expected output, and related commands.

## Global Config Page

Document file path, required fields, optional fields, database URL, embedding provider settings, LLM provider settings, secrets, safe editing, and regeneration.

## Project Config Page

Document `.mem/project.toml`, project slug, repo-local config, `.agents/` files, relation to global config, what should be committed, and what should not be committed.

## Database Schema Page

Keep high-level unless the schema is stable. Cover memories, evidence, embeddings, projects, activity events, code graph entities, and evaluation artifacts if stored.

## Environment Variables Page

Use a table:

```mdx
| Variable | Required | Default | Description |
|---|---:|---|---|
| `DATABASE_URL` | Sometimes | none | PostgreSQL connection string |
```

## Troubleshooting Page

Use a routing page:

```text
Install failed
Database connection failed
pgvector missing
Wizard cannot write config
Service will not start
MCP client cannot connect
Watcher does not detect session
Agent cannot find memories
Evaluation failed
```

## Common Errors Page

For each error:

```mdx
# Error message

## Meaning

## Fix

## Verify
```

Examples:

- `connection refused`
- `extension "vector" is not available`
- `DATABASE_URL is not set`
- `permission denied`
- `memory command not found`
- MCP server connection refused
- Project not configured
- No memories found
- Watcher heartbeat missing

## Doctor and Health Page

Explain the difference between `memory doctor` and `memory health`, when to run each, how to interpret output, and what to include in a support issue.

## FAQ Page

Initial questions:

- Is Memory Layer a vector database?
- Does it replace documentation?
- Does it work offline?
- Does it send data to OpenAI or Anthropic?
- Can I use local embeddings?
- Can multiple projects share one database?
- Can multiple agents use it at once?
- What should become a memory?
- How do I delete a memory?
- How do I handle stale memories?
- How do I know it works?

## Support Bundle Page

If the product does not yet have a support bundle command, document the desired future behaviour. It should collect version, OS, config summary with secrets redacted, health output, recent logs, service status, database connectivity, project slug, MCP status, and watcher status.
