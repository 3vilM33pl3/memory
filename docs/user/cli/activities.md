# `memory activities`

`memory activities` lists persisted project activity events from the backend database.

Use it when a human or agent needs to inspect what Memory Layer recently did: queries, scans, captures, curation, plan lifecycle events, watcher transitions, graph extraction, reindexing, re-embedding, archive/delete operations, bundle transfers, and get-up-to-speed briefings.

## Common Usage

```bash
memory activities --project memory
memory activities --project memory --limit 50 --text
memory activities --project memory --kind query
memory activities --project memory --kind llm_audit
```

JSON is the default so agents can consume it directly. Use `--text` for a compact human-readable timeline.

## What It Shows

- event id, project, kind, summary, and recorded time
- structured event details where available
- linked memory id when the event changed or referenced a memory
- duration in milliseconds when the operation reports it
- token counts when an LLM provider returned usage metadata
- source, provider, model, actor, and operation metadata when available
- query graph retrieval status, candidate counts, timing, and sampled graph connections when a query used graph-aware retrieval
- graph extraction run id, analyzer/strategy versions, symbol/reference counts, graph edge counts, and reuse flags
- `llm_audit` events, when `[llm_audit].enabled = true`, with redacted service-side LLM prompt messages and request status

## LLM Audit Mode

Enable audit mode in the service config when you need to debug what Memory Layer sends to an LLM:

```toml
[llm_audit]
enabled = true
redact = true
max_message_chars = 8000
max_total_chars = 32000
```

Audit events are disabled by default. You can also toggle audit mode from the TUI Activity tab with `A`; the TUI updates the running service immediately and persists the same config setting. With `redact = true`, Memory Layer redacts common API key, bearer token, password, secret, and database URL credential patterns before storing the prompt messages.

Older events remain readable even if they were recorded before token and metadata columns existed; those fields appear as `null` or `-`.

## Related Docs

- [Get Up To Speed](up-to-speed.md)
- [Activity Tab](../tui/activity.md)
- [Resume Briefings](resume.md)
