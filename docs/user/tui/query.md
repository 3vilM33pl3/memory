# Query Tab

Use the `Query` tab to ask a scoped question against project memory and inspect the ranked results without leaving the TUI.
The tab shows both the synthesized answer and the evidence trail used to create it.

![Query tab](../../img/tui/query-tab.png)

## What It Shows

- a focused question box with a visible cursor while editing
- the answer, confidence, generation method, citation numbers, and diagnostics
- an explicit timing breakdown for the completed query roundtrip
- the returned matching memories with citation numbers that map to the answer
- the selected result in more detail, including whether it was cited and why it ranked well
- graph diagnostics and graph connections when a completed code graph extraction is available

When LLM answering is configured, the backend answers using only the returned memories. Supported providers are `openai_compatible` and `ollama`; Ollama uses the local OpenAI-compatible endpoint without an API key by default. If the LLM is unavailable or returns invalid citations, the tab shows the deterministic fallback answer and the fallback reason.
After you press `Enter` while editing, the query runs in the background and the tab shows a searching state until the new answer arrives. Previous results remain visible during the search so you can keep reading while waiting.
Query history is session-local. When you use `Up` or `Down` while editing, the TUI restores both the previous question text and the cached answer, timing, returned memories, selected-memory detail, or error for that history item. Restoring a history item does not re-run the query; press `Enter` to refresh it.

If graph data exists for the project, the tab shows graph status, graph candidate counts, graph timing, and per-result graph connections in the detail pane. These explain which file or symbol helped retrieve a memory; the answer still cites the returned memories rather than raw graph rows.

## Timing Breakdown

After a query completes, the `Query Result` panel explains where the request spent time:

- `UI ready` is the full TUI wait until results and the first selected memory detail are ready to render.
- `Query API` is only the `/v1/query` request time.
- `Initial detail` is the follow-up request for the first returned memory detail.
- `Backend` is the backend-reported retrieval time plus answer synthesis time.
- `Retrieval` is the backend memory search total before answer synthesis.
- `Answer` is the answer synthesis step; this is usually the LLM call when LLM answering is enabled, or the deterministic/fallback synthesis otherwise.
- `Overhead` is client/API time not explained by backend-reported work, such as HTTP transport, JSON serialization, scheduling, and notification bookkeeping.
- `Lexical`, `Semantic`, `Graph`, and `Rerank/relation` are the retrieval phases inside the backend.
- `Other` is retrieval work not covered by the named phases, such as fetching result sources and assembling the final response.

Percentages are relative to `UI ready` for the top-level rows and relative to `Retrieval` for retrieval phases. Phase totals can differ slightly from the roundtrip because the backend and TUI measure different boundaries.

You can jump into query mode from anywhere in the TUI with `?`.

## Key Controls

- `Enter` on the `Query` tab starts a new empty question
- `?` switches to the `Query` tab and starts a new empty question
- type your question and press `Enter` to run it
- `Up/Down` while editing walks through previous queries from this TUI session and restores their cached results
- `Esc` cancel query input
- `j/k` move through returned results
- `r` refresh project state after backend changes
- `h` open or close detailed help for this tab

## When To Use It

- asking "how does this project do X?"
- checking whether a detail was already captured in memory
- exploring retrieved evidence after a fresh `remember`, `scan`, or commit import

## See Also

- [TUI Guide](README.md)
- [Scan Command](../cli/scan.md)
