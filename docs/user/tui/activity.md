# Activity Tab

Use the `Activity` tab to understand what Memory Layer has been doing and to generate a get-up-to-speed briefing for a new or returning agent.

Activity is persisted in the backend database, so the tab is no longer limited to events seen by the current TUI session.

## What It Shows

- persisted project activity events such as query, scan, capture, curate, plan, checkpoint, watcher-health, reindex, re-embed, archive, delete, bundle, and briefing events
- operation detail from each event's structured metadata
- linked memory ids when an event created, replaced, deleted, or referenced memory
- duration and token counts when the operation reports them
- provider/model/source metadata when available

## Get Up To Speed

The top panel can generate a briefing:

- `g` builds a deterministic briefing from persisted activities, recent memories, commits, project warnings, and token summaries
- `L` asks the configured LLM to synthesize the same evidence
- `r` reloads persisted activities from the backend

The briefing is intended for agents that need to hit the ground running. It highlights current focus, recent work, blockers, useful memories, recommended next actions, and token-heavy recent actions.

## Controls

- `j` / `k` move through activities
- `PgUp` / `PgDn` scroll activity detail
- `Home` returns detail scroll to the top
- `r` refreshes persisted activity events
- `g` generates a deterministic get-up-to-speed briefing
- `L` generates an LLM-assisted get-up-to-speed briefing

## Related Docs

- [Activities CLI](../cli/activities.md)
- [Get Up To Speed CLI](../cli/up-to-speed.md)
- [Resume Tab](resume.md)
- [Project Tab](project.md)
