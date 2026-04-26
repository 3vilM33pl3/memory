# Knowledge Graph Plan

This plan describes how Memory Layer should grow from the current
`memory_relations` table into a first-class, provenance-backed knowledge graph.
The graph work should be additive: existing memory APIs, query behavior, and TUI
views must keep working while graph-specific storage and retrieval are added
beside them.

## Summary

Memory Layer should build the graph in layers:

1. Memory graph
2. Entity and claim graph
3. Code graph
4. Evidence layer
5. Graph-assisted retrieval

The guiding rule is simple: every graph node and edge must be explainable from
evidence. LLMs may propose graph facts later, but the system should not treat
model output as authoritative without provenance, confidence, and a named
strategy version.

## Current State

The application already stores a lightweight relation graph between memories in
`memory_relations`. Those rows link one memory to another with relation types
such as `duplicates`, `supersedes`, `depends_on`, `supports`, and `related_to`.

That is useful, but it is not yet a full knowledge graph:

- There are no dedicated `graph_nodes` or `graph_edges` tables.
- Code symbols are extracted by `mem-analyze`, but not persisted as graph nodes.
- Query uses `memory_relations` as a reranking signal, not as a graph traversal
  engine.
- Memory detail reads precomputed related memories; it does not infer graph
  structure at read time.

## Target Model

### Memory Graph

Start by representing current curated memories as graph nodes and mirroring
selected `memory_relations` rows into graph edges.

This gives the project a safe first graph because it reuses facts the system
already computes. Existing memory reads should remain unchanged until graph
parity is tested.

### Entity And Claim Graph

Extract stable entities and claims from curated memories.

Entities are durable things the system can refer to, for example:

- repositories
- crates, modules, packages, and services
- commands and config keys
- protocols and external systems
- domain concepts

Claims are structured assertions about those entities, for example:

- "The watcher manager starts one watcher per agent session."
- "`memory query` combines lexical and semantic retrieval."
- "The service owns curation, retrieval, and streaming updates."

Claims should link back to the memory version, capture, source file, or task
that introduced them.

### Code Graph

Use existing `mem-analyze` output as the seed for code graph extraction.

Initial graph objects:

- symbols become `code_symbol` nodes
- imports become `imports` edges
- calls become `calls` edges
- references become `mentions` or `references` edges
- test links become `tested_by` edges

Code symbol identity should include project, language, file path, symbol kind,
qualified name when available, and source span when available.

### Evidence Layer

Facts and evidence must be separate.

Every node and edge should carry or link to:

- memory id and version when derived from memory
- source file path and source span when derived from code
- git commit when available
- task or capture id when available
- extraction strategy name and version
- confidence score

This makes graph output auditable and allows future recuration without losing
the reason a fact was created.

### Retrieval Layer

Do not make the graph the primary query path at first.

Use it as an additive retrieval signal:

- expand query candidates through nearby graph nodes
- boost memories connected by strong graph evidence
- expose graph-derived explanations in diagnostics
- keep lexical and semantic search as the baseline

Only promote graph-backed query behavior after parity tests show it improves
retrieval without reducing answer grounding.

## Implementation Sequence

1. Add graph storage beside existing memory storage.
   Create tables for `graph_nodes`, `graph_edges`, `graph_evidence`,
   `graph_extraction_runs`, `code_symbols`, and `code_references`.

2. Add repository interfaces.
   Introduce graph-facing repository traits and PostgreSQL implementations so
   extraction and evaluation code does not depend directly on SQL details.

3. Backfill memory graph nodes.
   Create graph nodes for active latest non-tombstone memories only. Preserve
   compatibility with the existing memory version model.

4. Mirror memory relations.
   Copy selected `memory_relations` rows into graph edges while keeping
   `memory_relations` as the compatibility surface for current query and TUI
   behavior.

5. Persist code graph facts.
   Convert `mem-analyze` symbols, imports, calls, references, and test links
   into graph-backed code objects with source-span evidence.

6. Add graph-aware retrieval.
   Use graph neighbors as an optional reranking and explanation signal. Keep it
   disabled or additive until evaluation fixtures prove value.

7. Add graph inspection surfaces.
   Add CLI or TUI views only after storage, extraction, and retrieval behavior
   are stable enough to explain clearly.

## Evaluation

Graph work should ship with fixtures before it becomes default behavior.

Initial fixture groups:

- memory-to-memory relation mirroring
- entity extraction from curated memories
- claim extraction from curated memories
- Rust and TypeScript code symbol extraction
- imports, calls, references, and test-link extraction
- graph-assisted retrieval recall
- answer citation quality with graph-expanded evidence

Recommended metrics:

- extraction precision and recall
- edge type accuracy
- exact and partial symbol identity match
- retrieval recall at K
- answer citation coverage
- false-positive graph expansion rate

The default evaluation profile should not require network calls. Model-backed
claim extraction can be tested in an optional profile.

## Compatibility Rules

- Do not remove `memory_relations` until graph-backed query and TUI behavior has
  parity coverage.
- Do not make graph extraction mandatory for `memory query`.
- Do not write graph facts without provenance.
- Do not treat LLM output as authoritative without confidence, evidence, and
  strategy metadata.
- Do not mix prose memory identity with code symbol identity. Link them through
  evidence and edges.

## Implemented First Slice

The first slice now provides a code graph baseline:

- Graph table migrations exist beside current memory tables.
- `mem-analyze` emits stable symbol identities and conservative resolved
  references.
- `mem-graph` persists extraction runs, code symbols, code references, graph
  nodes, graph edges, and evidence.
- `memory graph extract` and `memory graph status` expose the workflow.
- Current memory query and TUI behavior remain unchanged by default.

## Next Slices

The next knowledge graph work should build on this baseline:

1. Backfill active latest memories as graph nodes.
2. Mirror selected `memory_relations` rows into graph edges.
3. Add graph-aware retrieval as an optional reranking signal.
4. Add graph inspection surfaces in the TUI after storage and retrieval parity
   tests exist.
