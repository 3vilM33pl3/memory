# Error Messaging And Diagnostics

Memory Layer errors should be useful to both humans and agents. A failing request should explain what failed, why it probably failed, where to look, and which recovery command is relevant.

## API Shape

HTTP error responses keep the legacy `error` string and add structured diagnostic fields when the server can classify the failure:

```json
{
  "error": "The configured embedding provider rejected the request because quota or billing is exhausted.",
  "code": "embedding_quota_exceeded",
  "source": "provider",
  "component": "embeddings",
  "operation": "automatic_embedding_creation",
  "severity": "warning",
  "explanation": "The memory write can succeed while follow-up provider work fails at the provider boundary.",
  "fix_hint": "Restore provider quota/billing or disable automatic creation for the failing backend until quota is available.",
  "doctor_hint": "memory doctor",
  "command_hint": "memory embeddings list",
  "diagnostic": {
    "code": "embedding_quota_exceeded"
  }
}
```

The top-level fields are for simple clients. The nested `diagnostic` object is the canonical shape shared by the API, activity log, and TUI.

## Diagnostic Fields

- `code` is stable enough for UI grouping and tests, for example `embedding_quota_exceeded`, `auth_invalid_token`, or `database_pgvector_missing`.
- `source` identifies the boundary that rejected the operation, such as `service`, `provider`, `database`, `configuration`, `watcher`, or `tui`.
- `component` identifies the Memory Layer subsystem, such as `embeddings`, `query`, `database`, `watcher`, or `service`.
- `operation` identifies the action that failed, such as `automatic_embedding_creation`, `reembed`, `reindex`, `query`, or `heartbeat`.
- `severity` is `info`, `warning`, or `error`. Warnings should not fail the user-visible operation.
- `message` is the concise human-readable summary.
- `raw_error` preserves the original chain for debugging.
- `explanation` describes the likely cause in product terms.
- `fix_hint`, `doctor_hint`, and `command_hint` tell the user or agent what to try next.

## Persisted Diagnostics

The service records diagnostics as `ActivityKind::Diagnostic` with `ActivityDetails::Diagnostic`. The TUI Errors tab reads those persisted activities for the current project and mixes them with session-local TUI errors.

Persist a diagnostic when an operational problem matters after the request completes:

- provider quota/auth failures during automatic embedding creation
- query/search failures
- watcher health transitions to stale, failed, or restarting state
- database or migration failures that the server can classify

Do not persist every validation error. Bad user input should usually remain an immediate HTTP 400 response unless it indicates a wider operational problem.

## Automatic Versus Explicit Embedding Work

Automatic embedding creation runs after capture/curation. It must be best-effort: a provider quota or auth failure should not turn an already-saved memory into a failed curation request. Instead, the service returns the successful curation response with `warnings` and records a diagnostic activity.

Explicit maintenance commands such as `memory reembed`, `memory reindex`, and embedding prune operations remain strict. If the user explicitly asked for embedding maintenance, provider/database failures should fail the command, but the response should include the structured diagnostic and fix hints.

## Reviewed Paths

- `crates/mem-service/src/main.rs`: API error conversion, diagnostic classification, activity persistence, curate/reindex/reembed behavior.
- `crates/mem-api/src/lib.rs`: shared diagnostic model, diagnostic activity type, curation warnings.
- `crates/mem-cli/src/main.rs`: CLI formatting for structured HTTP errors.
- `crates/mem-cli/src/tui.rs`: Errors tab, diagnostic activity rendering, footer error count.

## TUI Behavior

The bottom bar shows a TUI error count when diagnostics or session-local errors are present. The Errors tab shows:

- persisted diagnostics from backend activity
- query errors from activity history
- watcher health failures/restarts
- session-local TUI failures, such as connection, activity, agents, resume, query, briefing, or embedding tab errors

Each selected error shows the code, source, component, operation, explanation, fix hint, related command, and raw error when available.
