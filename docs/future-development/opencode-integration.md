# OpenCode Integration

This note captures the likely shape of a future OpenCode integration for Memory
Layer. The goal is to make OpenCode a first-class agent client without creating
a second persistence path or duplicating Memory's backend behavior.

## Summary

An OpenCode integration makes sense in two layers:

1. MCP client setup for immediate read-only Memory access.
2. Native watcher and agent-session support after OpenCode session discovery is
   confirmed to be stable.

The first layer should be small and low risk. OpenCode supports MCP server
configuration and management, while Memory already exposes a read-only MCP
adapter through `memory mcp run` and the local HTTP `/mcp` endpoint. That means
OpenCode can query, resume, inspect project state, list activities, and review
replacement proposals without any new Memory storage or API surface.

The second layer should make OpenCode visible in the same places as Codex and
Claude: the watcher manager, TUI Agents tab, watcher ownership, and activity
metadata. That work depends on a reliable way to discover OpenCode sessions,
their working directory, stable session id, process id, model, and token or
context usage.

## Current Memory Shape

Memory Layer already has the pieces needed for a narrow integration:

- `mem-mcp` exposes read-only project memory tools, resources, and prompts.
- `memory mcp run` supports stdio MCP clients.
- `memory service` can mount Streamable HTTP MCP at `/mcp`.
- `mem-agenttop` has agent-specific collectors for Codex and Claude sessions.
- `memory watcher manager` starts one watcher per detected agent session.
- The TUI already renders agent sessions, watcher ownership, token pressure,
  process state, child processes, and open ports.

The integration should extend those seams rather than adding OpenCode-specific
storage.

## Proposed V1: MCP Client Documentation

Document OpenCode as an MCP client and provide a copyable local setup example.

Example stdio shape:

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "memory": {
      "type": "local",
      "command": ["memory", "mcp", "run", "--project", "memory"]
    }
  }
}
```

If OpenCode expects a different local command shape at implementation time, keep
the documented intent the same: run Memory's stdio MCP command and avoid exposing
write tools.

The user-facing docs should explain:

- MCP is the recommended first integration path.
- The MCP server is read-only in v1.
- Project-scoped stdio config is preferred for local agent use.
- HTTP MCP is advanced and should remain local/token-protected.
- `memory mcp status --project <slug>` is the first troubleshooting command.

## Proposed V2: Native Agent Monitoring

Add OpenCode support to `mem-agenttop` after confirming stable local session
discovery.

Implementation shape:

1. Add `crates/mem-agenttop/src/collector/opencode.rs`.
2. Register `OpenCodeCollector` in `MultiCollector`.
3. Add `opencode::collect_lightweight_sessions` so the watcher manager can
   start one watcher per live OpenCode session.
4. Populate `AgentSession.agent_cli = "opencode"`.
5. Parse cwd, session id, pid, started time, model, status, and token/context
   fields when available.
6. Keep unknown token/context fields as zero or `"-"` rather than inventing
   values.
7. Update docs and UI copy from "Codex and Claude" to "Codex, Claude, and
   OpenCode" only where support is actually implemented.

OpenCode appears to expose useful CLI surfaces such as `opencode session list`,
`opencode export`, `opencode stats`, `opencode run`, and MCP management. Prefer
stable CLI or documented config/session surfaces over reverse-engineering
internal files unless the upstream format is clearly durable.

## Non-Goals

- Do not build an OpenCode-specific Memory database.
- Do not expose write-capable MCP tools just for OpenCode.
- Do not parse private or unstable session files before checking for a supported
  CLI/API surface.
- Do not label OpenCode watcher support as complete until live session discovery
  and watcher ownership are verified.

## Open Questions

- Where does OpenCode store local session metadata on Linux and macOS?
- Does `opencode session list --format json` include cwd, pid, model, and start
  time, or only historical session metadata?
- Can a live OpenCode TUI or `opencode run` process be mapped reliably to a
  session id?
- Are token and context usage available from `opencode stats`, `opencode export`,
  local session state, or runtime events?
- Does OpenCode expose hooks or plugins that can call Memory commands at task
  start and completion?

## Recommended Sequence

1. Add OpenCode MCP setup documentation.
2. Manually test `memory mcp run --project <slug>` from OpenCode.
3. Inspect OpenCode's stable session list/export JSON for live-session fields.
4. Prototype `OpenCodeCollector` behind tests with fixture JSON.
5. Wire lightweight session detection into the watcher manager.
6. Update TUI and docs wording after runtime verification.
