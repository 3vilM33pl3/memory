# Errors Help

## Purpose
Inspect backend diagnostics and session-local TUI errors with explanations and suggested fixes.

## Layout
- Error table: time, severity, source, component, and summary.
- Detail pane: explanation, fix hints, command suggestions, and raw metadata.
- Sources include TUI, service, watcher, manager, database, and providers.

## Controls
- `j/k` or `Up/Down`: select an error.
- `PgUp/PgDn`: scroll detail. `Home`: jump to top.
- `r`: refresh diagnostics.

## Workflows
- Open this tab when the footer shows warnings/errors or an operation fails.
- Prefer suggested `memory doctor` commands when shown.
- Use source/component to route fixes to config, service, watcher, manager, provider, or database.

## Troubleshooting
- If the table is empty but the footer is red, refresh and check live connection state.
- If provider errors repeat, verify API keys and backend readiness.
