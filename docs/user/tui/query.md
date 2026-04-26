# Query Tab

Use the `Query` tab to ask a scoped question against project memory and inspect the ranked results without leaving the TUI.
The tab shows both the synthesized answer and the evidence trail used to create it.

![Query tab](../../img/tui/query-tab.png)

## What It Shows

- a focused question box with a visible cursor while editing
- the answer, confidence, generation method, citation numbers, and diagnostics
- the returned matching memories with citation numbers that map to the answer
- the selected result in more detail, including whether it was cited and why it ranked well
- graph diagnostics and graph connections when a completed code graph extraction is available

When LLM answering is configured, the backend answers using only the returned memories. If the LLM is unavailable or returns invalid citations, the tab shows the deterministic fallback answer and the fallback reason.
After you press `Enter` while editing, the query runs in the background and the tab shows a searching state until the new answer arrives. Previous results remain visible during the search so you can keep reading while waiting.

If graph data exists for the project, the tab shows graph status, graph candidate counts, graph timing, and per-result graph connections in the detail pane. These explain which file or symbol helped retrieve a memory; the answer still cites the returned memories rather than raw graph rows.

You can jump into query mode from anywhere in the TUI with `?`.

## Key Controls

- `Enter` on the `Query` tab starts a new empty question
- `?` switches to the `Query` tab and starts a new empty question
- type your question and press `Enter` to run it
- `Up/Down` while editing walks through previous queries from this TUI session
- `Esc` cancel query input
- `j/k` move through returned results
- `r` refresh project state after backend changes

## When To Use It

- asking "how does this project do X?"
- checking whether a detail was already captured in memory
- exploring retrieved evidence after a fresh `remember`, `scan`, or commit import

## See Also

- [TUI Guide](README.md)
- [Scan Command](../cli/scan.md)
