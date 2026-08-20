# Automations Help

## Purpose
Inspect loop-engineering automations, effective settings, recent run state, and pending approval requests from the terminal.

## Layout
- Loop table: loop id, effective mode, scope, latest run status, and pending approval count.
- Detail pane: selected loop description, risk, effective settings, latest run, pending approvals, and load warnings.
- Global line: kill-switch state when the service reports it.

## Controls
- `j/k` or `Up/Down`: select an automation.
- `PgUp/PgDn`: scroll the selected automation detail. `Home`: jump to top.
- `r`: refresh project state outside help.

## Workflows
- Open this tab to see which built-in loops are registered and whether they are off, observing, suggesting, or blocked.
- Check pending approvals before allowing higher-risk loop actions to continue.
- Use the browser UI or `memory loops ...` commands for mutating controls.

## Troubleshooting
- If the tab is empty, verify the service registered loop definitions with `memory loops list`.
- If settings or runs show warnings, refresh once and then inspect `memory loops show <loop_id>` or `memory loops runs --project <project>`.
