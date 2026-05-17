# Watcher Health

This page explains how watcher liveness and recovery show up in Memory Layer.

## Table of Contents

- [What The Watcher Does](#what-the-watcher-does)
- [Watcher Health States](#watcher-health-states)
- [Where To See Watcher Health](#where-to-see-watcher-health)
- [Recovery Signals](#recovery-signals)
- [Common Commands](#common-commands)

## What The Watcher Does

`memory watcher` is the background process that watches a project and sends useful work context to `memory service`.

On Linux and macOS, the recommended path is the Codex-linked watcher manager:

```bash
memory watcher manager enable
```

That installs a persistent user manager service:

- on Linux it uses `systemd --user`
- on macOS it uses a `launchd` LaunchAgent

The manager detects live Codex sessions, bootstraps git repos if needed, and starts one watcher per Codex session.

The manager is event-driven. It watches agent session directories for Codex/Claude changes and only reconciles after a debounced session event, with a slow fallback scan for missed events. A singleton lock prevents accidentally running two managers at the same time.

Project watchers are also event-driven by default. They watch the repository, debounce file changes, and then run one `git status --porcelain` pass for the burst. `poll_interval` is now the fallback safety scan interval, not the primary loop. New configs use:

```toml
[automation]
file_events = true
poll_interval = "60s"
```

If filesystem watching is unavailable, the watcher logs a warning and falls back to polling.

Legacy per-project service-managed watchers can still be installed with:

```bash
memory watcher enable --project my-project
```

Manual foreground watchers can still be run with:

```bash
memory watcher run --project my-project
```

Treat the legacy service-managed and foreground watcher commands as advanced flows. For normal Codex-linked operation, prefer the watcher manager and use `memory status --project my-project` for a combined service, watcher, MCP, and doctor snapshot.

## Watcher Health States

Memory Layer tracks watcher health with a heartbeat and watchdog.

The main states are:

- `healthy`: the watcher is heartbeating normally
- `stale`: the watcher stopped heartbeating in time
- `restarting`: the backend requested a restart for a service-managed watcher
- `failed`: the watcher did not recover or the restart request failed

Manual watchers can become `stale`, but they are not restarted automatically.

## Where To See Watcher Health

In the TUI:

- `Watchers` tab:
  - shows the current watcher list for the project
  - shows health state, restart attempts, last restart attempt, and last heartbeat
  - shows the owning agent session when the watcher is manager-started
- `Project` tab:
  - shows the compact watcher summary
  - shows recent watcher-health transitions such as stale, restarting, failed, and recovered in the recent activity section

## Recovery Signals

When a watcher recovers after being `stale`, `restarting`, or `failed`, the TUI now makes that explicit:

- the status line shows a recovery message immediately
- the `Project` tab recent activity section shows a watcher transition row for the recovery
- the watcher and activity details include:
  - the current health
  - the previous health
  - whether the watcher is service-managed
  - the restart attempt count
  - how many restart attempts happened before recovery, when relevant

That makes it easier to tell the difference between:

- a watcher that is still unhealthy
- a watcher that restarted and recovered
- a manual watcher that went stale and needs user action

## Common Commands

Enable the watcher manager:

```bash
memory watcher manager enable
```

Check watcher manager status:

```bash
memory watcher manager status
```

The status output includes the manager mode, last reconcile reason, event count, fallback scan count, and lock owner PID. If another manager is already running, `memory watcher manager run` exits with the active PID instead of starting a duplicate.

Enable a legacy service-managed watcher:

```bash
memory watcher enable --project my-project
```

Check legacy watcher service status:

```bash
memory watcher status --project my-project
```

Disable the service-managed watcher:

```bash
memory watcher disable --project my-project
```

Run a watcher manually:

```bash
memory watcher run --project my-project
```
