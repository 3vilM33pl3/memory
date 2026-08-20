# Watchers Help

## Purpose
Show project watchers, heartbeat state, agent ownership, service identity, restart attempts, and recovery behavior.

## Layout
- Scrollable watcher report.
- Each watcher shows health, mode, repo root, watcher id, owner agent/session/pid, host service, heartbeat, and restart attempts.
- Managed watchers belong to agent sessions; manual watchers were started directly.

## Controls
- `j/k` or `Up/Down`: scroll.
- `PgUp/PgDn`: page. `Home`: jump to top.
- `r`: refresh project state outside help.

## Workflows
- Use this tab when captures are not appearing or watcher health is degraded.
- Check owner/session and stale heartbeat before restarting anything.
- Use restart attempts to distinguish transient restarts from repeated failures.

## Troubleshooting
- If a managed watcher stays stale, check Manager footer and Errors.
- If only manual watchers exist, start through the manager-integrated path.
