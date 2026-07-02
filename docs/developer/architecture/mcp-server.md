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
- `memory_search_all` for cross-project retrieval with result-level project slug, project name, and repository root metadata
- resource templates for project overview, memories, activities, memory detail, and memory history
- prompts for getting up to speed, answering project questions with Memory context, and routing cross-project tasks before follow-up actions

Write-capable Memory operations remain absent from MCP v1 by design.

## Cross-Project Search

`memory_search_all` is the agent routing tool. It calls `POST /v1/query/global` on the service and receives the same `QueryResponse` shape as project query, with optional `project`, `project_name`, and `repo_root` fields populated on results and answer citations.

The search crate owns the scoped/global distinction through a shared query execution model:

- project query binds a project slug and keeps the old `/v1/query` behavior
- global query leaves the project scope unset, so lexical, semantic, graph, provenance, and reranking logic run across all active projects

Global query is still read-only. It should not infer write targets by cwd. Agents that need follow-up action must choose the repository from returned `repo_root` metadata, then use project-scoped tools or local repository actions.

The `memory_route_cross_project_task` prompt documents this sequence for clients that support MCP prompts.

## Project Resolution

Stdio can safely use local process context:

1. explicit tool `project`
2. `memory mcp run --project`
3. current initialized repo slug

HTTP cannot trust cwd, so HTTP tool calls must pass `project` explicitly. `memory_search_all` has no `project` argument because it deliberately searches every project and returns routing metadata instead.

## Extension Points

Add new MCP capabilities in `mem-mcp` only after the backing behavior exists in the service HTTP API. That preserves the service as the system boundary and avoids introducing hidden state mutation paths.
