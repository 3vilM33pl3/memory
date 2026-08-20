# Agents Help

## Purpose
Monitor live coding-agent sessions across projects, including process state, token pressure, context usage, rate limits, and active work.

## Layout
- Session table: detected Codex and Claude sessions, preferring the current project when possible.
- Detail pane: model, status, transcript, ports, child processes, current task, context budget, and rate limits.
- Auto-refresh: fast while this tab is visible, slower while hidden.

## Controls
- `j/k` or `Up/Down`: select a session.
- `PgUp/PgDn`: scroll details. `Home`: jump to top.

## Workflows
- Check which agent owns a watcher or whether a session is active, idle, stale, or over budget.
- Inspect context and rate-limit bars before adding more work to a busy session.
- Use process and port details to diagnose stuck local tools.

## Troubleshooting
- If no agents appear, check transcript permissions and watcher-manager state.
- If the wrong project is selected, restart the TUI from the intended repository.
