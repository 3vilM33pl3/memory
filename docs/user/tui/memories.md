# Memories Tab

The `Memories` tab is the main browsing view for canonical project memory.

![Memories tab](../../img/tui/memories-tab.png)

## What It Shows

- a filterable memory list on the left
- the selected memory entry in detail on the right
- summary, canonical text, type, confidence, tags, sources, and related memories

This is the best tab for reading what Memory Layer already knows about a project.

## Related Memories

The `Related memories` section is a navigation aid, not a hand-curated truth table.

Those links are computed automatically during curation from strong text overlap, shared tags, shared provenance file paths, and explicit dependency or supersession language. They are useful for finding nearby context, but they are still heuristic.

## Key Controls

- `j/k` move through the memory list
- `PgUp/PgDn` scroll the selected memory detail
- `Home` jump the detail pane back to the top
- `/` edit the text filter
- `g` edit the tag filter
- `s` cycle status filters
- `t` cycle memory-type filters
- `x` clear all active filters
- `c` run curation
- `i` reindex memory chunks
- `e` re-embed the active embedding space
- `a` archive low-value memories
- `Shift+D` delete the selected memory

## When To Use It

- browsing architecture or workflow knowledge already in the system
- checking whether a fact is already stored before adding more memory
- inspecting provenance on an existing memory entry
- doing maintenance work such as curate, reindex, or re-embed

## See Also

- [Remember Command](../cli/remember.md)
- [Embedding Operations](../cli/embeddings.md)
- [TUI Guide](README.md)
