# Activity Tab

Use the `Activity` tab to understand what Memory Layer has been doing and to generate a get-up-to-speed briefing for a new or returning agent.

Activity is persisted in the backend database, so the tab is no longer limited to events seen by the current TUI session.

![Activity tab](../../img/tui/activity-tab.png)

## What It Shows

- persisted project activity events such as query, scan, graph extraction, capture, curate, plan, checkpoint, watcher-health, reindex, re-embed, archive, delete, bundle, and briefing events
- operation detail from each event's structured metadata
- linked memory ids when an event created, replaced, deleted, or referenced memory
- duration and token counts when the operation reports them
- provider/model/source metadata when available
- graph retrieval diagnostics and sampled graph connections for query events that used graph-aware retrieval
- graph extraction run id, analyzer/strategy versions, symbol/reference counts, graph edge counts, and reuse flags
- `llm-audit` events, when `[llm_audit].enabled = true`, showing redacted service-side LLM prompt messages for query answers, resume summaries, and get-up-to-speed summaries

## LLM Audit Mode

LLM audit mode is opt-in because prompt messages can include sensitive memory content. In the TUI, press `A` on the Activity tab to enable or disable it. The toggle updates the running service immediately and persists the setting to the active TOML config.

You can also enable it manually in the service config:

```toml
[llm_audit]
enabled = true
redact = true
max_message_chars = 8000
max_total_chars = 32000
```

When enabled, the Activity tab records a dedicated `llm-audit` event for each covered service-side LLM request. The detail panel shows the operation, provider/model, token usage when available, request status, redaction/truncation flags, and the redacted messages sent to the LLM.

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
- `A` toggles LLM audit/debug logging in the running service and persists the config
- `h` open or close detailed help for this tab

## Related Docs

- [Activities CLI](../cli/activities.md)
- [Get Up To Speed CLI](../cli/up-to-speed.md)
- [Resume Tab](resume.md)
- [Project Tab](project.md)
