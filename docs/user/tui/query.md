# Query Tab

Use **Query** to ask a scoped question and inspect the ranked evidence behind the answer.

![Cited query answer and ranked memory results in the TUI](https://www.memory-layer.dev/images/tui/query-tab.png)

The result includes the answer, confidence, citations, timing, matching memories, ranking reasons, and graph context when a code graph is available. If configured LLM synthesis is unavailable or cannot cite valid evidence, the UI reports the deterministic fallback instead of hiding it.

## Controls

- Press `?` from any tab to open Query with a fresh question.
- On Query, press `Enter` to begin a question; type and press `Enter` again to run it.
- `Esc` cancels question input.
- `Up` / `Down` while editing restores session-local query history and its cached results; press `Enter` to run it again.
- `j` / `k` choose a returned memory.
- `h` opens help and `r` refreshes project state.

`/` is the global filter shortcut, not a query shortcut. Use Query when you need a question answered; use the global filter to narrow what a tab displays.

See also: [Daily workflow](https://www.memory-layer.dev/docs/daily-workflow) and [Query CLI](https://www.memory-layer.dev/docs/reference/cli/query-briefings).
