# Review Tab

**Review** is the human gate for replacement proposals created by curation. It lets you compare the existing target with the candidate before canonical memory changes.

![Pending replacement proposals in the TUI](https://www.memory-layer.dev/images/tui/review-tab.png)

The list shows target, candidate, and similarity score. The detail pane explains matching reasons and shows the candidate text. Approval creates a new immutable version; rejection leaves the existing memory unchanged.

## Controls

| Key | Action |
| --- | --- |
| `j` / `k`, arrows, `[` / `]` | Move through proposals |
| `PgUp` / `PgDn`, `Home` / `End` | Move quickly through the queue |
| `y` | Approve the selected proposal |
| `n` | Reject it |
| `p` | Cycle the replacement policy for future proposals |
| `r` | Refresh |
| `h` | Open help |

Use Review after curation or a meaningful `memory remember` run. Do not approve a proposal merely because its score is high: the candidate still needs current, better-supported evidence.

See also: [Capture and curation](https://www.memory-layer.dev/docs/reference/cli/capture-curation).
