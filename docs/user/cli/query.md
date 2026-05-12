# `memory query`

`memory query` asks a project-specific question against curated Memory Layer data.

Use it when you want a direct answer from durable project memory instead of browsing the TUI.
The answer is synthesized from the returned memories and includes citation numbers that map back to the ranked results.

## Common Usage

```bash
memory query --project memory --question "How does resume work?"
memory query --project memory --question "What changed recently?" --type plan
memory query --project memory --question "What was actually implemented for the watcher manager?" --type implementation
memory query --project memory --question "How is PostgreSQL setup documented?" --type documentation
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

If LLM answering is configured, the backend asks the model to answer using only the returned memories. Supported LLM providers are `openai_compatible` and `ollama`; Ollama uses `http://127.0.0.1:11434/v1` and no API key by default. If the model is unavailable or returns invalid citations, Memory Layer falls back to deterministic summary synthesis and reports the fallback reason.

## Graph-Aware Retrieval

When a project has a completed `memory graph extract` run, `memory query` automatically uses the latest completed code graph as an additional retrieval signal. No extra flag is needed.

Graph retrieval is additive:

- lexical and semantic matches still determine the baseline result set
- graph matches can add memories whose file provenance points at matching symbols or one-hop related symbols
- graph boosts are capped so code graph hints cannot overwhelm strong memory matches
- graph connections are shown as explanations, not as standalone answer citations

The JSON output includes graph diagnostics such as `graph_status`, `graph_candidates`, `graph_augmented_candidates`, and `graph_duration_ms`. Individual results can include `graph_connections` describing the file, symbol, edge, neighbor symbol, reason, and score boost that affected ranking.

## Related Docs

- [Resume Briefings](resume.md)
- [Remember Command](remember.md)
- [Graph Command](graph.md)
- [TUI Query Tab](../tui/query.md)
- [Memory Types Reference](../../developer/architecture/memory-types.md)
