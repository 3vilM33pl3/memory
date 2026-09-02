# Memories Tab

Use **Memories** to browse the canonical project memory rather than relying on a summary alone.

![Memory list and detail panes in the TUI](https://www.memory-layer.dev/images/tui/memories-tab.png)

The left pane lists memories; the detail pane shows canonical text, type, confidence, tags, provenance, related memories, validation state, and any proposed replacement. Related memories are navigation hints computed from overlap, tags, provenance, and relationships, not a separate source of truth.

## Controls

- `j` / `k` select a memory.
- `/` edits the text filter; `g` edits the tag filter.
- `s` cycles status filters and `t` cycles memory types.
- `x` clears filters.
- `PgUp` / `PgDn` and `Home` scroll detail.
- `c` runs curation; `i` reindexes chunks; `e` re-embeds the active space.
- `v` previews evidence-backed validation for the selected memory. `y` applies the visible preview as a new version; `n` dismisses it.
- `a` archives low-value memories and `Shift+D` deletes the selected memory.

Use this tab before adding a duplicate, when inspecting source evidence, or when reviewing a fact that may be stale.

See also: [Remember](https://www.memory-layer.dev/docs/reference/cli/capture-curation) and [Embeddings](https://www.memory-layer.dev/docs/tui/embeddings).
