# Activity Help

## Purpose
Review persisted backend activity and generate get-up-to-speed briefings for new or returning agents.

## Layout
- Briefing panel: deterministic or LLM-generated continuity context plus LLM audit/debug status.
- Activity table: event time, kind, tokens, duration, and summary.
- Detail pane: selected event metadata, including query diagnostics, graph details, token usage, or curation counts.

## Controls
- `j/k` or `Up/Down`: select activity.
- `PgUp/PgDn`: scroll detail. `Home`: jump to top.
- `g`: deterministic briefing. `Shift+L`: LLM briefing. `r`: refresh.
- `Shift+A`: toggle LLM audit/debug logging in the running service and persist the config.

## Workflows
- Use this tab at handoff or after interruption.
- Enable audit briefly when you need to inspect service-side LLM prompts, then disable it after debugging.
- Inspect token and duration fields to understand cost and latency.
- Open query activities to see retrieval mode, graph behavior, and answer cost.

## Troubleshooting
- If activity is empty, perform a query, capture, curation, graph extraction, or embedding operation.
- If LLM briefing fails, use deterministic briefing and check Errors.
