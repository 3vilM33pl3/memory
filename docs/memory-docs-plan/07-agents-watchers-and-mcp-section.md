# Agents, Watchers, and MCP Section

## Purpose

Document how Memory Layer connects to coding agents and developer workflows. This is one of the most important parts of the site because it makes the product concrete.

## Agents Section

### Navigation

```text
Agents
  Overview
  Codex CLI
  Claude Code
  Generic agent
  Agent install prompt
  Agent project-init prompt
  Agent briefings
  Permissions and safety
```

### Agents Overview Page

Explain that agents need project context, Memory Layer exposes context through CLI/MCP/project-local config, agents should retrieve evidence before making assumptions, and humans can install/configure directly or use an agent prompt.

### Codex CLI Page

Cover required Memory Layer setup, project-local config, how Codex should discover Memory Layer, querying memories, briefings, recommended agent instructions, and common failure modes.

### Claude Code Page

Use the same shape as Codex CLI, with Claude-specific config or instruction files where confirmed.

### Generic Agent Page

For any CLI-based or MCP-capable coding agent. Cover CLI usage, MCP stdio, MCP Streamable HTTP, and the prompting pattern: check project memory first, cite memory IDs, verify before edits, store durable discoveries.

### Agent Install Prompt Page

Package the install prompt from the README as a first-class docs page. Include when to use it, what the agent is allowed to do, what it must ask before doing, full copyable prompt, and verification checklist.

### Agent Project-Init Prompt Page

Do the same for configuring a project once Memory Layer is installed.

### Agent Briefings Page

Explain returning to a project and generating a briefing from recent events, memory changes, commits, warnings, and token summaries.

## Watchers Section

### Navigation

```text
Watchers
  Overview
  Generic watcher
  Codex profile
  Claude profile
  Session detection
  Token pressure
  Lifecycle and heartbeats
  Troubleshooting
```

### Watchers Overview Page

> Watchers observe agent sessions and project activity so Memory Layer can capture useful context without requiring every fact to be manually entered.

Cover what watchers capture, what they do not capture, privacy implications, session attachment, and lifecycle behaviour.

### Generic Watcher Page

Document inputs, session identity, project detection, output events, and failure handling.

### Codex Profile and Claude Profile Pages

For each profile: how the profile identifies sessions, what metadata it captures, how to enable it, how to verify it, and common issues.

### Token Pressure Page

Explain why token pressure matters, how it can be used in briefings, and how agents should respond when context is running out.

## MCP Section

### Navigation

```text
MCP
  Overview
  stdio server
  Streamable HTTP server
  Tools
  Client setup
  Security
  Examples
```

### MCP Overview Page

> The MCP server exposes read-only project memory tools to MCP-capable clients, including coding agents.

Cover why MCP matters, when to use CLI vs MCP, stdio vs Streamable HTTP, read-only posture, and project scoping.

### MCP Tools Page

List tools with name, purpose, inputs, output, example, and safety notes.

### MCP Client Setup Page

Include setup snippets for likely clients, but use TODO blocks where exact config needs confirming from the repo.

### MCP Security Page

Cover localhost binding, read-only design, project scoping, secrets, logs, and avoiding public exposure of MCP HTTP.

## Cross-Linking

Every agent page should link to MCP setup, watcher setup, project wizard, troubleshooting, and CLI reference. Every watcher page should link to agents overview, activity events concept, operations logs, and troubleshooting.
