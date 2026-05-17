# Service Route Split

## Review Basis

Claude identified `crates/mem-service/src/main.rs` as another god file. Current `main` still keeps route setup, auth, MCP mount, websocket streaming, API handlers, runtime status, and tests together.

## Goal

Make service changes pull-request friendly by separating routing concerns from handler implementation and runtime support.

## PR Shape

Use move-only PRs before behavior changes. Start with auth/MCP/routing because those are security-sensitive and already have focused tests.

## Implementation Notes

- Split into internal modules such as `auth`, `routes`, `handlers`, `mcp_http`, `stream`, and `runtime_status`.
- Keep the binary entrypoint thin: load config, initialize state, build router, run server.
- Move tests with the code they cover where practical.
- Preserve all route paths and response shapes.
- Do not introduce a repository layer in this PR; that is separate work.

## Tests

- Run `cargo test -p mem-service --all-targets --locked`.
- Run `cargo clippy -p mem-service --all-targets --locked -- -D warnings`.
- Verify MCP HTTP auth tests still cover bearer token, `x-api-token`, and cross-origin rejection.

## Acceptance Criteria

- A reviewer can locate auth, route mounting, and handler code without searching an 8k-line file.
- Router behavior is unchanged.
- Service tests remain colocated with their owning modules where possible.
