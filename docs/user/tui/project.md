# Project Tab

The `Project` tab is the high-level health and maintenance dashboard for the current project.

![Project tab](../../img/tui/project-tab.png)

## What It Shows

- project-wide counts for memories, captures, sessions, and curation runs
- the latest approved plan memory, including its `plan-thread` key when present
- recent-memory timing and confidence breakdowns
- embedding and automation status
- Memory CLI/service/watcher versions and repo-local Memory skill-bundle status
- top tags and top files
- replacement-policy status and pending-proposal count (the review queue itself lives in the [Review](review.md) tab)
- a compact watcher summary
- recent operational activity such as queries, captures, curation, plan lifecycle events, and watcher-health transitions

This tab is where you look when the question is about project state rather than one specific memory.

## Key Controls

- `j/k` scroll the tab
- `PgUp/PgDn` page through longer project summaries
- `Home` jump back to the top

## When To Use It

- checking whether the project is healthy
- reviewing embedding coverage or automation state
- checking whether project-local Memory skills need `memory upgrade`
- understanding top files, tags, or memory-type distribution
- confirming what the backend or TUI did recently without opening a separate activity view

For approving or rejecting queued memory replacements, use the [Review Tab](review.md).

## See Also

- [Review Tab](review.md)
- [Activity Events](activity.md)
- [Embedding Operations](../cli/embeddings.md)
- [Watcher Health](../cli/watchers.md)
- [TUI Guide](README.md)
