# TUI Guide

Memory Layer's TUI is the fastest way to browse project memory, ask cited questions, monitor live agents, switch embedding backends, get back into flow after an interruption, and inspect what the backend has been doing.

![Memory Layer TUI overview](../../img/tui/overview.png)

## What Makes It Useful

- Query answers show citations, confidence, diagnostics, and the ranked memories behind the answer.
- Agents and Watchers show live distributed agent state, token pressure, context usage, rate limits, heartbeats, and recovery behavior.
- Embeddings shows every configured backend and per-project coverage so model switching is visible instead of hidden in config files.
- Activity and Resume turn persisted operations into get-up-to-speed context for new or returning agents.
- Errors turns backend, provider, watcher, and TUI failures into actionable diagnostics with fix hints.
- Memories and Review keep the durable knowledge base readable and maintainable.

## Layout

Every TUI screen uses the same four-part layout:

- the top tab bar
- the controls row for the active tab
- the main content area
- the bottom status/footer area with TUI, service, watcher, and on Linux the watcher-manager state

Shared navigation:

- `Tab`, `Right`, or `l` moves to the next tab
- `Shift+Tab` or `Left` moves to the previous tab
- `h` opens detailed help for the active tab; press `h` again or `Esc` to return
- `r` refreshes the project state
- `Ctrl+C` exits the TUI

## Tabs

- [Memories Tab](memories.md)
  - Browse the canonical memory list, filter it, and inspect one memory entry in detail.
- [Agents Tab](agents.md)
  - Monitor live Codex and Claude sessions across projects with token, context, rate-limit, process, and port details.
- [Query Tab](query.md)
  - Ask a question against project memory and inspect the ranked results.
- [Activity Tab](activity.md)
  - Review persisted activity history and generate get-up-to-speed briefings for new or returning agents.
- [Errors Tab](errors.md)
  - Inspect persisted diagnostics and session-local TUI errors with explanations and suggested fixes.
- [Project Tab](project.md)
  - Inspect project-level health, counts, embedding state, recent activity, and automation status.
- [Review Tab](review.md)
  - Work the queue of pending replacement proposals: approve, reject, or change the curation replacement policy.
- [Watchers Tab](watchers.md)
  - See active watchers, heartbeat state, restart attempts, and watcher recovery details.
- [Embeddings Tab](embeddings.md)
  - List configured embedding backends, see their per-project coverage, and activate a different backend without leaving the TUI.
- [Resume Tab](resume.md)
  - Use this after an interruption to get the current work thread, suggested next step, and recent timeline.

## Where To Start

- If you want to inspect or filter stored memory, open [Memories Tab](memories.md).
- If you want to ask a question directly, open [Query Tab](query.md).
- If you want a new-agent briefing or persisted operational history, open [Activity Tab](activity.md).
- If the bottom bar shows errors or a provider/backend operation failed, open [Errors Tab](errors.md).
- If you want to watch active coding-agent sessions across projects, open [Agents Tab](agents.md).
- If you want high-level health or recent operational activity, open [Project Tab](project.md); for pending-proposal review, open [Review Tab](review.md).
- If you want watcher liveness and watchdog status, open [Watchers Tab](watchers.md).
- If you want to see which embedding backends are configured and switch which one search uses, open [Embeddings Tab](embeddings.md).
- If you are returning after time away, open [Resume Tab](resume.md) for the re-entry briefing.

## Screenshot Gallery

| Tab | Screenshot |
|---|---|
| Memories | ![Memories tab](../../img/tui/memories-tab.png) |
| Agents | ![Agents tab](../../img/tui/agents-tab.png) |
| Query | ![Query tab](../../img/tui/query-tab.png) |
| Activity | ![Activity tab](../../img/tui/activity-tab.png) |
| Errors | ![Errors tab](../../img/tui/errors-tab.png) |
| Project | ![Project tab](../../img/tui/project-tab.png) |
| Review | ![Review tab](../../img/tui/review-tab.png) |
| Watchers | ![Watchers tab](../../img/tui/watchers-tab.png) |
| Embeddings | ![Embeddings tab](../../img/tui/embeddings-tab.png) |
| Resume | ![Resume tab](../../img/tui/resume-tab.png) |
