# Information Architecture

## Objective

Design a documentation structure that makes Memory Layer feel like a mature developer platform. The site should not expose the repository structure directly; it should guide readers through what the product does, how to run it, how agents use it, and how to evaluate it.

## Proposed Top-Level Navigation

```text
Get started
Install
Concepts
Agents
Watchers
MCP
Evaluations
Operations
Reference
Help
```

## Full Page Tree

```text
/
  index.mdx
  quickstart.mdx
  showcase.mdx
  features.mdx

/install
  index.mdx
  requirements.mdx
  linux-debian.mdx
  macos-homebrew.mdx
  from-source.mdx
  postgresql-pgvector.mdx
  wizard-global.mdx
  wizard-project.mdx
  service-setup.mdx
  update.mdx
  uninstall.mdx

/concepts
  index.mdx
  mental-model.mdx
  projects.mdx
  memories.mdx
  evidence.mdx
  curation.mdx
  retrieval.mdx
  embeddings.mdx
  code-graph.mdx
  activity-events.mdx
  trust-and-staleness.mdx

/agents
  index.mdx
  codex-cli.mdx
  claude-code.mdx
  generic-agent.mdx
  agent-install-prompt.mdx
  agent-project-init-prompt.mdx
  agent-briefings.mdx
  permissions-and-safety.mdx

/watchers
  index.mdx
  generic-watcher.mdx
  codex-profile.mdx
  claude-profile.mdx
  session-detection.mdx
  token-pressure.mdx
  lifecycle-and-heartbeats.mdx
  troubleshooting.mdx

/mcp
  index.mdx
  stdio.mdx
  streamable-http.mdx
  tools.mdx
  client-setup.mdx
  security.mdx
  examples.mdx

/evals
  index.mdx
  ablation-tests.mdx
  run-evals.mdx
  benchmark-reports.mdx
  metrics.mdx
  reproducibility.mdx
  interpreting-results.mdx
  limitations.mdx

/operations
  index.mdx
  service-operations.mdx
  database-operations.mdx
  backups-and-restore.mdx
  observability.mdx
  performance.mdx
  security-and-privacy.mdx
  multi-project-use.mdx

/reference
  index.mdx
  cli.mdx
  config-global.mdx
  config-project.mdx
  database-schema.mdx
  environment-variables.mdx
  exit-codes.mdx
  files-and-directories.mdx

/help
  index.mdx
  troubleshooting.mdx
  faq.mdx
  common-errors.mdx
  doctor-and-health.mdx
  support-bundle.mdx
  contributing-docs.mdx
```

## Left Navigation Pattern

Each section should have grouped left-nav headings. Example for Install:

```text
Install overview
  Install
  Requirements

Local install
  Linux / Debian
  macOS / Homebrew
  From source

Database
  PostgreSQL and pgvector

Configuration
  Global wizard
  Project wizard
  Service setup

Maintenance
  Update
  Uninstall
```

## Homepage Routing Blocks

Use task-oriented cards:

```text
Install Memory Layer
Configure a project
Connect a coding agent
Run the MCP server
Start a watcher
Query memory with evidence
Run an evaluation
Operate the service
```

## Must-Have Launch Pages

- Homepage
- Quick start
- Install overview
- PostgreSQL and pgvector
- Global wizard
- Project wizard
- Codex CLI integration
- Claude Code integration
- MCP overview
- Evaluation overview
- CLI reference
- Troubleshooting

## Naming Rules

Prefer action-oriented page names:

- `Connect Codex CLI`, not `Codex profile internals`
- `PostgreSQL and pgvector`, not `Storage backend`
- `Query memory with evidence`, not `Memory retrieval`
- `Run evaluations`, not `Eval harness`
