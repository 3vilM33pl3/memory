# Built-In MCP Server

Memory Layer includes a first-class MCP server in `crates/mem-mcp`.

The crate is a protocol adapter over the existing `mem-service` HTTP API. It does not read PostgreSQL directly and does not create a second persistence or query path. This keeps authentication, retrieval behavior, resume logic, watcher summaries, replacement proposal reads, and activity formats aligned with the normal CLI, TUI, and web UI.

## Transports

- **stdio:** `memory mcp run` constructs the adapter in the CLI process and serves it through the official Rust MCP SDK stdio transport.
- **Streamable HTTP:** `memory service run` mounts the official Rust MCP SDK `StreamableHttpService` at `[mcp].http_path`, `/mcp` by default.

The HTTP mount validates `Origin` when present and requires either `Authorization: Bearer <service.api_token>` or `x-api-token: <service.api_token>` when `[mcp].require_token` is true.

## Read-Only Surface

The v1 MCP surface exposes:

- tools for query, resume, up-to-speed, overview, memory list/detail/history, activities, and pending replacement proposals
- resource templates for project overview, memories, activities, memory detail, and memory history
- prompts for getting up to speed and answering project questions with Memory context

Write-capable Memory operations remain absent from MCP v1 by design.

## Project Resolution

Stdio can safely use local process context:

1. explicit tool `project`
2. `memory mcp run --project`
3. current initialized repo slug

HTTP cannot trust cwd, so HTTP tool calls must pass `project` explicitly.

## Extension Points

Add new MCP capabilities in `mem-mcp` only after the backing behavior exists in the service HTTP API. That preserves the service as the system boundary and avoids introducing hidden state mutation paths.
