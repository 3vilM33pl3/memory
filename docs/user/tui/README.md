# TUI Guide

Memory Layer's TUI is the fastest way to browse project memory, get back into flow after an interruption, and inspect what the backend has been doing.

![Memory Layer TUI overview](../../img/tui/overview.png)

## Layout

Every TUI screen uses the same four-part layout:

- the top tab bar
- the controls row for the active tab
- the main content area
- the bottom status/footer area with TUI, service, watcher, and on Linux the watcher-manager state

Shared navigation:

- `Tab`, `Right`, or `l` moves to the next tab
- `Shift+Tab`, `Left`, or `h` moves to the previous tab
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
  - Review recent queries, captures, curation runs, scans, replacements, and watcher health events.
- [Project Tab](project.md)
  - Inspect project-level health, counts, embedding state, automation status, and replacement proposals.
- [Watchers Tab](watchers.md)
  - See active watchers, heartbeat state, restart attempts, and watcher recovery details.
- [Resume Tab](resume.md)
  - Use this after an interruption to get the current work thread, suggested next step, and recent timeline.

## Where To Start

- If you want to inspect or filter stored memory, open [Memories Tab](memories.md).
- If you want to ask a question directly, open [Query Tab](query.md).
- If you want to watch active coding-agent sessions across projects, open [Agents Tab](agents.md).
- If you want to understand what changed recently, open [Activity Tab](activity.md).
- If you want high-level health and pending review work, open [Project Tab](project.md).
- If you want watcher liveness and watchdog status, open [Watchers Tab](watchers.md).
- If you are returning after time away, open [Resume Tab](resume.md) for the re-entry briefing.
