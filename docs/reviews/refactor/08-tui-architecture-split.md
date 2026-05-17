# TUI Architecture Split

## Review Basis

Claude flagged `crates/mem-cli/src/tui.rs` as a large file with a broad `App` state struct. Current `main` does have in-TUI help, so the missing-help finding is stale, but the architecture size issue remains.

## Goal

Split the TUI into reviewable state, event, rendering, API, and help modules without changing user-visible behavior.

## PR Shape

Make this a series of behavior-preserving refactors. Move one layer at a time and keep tests passing after each move.

## Implementation Notes

- Start by moving pure rendering helpers and markdown/help rendering into modules.
- Next move tab-specific state and render functions into `tabs/*`.
- Keep `App` as the central state initially; split it only after tab modules expose clear boundaries.
- Preserve keybindings and existing help text.
- Avoid a full MVU rewrite until the module split has stabilized.

## Tests

- Run `cargo test -p mem-cli --all-targets --locked`.
- Preserve existing TUI tests for help, tab ordering, query history, footer status, and selection behavior.
- Add focused tests only when a moved helper has no coverage.

## Acceptance Criteria

- TUI changes can be reviewed by tab or subsystem.
- `tui.rs` becomes an entrypoint/orchestrator instead of the only implementation file.
- No keyboard, layout, or API behavior changes in the refactor PRs.
