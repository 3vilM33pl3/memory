# Graph And Curation Foundations

This document defines the baseline direction for Memory Layer's future
knowledge graph, code structure graph, specialized curation, and code-analysis
work.

It is intentionally additive. Existing memory APIs, `memory_entries`, and
`memory_relations` remain compatible while graph-specific storage and curation
interfaces are introduced beside them.

## Goals

- Represent durable knowledge as typed graph objects with provenance.
- Represent code structure separately from prose memory.
- Keep every graph edge explainable through evidence.
- Support research-backed curation strategies without hard-wiring one model or
  one prompt into the core write path.
- Make extraction and retrieval quality measurable through fixtures.

## Core Concepts

### Entity

An entity is a stable thing the system can refer to across memories and code.

Examples:

- repository
- crate/module/package
- type/function/method
- config key
- service/process
- external protocol or domain concept

Entities must have:

- stable project scope
- kind
- canonical name
- optional aliases
- provenance for why the entity exists

### Claim

A claim is an asserted fact about an entity or relationship.

Examples:

- “`memory service` owns curation and retrieval.”
- “`memory watcher manager` starts one watcher per Codex session.”
- “`fetch_project_memories` returns latest non-tombstone canonical memories.”

Claims must have:

- canonical text
- confidence
- source evidence
- originating memory or capture where available
- lifecycle state

Claims are not replacements for memories. A curated memory can produce one or
more claims, and a claim can link back to the memory version that introduced it.

### Code Symbol

A code symbol is a graph entity extracted from source.

Minimum symbol identity:

- project
- language
- file path
- symbol kind
- qualified name when available
- source span when available

Code symbol extraction should start from parser-backed analyzers and keep
heuristics isolated behind extractor implementations.

### Edge

An edge is a typed relationship between two graph nodes.

Initial edge families:

- semantic: `supports`, `contradicts`, `supersedes`, `duplicates`, `related_to`
- dependency: `depends_on`, `calls`, `imports`, `configures`, `owns`
- provenance: `derived_from`, `mentioned_in`, `implemented_by`, `tested_by`
- scope: `belongs_to`, `part_of`

Every edge must carry:

- type
- source node
- destination node
- confidence
- evidence reference
- producer strategy
- created version or extraction run

Directional edges stay directional. Symmetric relationships should either be
stored as explicitly symmetric edge types or mirrored by the graph repository,
not inferred ad hoc by every reader.

## Storage Direction

The current `memory_relations` table is a memory-to-memory relation table. It
should remain as the compatibility surface for current query and TUI behavior.

Future graph storage should be added beside it:

- `graph_nodes`
- `graph_edges`
- `graph_evidence`
- `code_symbols`
- `code_references`
- `graph_extraction_runs`

Recommended migration path:

1. Add graph tables without changing existing memory reads.
2. Backfill graph nodes for active latest memories.
3. Mirror selected `memory_relations` rows into graph edges.
4. Add code symbol extraction from existing analyzers.
5. Add graph-aware retrieval as an optional reranking signal.
6. Promote graph-backed views only after parity tests exist.

Default reads must keep using latest non-tombstone memory versions unless a
history-aware API explicitly asks for older versions.

## Repository Interfaces

Introduce graph-facing repository traits before wiring them into curation.

Minimum interfaces:

- `GraphNodeRepository`
- `GraphEdgeRepository`
- `GraphEvidenceRepository`
- `CodeSymbolRepository`
- `GraphExtractionRunRepository`

Repository methods should accept project scope explicitly and avoid reaching
through global config. This keeps future batch jobs, service handlers, and CLI
commands testable.

The first implementation can live on PostgreSQL. The trait boundary exists to
make extraction and evaluation code independent from SQL details, not to support
multiple databases on day one.

## Curation Strategy Model

Current curation has deterministic heuristics for type and relation inference.
That remains useful as a baseline, but future specialized curation should use
explicit strategies.

Strategy inputs:

- raw capture payload
- candidate memory
- latest project memories
- code graph neighborhood
- existing graph claims and edges
- configured research/model profile

Strategy outputs:

- accepted/rejected candidate decisions
- replacement proposals
- claims
- graph edges
- confidence and rationale
- evaluation labels where available

Strategies must be named and versioned so stored graph facts can record which
producer created them. A future change in prompt, paper-backed algorithm, model,
or threshold should produce a new strategy version.

## Evaluation Fixtures

Future graph and curation changes must be evaluated against stable fixtures
before becoming default.

Initial fixture sets:

- memory extraction from task captures
- memory type classification
- replacement and tombstone decisions
- memory-to-memory relation detection
- code symbol extraction
- code reference extraction
- graph-assisted retrieval
- answer grounding and citation quality

Each fixture should include:

- input corpus
- expected extracted objects
- expected rejected objects where relevant
- provenance expectations
- scoring metric

Recommended metrics:

- precision and recall for extraction
- exact and partial match for symbol identity
- edge type accuracy
- replacement decision accuracy
- retrieval recall at K
- answer citation coverage

The evaluation harness should run without external network calls by default.
Model-backed evaluations can be a separate optional profile.

## Compatibility Rules

- Do not remove `memory_relations` until graph-backed query/TUI behavior has
  parity coverage.
- Do not make graph extraction mandatory for `memory query`.
- Do not write graph facts without provenance.
- Do not treat LLM output as authoritative without confidence, evidence, and
  strategy metadata.
- Do not mix code symbol identity with prose memory identity; link them through
  evidence and edges.

## First Implementation Slice

The first implementation issue should do only this:

1. Add graph table migrations.
2. Add repository traits and PostgreSQL implementations.
3. Add fixtures for a tiny Rust project and a tiny TypeScript project.
4. Add a read-only code symbol extraction command or test harness.
5. Keep all existing memory query and TUI behavior unchanged.
