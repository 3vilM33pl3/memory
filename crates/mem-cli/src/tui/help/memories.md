# Memories Help

## Purpose
Browse canonical project memory, inspect one entry in detail, and maintain durable knowledge.

## Layout
- Left table: filtered memories with summary, type, status, confidence, and update time.
- Right detail: validation proof, replacement diff, canonical text, embeddings, tags, sources, history, and related memories.
- Focus indicator: shows whether movement keys select memories or scroll detail.

## Controls
- `j/k` or `Up/Down`: select memories or scroll detail when detail focus is active.
- `Enter`: toggle list/detail focus. `Esc`: return to list focus.
- `PgUp/PgDn`, `Home`, `End`: scroll or jump detail.
- `/`: text filter. `g`: tag filter. `s`: status filter. `t`: type filter. `x`: clear filters.
- `v`: validate selected memory with proof search. `y`: apply the visible validation preview. `n`: dismiss it.
- `c`: curate. `i`: reindex chunks. `e`: re-embed active space. `a`: archive low-value memories. `Shift+D`: delete. `Shift+H`: history.

## Workflows
- Filter by type or text, select a memory, then read canonical text and sources.
- Use `v` to search for codebase proof; suggested replacements appear as a diff and do not apply until `y`.
- Use curation and Review rather than creating duplicate memories.

## Troubleshooting
- If detail is empty, move selection or refresh project state.
- If embeddings are missing, use `e` here or the Embeddings tab.
