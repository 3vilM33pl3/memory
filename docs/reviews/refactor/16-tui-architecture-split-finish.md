# TUI Architecture Split — Finish

## Review Basis

The 2026-05-18 audit (`AUDIT-2026-05-18.md`) found that plan `08-tui-architecture-split.md` landed cosmetically. Only `crates/mem-cli/src/tui/markdown.rs` (298 LOC) and `tui/theme.rs` (64 LOC) were extracted. `crates/mem-cli/src/tui.rs` went from 11,487 → 11,154 LOC (-333). The 50-field `App` struct remains monolithic; no per-tab state separation; no `tabs/` module; no MVU movement.

## Goal

Turn `tui.rs` from a god-file into an entrypoint by extracting tab-level state and rendering into per-tab modules. Set the stage (without enforcing yet) for an eventual model-view-update separation.

## PR Shape

One PR per tab. Behavior-preserving moves. Suggested order (small/independent first):

1. `tui/tabs/errors.rs`
2. `tui/tabs/embeddings.rs`
3. `tui/tabs/project.rs`
4. `tui/tabs/watchers.rs`
5. `tui/tabs/agents.rs`
6. `tui/tabs/activity.rs`
7. `tui/tabs/memories.rs`
8. `tui/tabs/query.rs`
9. `tui/tabs/resume.rs`
10. `tui/tabs/review.rs`
11. `tui/tabs/bundles.rs`

## Implementation Notes

- Each tab gets a sub-struct: `struct MemoriesTabState { … }`. Move the matching fields off `App` and onto the sub-struct. `App` ends up as a coordinator with per-tab sub-states + global state (focus, theme, keymap).
- Each tab module exposes (at minimum): `render(frame, area, state, &AppContext)` and `update(event, &mut state, &AppContext) -> Option<Action>`. This is the seed of MVU without forcing the whole TUI into it in one go.
- Reuse `tui/markdown.rs` and `tui/theme.rs` from plan `08`.
- Keep keymap handling in `tui.rs` for now; tabs declare which keys they consume.
- Do not change rendered output, keybindings, or tab order.

## Tests

- `cargo test -p mem-cli --all-targets --locked` after each PR.
- `cargo clippy -p mem-cli --all-targets --locked -- -D warnings`.
- Manual smoke: open the TUI, cycle every tab, exercise one keybinding per tab.
- Optional: add headless render-snapshot tests for each tab using `ratatui::backend::TestBackend` (cheap once tabs own their state).

## Acceptance Criteria

- **LOC budget**: `crates/mem-cli/src/tui.rs` ≤ **3,000 LOC** after the pack completes (down from 11,154).
- `struct App` has at most ~10 direct fields; the rest live on per-tab sub-structs.
- Each tab is locatable as `crates/mem-cli/src/tui/tabs/<name>.rs`.
- No visible TUI behavior changes.
