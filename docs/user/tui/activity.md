# Activity Events

Activity is no longer a primary TUI tab. Recent operational events are shown contextually in the `Project`, `Watchers`, `Query`, and `Resume` views, while the backend still keeps the same event stream for diagnostics and future developer tooling.

The old dedicated Activity view was demoted because most users need outcomes and status, not a separate audit screen. The event model still exists; it is just not part of the default tab cycle.

## What Activity Records

- recent backend events such as capture, curate, scan, reindex, re-embed, replacement, and checkpoint activity
- explicit plan lifecycle events for approved plan recording, plan sync, blocked completion, and verified completion
- recent queries and whether they succeeded
- watcher-health transitions such as stale, restarting, failed, and recovered

## Where To See It

- `Project` tab: compact recent activity timeline for the current TUI session
- `Watchers` tab: current watcher health, restart attempts, and last heartbeat
- `Query` tab: current query status, answer generation, and returned memories
- `Resume` tab: recent timeline when returning after an interruption
- CLI/API diagnostics: the event stream remains available to clients that need a fuller audit trail

## When To Use It

- verifying that a `scan`, `remember`, or curate action actually ran by checking the Project timeline
- confirming plan lifecycle activity through Project or Resume context
- reviewing the latest query status in the Query tab
- understanding watcher failures or recovery events through the Watchers tab and Project timeline

## See Also

- [Project Tab](project.md)
- [Query Tab](query.md)
- [Watcher Health](../cli/watchers.md)
- [Resume Briefings](../cli/resume.md)
- [TUI Guide](README.md)
