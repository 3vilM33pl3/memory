# Query Help

## Purpose
Ask questions against project memory and inspect the evidence, citations, timings, and graph connections behind the answer.

## Layout
- Question box: current or last submitted question.
- Query Result: answer, confidence, citations, evidence state, match count, and timing breakdown.
- Results/detail: ranked memories and why the selected memory matched.

## Controls
- `Enter`: start a new empty question from Query.
- `?`: jump to Query and start a question from anywhere.
- While editing: `Enter` submits, `Esc` cancels, `Up/Down` restores cached query history.
- `j/k`: move through results. `Shift+D`: delete selected result memory.

## Workflows
- Compare answer citations with numbered returned memories before trusting an answer.
- Use timing breakdown to locate slow lexical, semantic, graph, rerank, answer, or UI phases.
- Treat graph connections as retrieval explanations; citations still point to memories.

## Troubleshooting
- If evidence is insufficient, add or curate memory and ask again.
- If a restored history item is stale, press `Enter` to re-run it.
