# Core Concepts Section

## Purpose

Explain the mental model behind Memory Layer so users understand why the system is more than vector search.

## Section Navigation

```text
Concepts
  Mental model
  Projects
  Memories
  Evidence
  Curation
  Retrieval
  Embeddings
  Code graph
  Activity events
  Trust and staleness
```

## Concepts Overview Page

Start with a simple flow:

```text
Capture → Curate → Store → Retrieve → Verify
```

Suggested copy:

> Memory Layer turns project activity into durable memories. Memories are useful because they are scoped to projects, linked to evidence, curated over time, and retrievable by agents when a future task needs context.

## Mental Model Page

Explain:

- A **project** is the unit of context.
- A **memory** is a durable claim or useful fact about the project.
- **Evidence** is what makes a memory trustworthy.
- **Curation** keeps memories useful as projects change.
- **Retrieval** selects relevant memories for humans or agents.
- **Evaluation** measures whether memory improves outcomes.

Suggested diagram:

```text
Work happens
  ↓
Events, commits, scans, notes
  ↓
Candidate memories
  ↓
Human/agent curation
  ↓
Project memory store
  ↓
Evidence-backed retrieval
```

## Projects Page

Cover project slug, project config, relationship to a repository, multi-project usage, shared backend with multiple project configs, and what happens when the same repo is moved.

## Memories Page

Cover what should become a memory and what should not.

Good memory:

> The web UI expects the API server on port 8787 during local development unless `MEMORY_API_URL` is set.

Bad memory:

> I am currently looking at the README.

## Evidence Page

This is central to the product. Cover why Memory Layer prefers cited context and the types of evidence it can use: memory records, commits, activity events, scan results, code graph references, and evaluation artifacts.

## Curation Page

Explain manual curation, automatic candidates, replacement proposals, approving/rejecting changes, stale memories, and preserving historical context without polluting current retrieval.

Suggested framing:

> Curation is how Memory Layer avoids becoming a junk drawer.

## Retrieval Page

Cover keyword search, vector search, multi-embedding search, code graph-aware retrieval, ranking, filtering by project, freshness/staleness, and evidence display.

## Embeddings Page

Cover why embeddings are optional but useful, provider choices, multi-embedding spaces, switching active retrieval backends, recomputing vs preserving embeddings, and local-compatible endpoints.

## Code Graph Page

Cover parser-backed symbols, file references, symbol references, graph edges, and why code graph-aware retrieval helps coding agents.

## Activity Events Page

Cover what activity events are, how watchers produce them, how they support briefings, how they differ from memories, and how they can become candidate memories.

## Trust and Staleness Page

Cover stale memories, current evidence-backed context, replacement proposals, agent verification before code changes, and why the docs should avoid promising perfect correctness.
