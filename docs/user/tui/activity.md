# Activity Tab

**Activity** is the persisted timeline for work that Memory Layer has seen: queries, captures, curation, checkpoints, watcher transitions, graph work, embedding work, and briefings.

![Persisted activity and briefing controls in the TUI](https://www.memory-layer.dev/images/tui/activity-tab.png)

Each event can show IDs, source metadata, durations, token usage, graph diagnostics, and linked memory. Use it when a new person or agent needs evidence of what happened, not just the current answer.

## Briefings and audit controls

- `g` builds a deterministic get-up-to-speed briefing from activities, changes, warnings, and durable context.
- Uppercase `L` asks the configured LLM to synthesize the same evidence.
- Uppercase `A` toggles opt-in LLM audit logging for the running service and active config.
- `r` reloads events; `j` / `k`, `PgUp` / `PgDn`, and `Home` navigate the list and detail.

LLM audit data can contain sensitive memory content. Enable it only when you need troubleshooting evidence and keep redaction enabled.

See also: [Activities CLI](https://www.memory-layer.dev/docs/reference/cli/query-briefings) and [Resume](https://www.memory-layer.dev/docs/tui/resume).
