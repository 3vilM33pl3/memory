# Service Route Split — Finish

## Review Basis

The 2026-05-18 audit (`AUDIT-2026-05-18.md`) found that plan `05-service-route-split.md` landed cosmetically. `crates/mem-service/src/routes.rs` (255 LOC) was created with `build_http_app()` and two MCP auth helpers, but `crates/mem-service/src/main.rs` grew from 8,748 → 9,007 LOC. All 103 handler functions remain in `main.rs` (`healthz` `:1866`, `query` `:2556`, `curate_memory` `:2912`, `get_memory` `:4072`, …). `lib.rs` still aliases via `#[path = "main.rs"]`. The planned `auth`/`handlers`/`mcp_http`/`stream`/`runtime_status` modules were never created.

## Goal

Finish the split the original plan described: thin binary entrypoint, handlers organized by concern, lib/bin split that actually works.

## PR Shape

Move-only PRs, security-sensitive areas first. Each PR should be reviewable as a `git diff -M` largely-moves diff. Suggested order:

1. **Auth module**: extract `crates/mem-service/src/auth.rs`. Move `require_token` (`main.rs:8746`), `mcp_token_matches`, `validate_mcp_origin`, plus the token-flow tests at `main.rs:8469`, `:8497`, `:8512`. Already partially seeded in `routes.rs`.
2. **MCP HTTP**: extract `crates/mem-service/src/mcp_http.rs`. Move MCP route handlers and helpers.
3. **Streaming**: extract `crates/mem-service/src/stream.rs`. Move `websocket()` (`main.rs:759`), `handle_websocket_connection()` (`main.rs:769`), and related plumbing.
4. **Handlers by domain**: extract `crates/mem-service/src/handlers/{memory,project,curation,activity,query,proposals,provenance,…}.rs`. Move handler functions into the matching module. The provenance endpoint at `main.rs:3015` belongs in `handlers/provenance.rs`.
5. **Runtime status**: extract `crates/mem-service/src/runtime_status.rs`. Move cluster tasks and runtime introspection.
6. **Cleanup lib/bin split**: remove `#[path = "main.rs"]` aliasing in `lib.rs`. `lib.rs` should re-export the public surface; `main.rs` should be the binary entrypoint and not much else.

## Implementation Notes

- Preserve every route path, response shape, status code, and header.
- Tests move with the code they cover.
- Repository extraction is `15-repository-layer-extend.md`'s scope; this pack must not introduce new SQL plumbing.
- Avoid touching the websocket protocol; just move the handler.
- The provenance verifier endpoint (`/v1/provenance/verify`) is in active use by `crates/mem-cli`; do not change its wire format here.

## Tests

- `cargo test -p mem-service --all-targets --locked` after each PR.
- `cargo clippy -p mem-service --all-targets --locked -- -D warnings`.
- Run the MCP HTTP auth tests explicitly — bearer token, `x-api-token`, cross-origin rejection.
- Manual smoke: start the service, hit `/healthz`, `/v1/query`, `/v1/provenance/verify`, MCP stream init.

## Acceptance Criteria

- **LOC budget**: `crates/mem-service/src/main.rs` ≤ **1,500 LOC** after the pack completes (down from 9,007). `main.rs` should be: config load, state init, router build, server run, plus a small `main()`.
- `lib.rs` no longer uses `#[path = "main.rs"]`.
- Each handler is locatable by reading the `handlers/` module names without `grep`-ing 9k lines.
- No route, response shape, status code, or auth behavior changes.
