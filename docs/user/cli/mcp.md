# MCP Server

`memory mcp` exposes Memory Layer to MCP clients such as Codex and Claude.

The built-in server is read-only in this release. It can query, resume, inspect project overview data, list memories and activities, read memory details/history, and list pending replacement proposals. It does not expose remember, capture, curate, archive, delete, reindex, or embedding mutation tools.

## Stdio

Use stdio for local agent clients:

```bash
memory mcp run --project memory
```

`memory mcp run` writes only MCP JSON-RPC messages to stdout. Diagnostic output and logs must go to stderr so stdio clients can parse the stream safely.

Example Codex-style server entry:

```toml
[mcp_servers.memory]
command = "memory"
args = ["mcp", "run", "--project", "memory"]
```

Example Claude Desktop-style server entry:

```json
{
  "mcpServers": {
    "memory": {
      "command": "memory",
      "args": ["mcp", "run", "--project", "memory"]
    }
  }
}
```

Project resolution for stdio tools is:

1. explicit tool `project` argument
2. `memory mcp run --project <slug>`
3. current initialized repo slug

If none is available, the tool returns an MCP invalid-params error.

## Status

Check the adapter and backend before configuring a client:

```bash
memory mcp status --project memory
memory mcp status --project memory --json
```

The status report checks service reachability, the selected project overview, the configured HTTP MCP mount, and the exposed tools, resource templates, and prompts.

## Streamable HTTP

`memory service run` mounts Streamable HTTP MCP at `[mcp].http_path`, `/mcp` by default, when both `[mcp].enabled` and `[mcp].http_enabled` are true.

HTTP MCP is advanced/local-only by default. Clients must send either:

- `Authorization: Bearer <service.api_token>`
- `x-api-token: <service.api_token>`

HTTP tools always require an explicit `project` argument because the service process has no trustworthy client working directory.

## Troubleshooting

- **Service down:** start the backend with `memory service run` or `memory service enable`, then run `memory mcp status --project <slug>`.
- **Missing project:** pass `--project` for stdio or include `project` in each HTTP tool call.
- **Invalid token:** refresh the service token with `memory service ensure-api-token` and update the MCP client config.
- **Stdout pollution:** stdio clients fail if wrappers print banners or logs to stdout before JSON-RPC frames. Run `memory mcp run` directly.
- **Unsupported transport:** use stdio unless your client supports MCP Streamable HTTP.

