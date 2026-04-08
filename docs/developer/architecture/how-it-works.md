# How Memory Layer Works

This page explains the current implementation of Memory Layer in detail. It is written for a senior developer who wants to understand the runtime model, data model, control flow, and the tradeoffs behind the design.

If you want installation instructions instead, use [Getting Started](../../user/getting-started.md).

## Table of Contents

- [Mental Model](#mental-model)
- [Workspace Layout](#workspace-layout)
- [Configuration Resolution](#configuration-resolution)
- [Repository Bootstrap](#repository-bootstrap)
- [Runtime Topology](#runtime-topology)
- [Writer Identity](#writer-identity)
- [Core Service Boundaries](#core-service-boundaries)
- [Database Model](#database-model)
- [Write Path: Explicit Remember](#write-path-explicit-remember)
- [Write Path: Raw Capture And Curation Separation](#write-path-raw-capture-and-curation-separation)
- [Capture Payload Ingestion](#capture-payload-ingestion)
- [Curation Pipeline](#curation-pipeline)
- [Search And Query Pipeline](#search-and-query-pipeline)
- [TUI And Streaming Updates](#tui-and-streaming-updates)
- [Watcher Model](#watcher-model)
- [Scan Flow](#scan-flow)
- [Activity And Observability](#activity-and-observability)
- [Commit History](#commit-history)
- [Why The Design Looks Like This](#why-the-design-looks-like-this)
- [Where To Read Next](#where-to-read-next)

## Mental Model

Memory Layer is a project-scoped memory system for coding agents and the developers working with them.

At a high level it does three things:

1. capture raw evidence about work that happened
2. turn some of that evidence into curated durable memory
3. make the curated memory searchable and inspectable later

The important design choice is the split between raw capture and curated memory.

Raw capture is intentionally messy and close to the original task:

- task title
- user prompt
- agent summary
- files changed
- tests run
- notes
- command output
- optional structured candidates

Curated memory is intentionally smaller and more durable:

- a canonical statement
- a short summary
- a memory type
- tags
- confidence and importance
- provenance back to the task or files that produced it

That split is what keeps the system auditable. Memory Layer does not pretend that every stored fact appeared fully formed. It keeps the raw source material, then derives canonical memory from it.

## Workspace Layout

The repository is a Rust workspace with these crates:

- `crates/mem-api`
  Shared DTOs, config loading, enums, and transport message types.
- `crates/mem-cli`
  The user-facing CLI, TUI, wizard, doctor, scan flow, and repo bootstrap logic.
- `crates/mem-service`
  The backend service. It owns HTTP routes, the Cap'n Proto streaming transport, migrations on startup, and orchestration of capture/query/curate operations.
- `crates/mem-ingest`
  Normalization of captured task payloads into candidate facts.
- `crates/mem-curate`
  Deterministic curation, dedupe, canonicalization, and provenance assembly.
- `crates/mem-search`
  Retrieval, ranking, and answer synthesis over PostgreSQL-backed memory.
- `crates/mem-watch`
  The optional background watcher that creates raw captures during work and curates in batches.

Outside the crates:

- `migrations/`
  PostgreSQL schema.
- `.agents/memory-layer.toml`
  Project-owned memory behavior. This is the intended customization layer for scan/analyzer/retrieval behavior.
- `.agents/skills/`
  The repo-local Memory Layer skill bundle. The umbrella skill and shared helper scripts live under `memory-layer/`, and the focused workflow skills live beside it. This is generated/runtime scaffolding, not the main user-edited memory config.
- `packaging/debian/`
  Debian and systemd assets.

## Configuration Resolution

Memory Layer uses layered configuration.

Effective config is resolved in this order:

1. explicit `--config <path>` if provided
2. shared/global config
3. repo-local `.mem/config.toml`
4. `MEMORY_LAYER__...` environment overrides

Shared/global config normally lives in:

- `/etc/memory-layer/memory-layer.toml` for Debian installs
- `~/.config/memory-layer/memory-layer.toml` for local installs

Repo-local overrides live in:

- `.mem/config.toml`

Secrets can come from:

- `/etc/memory-layer/memory-layer.env`
- `~/.config/memory-layer/memory-layer.env`
- `.mem/memory-layer.env`

The loader in `mem-api` also normalizes some legacy config keys during deserialization so older shared configs can coexist with newer repo-local overrides.

## Repository Bootstrap

`memory init` and `memory wizard` both prepare a repository for Memory Layer.

The repo-local bootstrap creates:

- `.mem/config.toml`
- `.mem/project.toml`
- `.mem/runtime/`
- `.agents/memory-layer.toml`
- `.agents/skills/`

This is deliberately repo-local because Memory Layer is project-scoped. The same backend and database may serve many repos, but each repo still needs its own slug, local overrides, and skill installation.

## Runtime Topology

In the normal multi-project setup there is:

- one shared `mem-service`
- one shared PostgreSQL database
- zero or more repo-local watcher processes
- many repos, each with its own `.mem/` configuration and project slug

In the local development setup for this repository there can also be a parallel repo-local backend with alternate ports so it does not clash with the installed shared service.

`mem-service` now has two runtime roles:

- `primary`
  The service can connect to PostgreSQL, runs migrations, owns direct query/capture/curate/reindex work, and serves as the system of record.
- `relay`
  The service cannot connect to PostgreSQL, so it stays up anyway, discovers a primary on the local network over UDP multicast, and proxies the normal HTTP API plus browser WebSocket traffic to that primary.

Relay mode is intentionally thin. It does not maintain its own durable write queue or a local database replica. It is a network-accessible facade over a selected primary.

## Writer Identity

Write-capable flows now require an explicit `writer_id`.

Resolution order is:

1. CLI flag `--writer-id`
2. `MEMORY_LAYER_WRITER_ID`
3. `[writer].id` in config

The value is stored on `sessions.writer_id` and carried in capture payloads. That does two things:

- multiple writers can contribute raw captures to the same project without sharing the same idempotency key
- later debugging can still trace which writer produced which raw capture session

Curated memory remains project-scoped. The system does not create separate canonical memory silos per writer. Cross-writer raw evidence is meant to converge during curation.

## Core Service Boundaries

`mem-service` is the system boundary for persistent state.

It owns:

- migration execution on startup
- project and memory reads
- raw task capture ingestion
- curation execution
- search and query answers
- stats and overview endpoints
- activity event fan-out

The service exposes two transport styles:

- HTTP, which remains the compatibility and fallback surface
- Cap'n Proto-framed streaming, which powers the live TUI

Current HTTP routes include:

- `GET /healthz`
- `POST /v1/query`
- `POST /v1/capture/task`
- `POST /v1/curate`
- `POST /v1/reindex`
- `GET /v1/memory/{id}`
- `DELETE /v1/memory`
- `GET /v1/stats`
- `GET /v1/projects/{slug}/memories`
- `GET /v1/projects/{slug}/overview`
- `POST /v1/archive`
- `GET /ws` for browser streaming

The service also serves the browser UI directly. In production or packaged installs it looks for static assets in a configured or discovered web root, and it falls back to a clear error page if the assets are missing.

The live UI split is now:

- TUI over the existing Cap'n Proto-framed stream
- web UI over a JSON WebSocket on `/ws`

Both consume the same logical stream message model from `mem-api`, which keeps project snapshot, memory snapshot, and activity update behavior aligned across terminal and browser clients.

## Database Model

The PostgreSQL schema is intentionally normalized around project scope and provenance.

Important tables:

- `projects`
  One row per project slug.
- `sessions`
  Logical work sessions associated with a project.
- `tasks`
  Individual tasks inside a session.
- `raw_captures`
  Stored task payloads before curation.
- `memory_entries`
  Canonical curated memories.
- `memory_sources`
  Provenance records tying a memory back to files, tasks, commits, tests, or notes.
- `memory_relations`
  Links between memory entries.
- `memory_tags`
  Searchable tags.
- `memory_chunks`
  Search chunks used by PostgreSQL full-text retrieval.
- `curation_runs`
  Audit trail for curation batches.

The critical integrity rule is that canonical memory is not just loose text. It is tied back to one or more provenance records, which in turn come from captured task evidence.

## Write Path: Explicit Remember

The simplest write path is `memory remember`.

That path looks like this:

1. The CLI resolves the project slug.
2. It derives a capture payload from the provided arguments and current repo state.
3. It sends `POST /v1/capture/task`.
4. The backend stores a `raw_captures` row plus the session/task records that contain it.
5. The CLI then sends `POST /v1/curate`.
6. The backend runs the deterministic curation pipeline.
7. New or updated `memory_entries`, `memory_sources`, `memory_tags`, and `memory_chunks` are written.
8. The backend emits activity events so connected TUIs can update.

This is the highest-confidence path because the user or agent is explicitly saying, “this work should be remembered now.”

## Write Path: Raw Capture And Curation Separation

The more advanced path is the watcher-driven one.

The watcher no longer treats persistence as one indivisible action. Instead it uses a two-speed model:

- raw captures can happen during work
- curation happens less frequently

This is important because it preserves intermediate evidence without flooding canonical memory with half-finished conclusions.

Current default watcher cadence:

- create a raw capture after 10 minutes of stable meaningful changes
- curate after 3 accumulated raw captures
- curate immediately on explicit flush

That means the system stores more evidence than before, but remains conservative about what becomes canonical memory.

## Capture Payload Ingestion

The capture endpoint stores the original payload, but the payload is only the start of the pipeline.

`mem-ingest` is responsible for turning that payload into candidate facts. Its job is not to produce perfect memory entries; its job is to normalize input into a stable intermediate form that curation can reason about.

Inputs that can contribute include:

- agent summary
- notes
- test results
- file paths
- structured candidates
- optional command output

The ingestion stage is intentionally permissive. The curation stage is where the system becomes selective.

## Curation Pipeline

`mem-curate` is where raw evidence becomes durable memory.

The broad pipeline is:

1. load uncurated raw captures for a project
2. extract candidate facts
3. classify memory type
4. reject weak, speculative, or transient statements
5. exact-dedupe against existing memory
6. for non-duplicates, either:
   - insert a new canonical memory
   - replace an older local memory when the candidate is a clear update
   - queue an ambiguous update as a replacement proposal for review
7. attach provenance and tags
8. regenerate search chunks
9. record the curation run and any replacement events

Canonical memories remain immutable. When curation decides that a new candidate updates an older memory, it inserts the new memory and deletes the old one rather than editing in place. Project policy for this lives in `.agents/memory-layer.toml` under `[curation].replacement_policy`.

The important tradeoff here is determinism over maximum semantic richness. The current implementation favors predictable, inspectable behavior and provenance-backed memory over aggressive inference.

## Search And Query Pipeline

The query path is handled by `mem-search`.

The current implementation is PostgreSQL-first. It uses:

- `memory_entries.search_document`
- `memory_chunks.tsv`
- pgvector-backed chunk embeddings stored in `memory_chunk_embeddings`
- `memory_relations` as a reranking signal
- Rust-side reranking
- deterministic answer synthesis

Because chunk embeddings are stored in PostgreSQL with `pgvector`, current deployments need the `vector` extension installed even if semantic recall is not yet enabled in config.

The query flow is:

1. normalize and interpret the query
2. retrieve lexical candidates from PostgreSQL full-text search
3. optionally retrieve semantic candidates with pgvector nearest-neighbor search over chunk embeddings
4. merge and rerank candidates in Rust, including relation boosts when related memories cluster
5. build a deterministic answer from the strongest evidence
6. return ranked results plus score explanations

The important design point is that semantic search is additive, not a replacement for lexical search. Exact wording still matters, but pgvector similarity can now recover relevant memories when the query uses different wording from the stored canonical text.

Embedding storage is now multi-space. A chunk can keep multiple vectors side by side for different embedding models or providers, and the active `[embeddings]` config selects which space semantic retrieval uses. Switching models does not overwrite older vectors; `reembed` only materializes the current active space, and cleanup of older spaces is explicit.

Results are still project-scoped. A query is always evaluated inside one project slug unless the interface is explicitly extended otherwise.

## TUI And Streaming Updates

The TUI is not just a wrapper around repeated HTTP polling.

It maintains a persistent connection to the backend and subscribes to streamed updates. The practical effect is:

- the memory list can update when another client writes memory
- project overview metrics can update in place
- recent backend activity can be replayed when the TUI connects

The Activity tab is backed by streamed events. The backend keeps a recent in-memory backlog and replays it to new TUI sessions so the screen does not start empty after reconnect.

## Watcher Model

`memory-watch` is a repo-local process, not a global daemon.

Its job is to observe a single repository and maintain a task window containing:

- changed files
- derived notes
- timestamps
- test results when available
- a dedupe fingerprint

Two details matter here:

1. The watcher now distinguishes between a file merely being dirty and new activity actually happening. Re-seeing the same dirty files on every poll does not reset the idle timer.
2. The dedupe fingerprint includes file modification state, not just file names, so repeated edits in the same files can still create new raw captures.

That makes the watcher materially more useful than a naive “check git status every few seconds” loop.

## Scan Flow

`memory scan` is a bootstrap path for existing repositories.

It:

1. reads high-value repo files
2. reads recent git history
3. builds or reuses a local repository index under `.mem/runtime/index/`
4. runs parser-backed analyzers for enabled languages from `.agents/memory-layer.toml`
5. builds a structured dossier from indexed evidence
6. sends that dossier to an OpenAI-compatible model
7. validates the returned candidate memories
8. captures and curates them through the normal backend flow

This is intentionally implemented in the CLI rather than as a special backend write path. The backend still remains the source of truth for storage and curation.

For the exact current behavior, limits, dossier contents, validation rules, and troubleshooting notes, see [Scan Command](../../user/cli/scan.md).

## Activity And Observability

The system exposes several layers of observability:

- `memory doctor` for setup diagnostics
- `GET /healthz` for backend/database health
- project overview endpoints for counts and recent activity
- watcher audit logs in `.mem/runtime/`
- TUI Activity streaming
- `curation_runs` in PostgreSQL

The overall design goal is that automatic behavior should never be magic. There should always be a way to inspect what was captured, what was curated, and why.

## Commit History

Commit history is stored as project-scoped evidence, not as canonical memory by default.

The intended flow is:

1. `memory commits sync` reads local git history in the current repository.
2. The CLI sends structured commit records to the backend.
3. The backend stores them in `project_commits`.
4. Those commits become searchable and inspectable project history.
5. Canonical memory may still reference specific commits through provenance when a durable fact is curated from them.

This keeps the system from polluting `memory_entries` with one memory per commit while still preserving the useful parts of commit history.

## Why The Design Looks Like This

The major design decisions are pragmatic:

- PostgreSQL is the canonical store because it already gives durable storage, filtering, and full-text search.
- The service boundary exists so the CLI, TUI, skill, watcher, and packaging story all talk to the same system.
- Raw capture and curated memory are separated because durable memory should be selective.
- The watcher is conservative because background memory writes are only useful if they are trustworthy.
- The TUI uses streaming because a memory browser that does not update when new memories arrive feels stale immediately.

## Where To Read Next

- [Architecture Overview](overview.md)
- [Hidden Memory Daemon](hidden-memory-daemon.md)
- [Getting Started](../../user/getting-started.md)
- [Developer Documentation](../README.md)
