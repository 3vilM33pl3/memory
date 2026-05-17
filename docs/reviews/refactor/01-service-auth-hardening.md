# Service Auth Hardening

## Review Basis

Claude flagged a local-browser auth bypass in `require_token`: requests with local `Origin` or `Referer` can pass without `x-api-token`. Current `main` still has a permissive local-origin branch for normal HTTP API routes, while MCP already has stricter token handling and origin validation.

## Goal

Require explicit service API authentication for mutating and sensitive HTTP API routes, without breaking the bundled web UI or local CLI clients.

## PR Shape

Make this a security-focused PR. Do not combine it with route refactors or web redesign.

## Implementation Notes

- Replace local-origin token bypass for `/v1/*` routes with strict token verification.
- Keep `/healthz` unauthenticated unless the project decides to make health private later.
- Add one deliberate web UI auth mechanism before removing the bypass. Recommended v1: serve a bootstrapped token to the bundled same-origin UI only, then have `web/src/api.ts` include `x-api-token` on API requests.
- Ensure non-browser clients continue using `x-api-token`; do not require cookies for CLI, MCP, or watcher flows.
- Update error messages so missing token reports "missing x-api-token header" instead of implying local origins are trusted.

## Tests

- Unit-test `require_token` with missing token plus local `Origin` and `Referer`; both must fail.
- Unit-test valid and invalid `x-api-token`.
- Add a service/web UI smoke test for the selected token bootstrap path.
- Run `cargo test -p mem-service --all-targets --locked`.

## Acceptance Criteria

- No `/v1/*` route accepts browser-origin evidence as authentication.
- Bundled web UI still works against the local service.
- MCP HTTP auth behavior remains unchanged.
- Security docs or troubleshooting text mention that localhost web apps are not trusted by origin alone.
