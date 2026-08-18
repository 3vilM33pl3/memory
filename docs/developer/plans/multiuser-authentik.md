# Memory Layer Multiuser Authentik Integration

Status: implemented and verified on 2026-08-18.

- [x] Add opt-in `multi_user` authentication configuration while preserving `single_user` defaults.
- [x] Add principal, service-token, project-membership, browser-session, OIDC-flow, and audit persistence.
- [x] Centralize credential resolution and reject conflicting browser, bearer, and legacy credentials.
- [x] Enforce reader, writer, operator, and admin policies across every service route.
- [x] Resolve Authentik group mappings and persisted project memberships into effective project access.
- [x] Restrict global search and ID-addressed resources to projects authorized for the caller.
- [x] Keep configured read-only mode as a ceiling over authenticated roles.
- [x] Stamp actor-controlled fields from the authenticated principal in multiuser mode.
- [x] Add scoped service-token APIs with one-time secret display, hashing, expiry, listing, and revocation.
- [x] Add Authentik OIDC Authorization Code flow with PKCE, state, nonce, secure sessions, CSRF, and lazy discovery.
- [x] Keep MCP HTTP cookie-blind and authenticate it only with scoped API tokens.
- [x] Carry caller authentication through CLI, Cap'n Proto, WebSocket, and relay paths without privilege escalation.
- [x] Add CLI commands for identity, service-token, and project-membership administration.
- [x] Add browser sign-in, current identity, sign-out, and an admin Access tab for principals, tokens, and memberships.
- [x] Document Authentik setup, OpenBao-backed token delivery, migration, rollback, transport behavior, and errors.
- [x] Add unit, database, route-policy, OIDC, CLI, MCP, transport, web, compatibility, and security regression tests.
- [x] Run focused and workspace verification, complete the plan checkpoint, and remember the verified implementation.

## Verification

- `cargo test -p mem-api -p mem-search -p mem-mcp -p mem-service -p mem-cli --all-targets --locked`
- `cargo clippy -p mem-api -p mem-search -p mem-mcp -p mem-service -p mem-cli --all-targets --locked -- -D warnings`
- `cargo fmt --all -- --check`
- `npm test` and `npm run build` in `web/`
- `npm run lint:links`, `npm run check:assets`, and `npm run build` in `docs-site/`
- OpenAPI YAML parsing and route-inventory contract test
- Playwright desktop and mobile checks of the multi-user admin Access view

Database integration tests require `MEMORY_LAYER_TEST_DATABASE_URL`; they remain skip-capable when that dedicated test database is not configured.
