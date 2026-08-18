# ADR 0007: Authentik Human Identity and Scoped Service Principals

Date: 2026-08-18
Status: accepted
Supersedes: [ADR 0006](0006-shared-classroom-multi-user.md)

## Context

Memory Layer's original shared `service.api_token` represents the installation,
not a person or workload. It cannot safely support a browser deployment shared
by multiple people, project-specific access, revocation, or trustworthy writer
attribution.

The deployment environment already separates human identity from workload
secrets: Authentik authenticates people, while OpenBao distributes secrets to
machines. Memory Layer must preserve that boundary rather than making either
system a runtime dependency of the other.

## Decision

Authentication is opt-in through `auth.mode = "multi_user"`. The default
`single_user` mode preserves existing local installations.

- Browser users authenticate with Authentik using OIDC Authorization Code flow,
  PKCE, state, and nonce. Memory Layer stores a short-lived opaque session and
  discards provider tokens after validating claims.
- CLI, TUI, watcher, relay, and MCP clients authenticate as service principals
  with high-entropy bearer tokens. Only SHA-256 token hashes are persisted.
- OpenBao is the preferred distribution mechanism for service-token secrets,
  but Memory Layer does not call OpenBao at runtime.
- Principals receive project memberships with `reader`, `writer`, `operator`,
  or `admin` roles. Authentik groups may add configured project or global
  grants. Global administrators are unrestricted.
- One service principal may hold memberships in several projects. Global search
  returns only projects authorized for that principal.
- In multiuser mode, actor identity is stamped by the server. Client-supplied
  writer fields are not authoritative.
- `service.read_only` remains a system-wide ceiling even for administrators.
- HTTP MCP deliberately ignores browser cookies and accepts service bearer
  tokens only.

The legacy installation token remains valid in `single_user`. In `multi_user`
it is disabled unless `auth.multi_user_legacy_token_enabled = true`, which is a
documented bootstrap and recovery mechanism granting global administrator
access.

## Consequences

Every protected route is covered by a centralized authorization policy and an
exhaustive policy test. Resource IDs are resolved to their owning project before
authorization. Authentication failures return 401; authenticated principals
without sufficient access receive 403.

Raw service tokens are shown once, browser sessions are HttpOnly, state/session/
CSRF secrets are stored as hashes, and authentication audit records never store
credentials. OIDC discovery is lazy so an Authentik outage does not prevent the
service or non-browser agents from starting.
