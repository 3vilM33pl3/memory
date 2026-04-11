# `memory query`

`memory query` asks a project-specific question against curated Memory Layer data.

Use it when you want a direct answer from durable project memory instead of browsing the TUI.

## Common Usage

```bash
memory query --project memory --question "How does resume work?"
memory query --project memory --question "What changed recently?" --type plan
memory query --project memory --question "What was actually implemented for the watcher manager?" --type implementation
memory query --project memory --question "What are the watcher health states?" --tag watcher
```

## Useful Flags

- `--type` restricts retrieval to one or more memory types
- `--tag` restricts retrieval to one or more tags
- `--limit` caps how many memories are considered before answer synthesis
- `--min-confidence` filters out weaker memories
- `--json` returns the full result payload

## Related Docs

- [Resume Briefings](resume.md)
- [Remember Command](remember.md)
- [TUI Query Tab](../tui/query.md)
- [Memory Types Reference](../../developer/architecture/memory-types.md)
