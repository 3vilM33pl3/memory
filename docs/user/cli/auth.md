# Authentication And Access

`memory auth` inspects the current principal and administers service tokens and
explicit project memberships. Browser users sign in through Authentik; CLI,
TUI, watcher, relay, and MCP clients use a Memory Layer service token.

## Modes

- `single_user` is the default and preserves existing local installations. The
  configured `service.api_token` remains the installation-wide credential.
- `multi_user` enables Authentik OIDC browser sessions, scoped service
  principals, project memberships, and role checks. The legacy token is
  rejected unless `auth.multi_user_legacy_token_enabled = true` is set for a
  temporary bootstrap or migration window.

Roles are cumulative: `reader` can inspect/query, `writer` can capture,
`operator` can curate and operate loops, and `admin` can manage access and
installation-wide controls. A global role applies to every project. A project
membership applies only to its project.

## Client Credential

Set the issued token in the client process:

```bash
export MEMORY_LAYER_CLIENT_TOKEN='mlt_...'
memory auth whoami
memory query --project memory --question 'What changed?'
```

`MEMORY_LAYER_CLIENT_TOKEN` takes precedence over `service.api_token`. Do not
write a multi-user service token into a repository or command history.

## Inspect Identity

```bash
memory auth whoami
memory auth whoami --json
```

The output includes principal ID, kind, global role, and effective project
access. A `401` means the credential is missing, expired, revoked, or invalid. A
`403` means the credential is valid but lacks the required role or project.

## Service Tokens

Only a global admin can create, list, or revoke tokens:

```bash
memory auth token create \
  --name hermes \
  --project memory \
  --role writer \
  --ttl 30d

memory auth token list
memory auth token revoke <token-uuid-or-unique-prefix>
```

Creation makes a service principal and an initial project membership. The raw
`mlt_...` secret is displayed once; Memory Layer stores only its SHA-256 hash.
Store the secret in OpenBao or another secret manager before closing the output.
Use memberships to grant the same service principal access to more projects.

## Project Memberships

```bash
memory auth membership grant \
  --principal <principal-uuid> \
  --project another-project \
  --role reader

memory auth membership list
memory auth membership revoke <membership-uuid>
```

Explicit memberships coexist with Authentik group mappings. Revoking an
explicit row does not remove access granted by a configured group rule.

## Browser Login

The bundled web UI redirects unauthenticated users to `/v1/auth/login`. Memory
Layer validates OIDC discovery metadata, signature, issuer, audience, state,
nonce, and PKCE before creating a 12-hour session by default. The provider
tokens are discarded. Browser writes also require a matching Origin and CSRF
value.

There is no interactive CLI OIDC login in this release. Automation should use a
service token instead of attempting to reuse browser cookies.

## OpenBao Pattern

OpenBao distributes the secret but is not a runtime dependency of Memory Layer:

```bash
bao kv put secret/memory-layer/hermes token='mlt_...'
export MEMORY_LAYER_CLIENT_TOKEN="$(bao kv get -field=token secret/memory-layer/hermes)"
memory auth whoami
```

Keep human login in Authentik and machine-secret delivery in OpenBao. Do not
store Authentik client secrets or Memory Layer service tokens in source control.

## See Also

- [MCP Server](mcp.md)
- [Service Commands](service.md)
- [Web UI](../web-ui.md)
- [Authentication Architecture](../../developer/architecture/authentication.md)
