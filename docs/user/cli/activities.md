# `memory activities`

`memory activities` lists persisted project activity events from the backend database.

Use it when a human or agent needs to inspect what Memory Layer recently did: queries, scans, captures, curation, plan lifecycle events, watcher transitions, reindexing, re-embedding, archive/delete operations, bundle transfers, and get-up-to-speed briefings.

## Common Usage

```bash
memory activities --project memory
memory activities --project memory --limit 50 --text
memory activities --project memory --kind query
```

JSON is the default so agents can consume it directly. Use `--text` for a compact human-readable timeline.

## What It Shows

- event id, project, kind, summary, and recorded time
- structured event details where available
- linked memory id when the event changed or referenced a memory
- duration in milliseconds when the operation reports it
- token counts when an LLM provider returned usage metadata
- source, provider, model, actor, and operation metadata when available

Older events remain readable even if they were recorded before token and metadata columns existed; those fields appear as `null` or `-`.

## Related Docs

- [Get Up To Speed](up-to-speed.md)
- [Activity Tab](../tui/activity.md)
- [Resume Briefings](resume.md)
