# Automations Tab

Use the `Automations` tab to inspect loop-engineering automation state from the
terminal.

## What It Shows

- registered loop definitions
- effective mode and scope for the current project/repo
- latest run status for each loop
- pending approval counts
- selected loop description, risk level, default mode, effective settings,
  latest run detail, pending approvals, and load warnings
- global kill-switch state when reported by the service

The TUI tab is read-oriented. Use the browser UI or `memory loops ...` commands
for mode changes, manual runs, approval decisions, or global kill-switch changes.

## Key Controls

- `j/k` move the selected automation
- `PgUp/PgDn` scroll the selected automation detail
- `Home` jump the detail pane back to the top
- `r` refresh the project snapshot
- `h` open or close detailed help for this tab

## When To Use It

- confirming which loop automations are registered
- checking whether a loop is off, observing, suggesting, paused, snoozed, or blocked
- seeing whether an automation has pending approval requests
- checking the most recent loop run without leaving the terminal

## See Also

- [`memory loops`](../cli/loops.md)
- [Browser UI Automations](../web-ui.md#automations)
- [TUI Guide](README.md)
