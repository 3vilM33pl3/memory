# Query Tab

Use the `Query` tab to ask a scoped question against project memory and inspect the ranked results without leaving the TUI.
The tab shows both the synthesized answer and the evidence trail used to create it.

![Query tab](../../img/tui/query-tab.png)

## What It Shows

- the current question in the controls row
- the answer, confidence, generation method, citation numbers, and diagnostics
- the returned matching memories with citation numbers that map to the answer
- the selected result in more detail, including whether it was cited and why it ranked well

When LLM answering is configured, the backend answers using only the returned memories. If the LLM is unavailable or returns invalid citations, the tab shows the deterministic fallback answer and the fallback reason.

You can jump into query mode from anywhere in the TUI with `?`.

## Key Controls

- `?` switch to the `Query` tab and start editing a question
- type your question and press `Enter` to run it
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
