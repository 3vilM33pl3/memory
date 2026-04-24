# `memory query`

`memory query` asks a project-specific question against curated Memory Layer data.

Use it when you want a direct answer from durable project memory instead of browsing the TUI.
The answer is synthesized from the returned memories and includes citation numbers that map back to the ranked results.

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

## Output

The default text output shows:

- the synthesized answer
- confidence and whether evidence was sufficient
- the answer-generation method (`llm`, `deterministic`, or `fallback`)
- cited memory numbers, matching the ranked result list
- retrieval diagnostics and provenance highlights

If LLM answering is configured, the backend asks the model to answer using only the returned memories. If the model is unavailable or returns invalid citations, Memory Layer falls back to deterministic summary synthesis and reports the fallback reason.

## Related Docs

- [Resume Briefings](resume.md)
- [Remember Command](remember.md)
- [TUI Query Tab](../tui/query.md)
- [Memory Types Reference](../../developer/architecture/memory-types.md)
