# Errors Tab

**Errors** collects operational diagnostics that would otherwise be scattered across service logs and short status messages.

![Actionable error diagnostics in the TUI](https://www.memory-layer.dev/images/tui/errors-tab.png)

The list covers persisted backend diagnostics and session-local UI errors. A selected error can include a stable code, component, operation, concise explanation, fix hint, suggested diagnostic command, and raw error chain.

## Controls

- `j` / `k` choose an error.
- `PgUp` / `PgDn` and `Home` scroll detail.
- `r` refreshes activity-backed diagnostics.
- `h` opens help.

For example, `embedding_quota_exceeded` means storage may have succeeded while the provider rejected follow-up embedding work. Restore provider quota or disable automatic creation, then rerun explicit maintenance. `auth_invalid_token` points to a credential mismatch; begin with `memory doctor`.

See also: [Help](https://www.memory-layer.dev/docs/help) and [service diagnostics](https://www.memory-layer.dev/docs/reference/cli/service-health).
