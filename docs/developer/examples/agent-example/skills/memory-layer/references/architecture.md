# Memory Layer Architecture

## Overview

This project implements a local-first memory layer for coding agents, designed to work with Codex via a repo-local Skill. The system stores raw task captures and curated durable memory, supports project-specific querying, and runs as a Rust backend service with PostgreSQL-backed BM25 text retrieval.

The architecture is intentionally split into:
1. **Skill layer** for agent workflow orchestration
2. **CLI layer** for local command execution
3. **Backend service** for API, capture, curation, and retrieval
4. **PostgreSQL storage** for persistence and search

This design keeps the model logic thin, makes the runtime inspectable, and avoids coupling durable memory to any single model provider.

---

## Goals

- Use **repo-local Codex Skills**
- Be **local-first**
- Use **Rust where possible**
- Use **PostgreSQL** as the canonical data store
- Use **BM25 text retrieval** in PostgreSQL for search
- Run as a **systemd-managed backend service**
- Be installable as a **Debian package**
- Preserve **provenance** for every memory item
- Store both **raw captures** and **curated memory**
- Keep v1 usable **without requiring an LLM**
- Allow optional LLM-assisted curation behind a feature flag

---

## Non-Goals for V1

- No mandatory vector database
- No mandatory cloud sync
- No distributed multi-node architecture
- No complex frontend UI
- No hidden agent-side persistence outside the backend
- No opaque memory writes without provenance

---

## System Context

### Main Components

#### 1. Codex Skill
A repo-local Skill in `.agents/skills/memory-layer/` tells Codex when to:
- query memory before answering project-specific questions
- capture task context after meaningful work
- curate raw captures into durable memory

The Skill calls local scripts, which in turn invoke the CLI.

#### 2. CLI (`memory`)
A Rust CLI that acts as the user and agent entrypoint.

Responsibilities:
- query memory
- capture task results
- trigger curation
- health checks
- reindex
- export
- stats
- archive/prune

The CLI talks to the backend over localhost HTTP.

#### 3. Backend Service (`memory-layer`)
A Rust daemon running under systemd.

Responsibilities:
- API handling
- capture ingestion
- curation pipeline
- retrieval/ranking
- indexing
- provenance management
- audit trail
- config management
- observability

#### 4. PostgreSQL
PostgreSQL is the system of record.

Responsibilities:
- durable storage
- BM25 full-text search
- metadata persistence
- provenance joins
- operational reporting

---

## High-Level Flow

### Query Flow

1. User asks a project-specific question
2. Codex Skill decides to query memory first
3. Skill script calls `memory query`
4. CLI sends request to backend
5. Backend performs PostgreSQL BM25 retrieval
6. Backend ranks and groups results
7. Backend returns:
   - answer summary
   - ranked memory entries
   - snippets
   - provenance
   - confidence
8. Codex uses result in its reasoning / response

### Capture Flow

1. Task completes or meaningful knowledge is discovered
2. Codex Skill decides to capture the task
3. Skill script calls `memory capture task`
4. CLI sends structured payload to backend
5. Backend stores raw capture in PostgreSQL
6. Raw capture is available for later curation and replay

### Curation Flow

1. Task is complete or user explicitly requests memory update
2. Skill script calls `memory curate`
3. Backend loads uncured raw captures
4. Backend normalizes and extracts candidate assertions
5. Backend deduplicates against existing memory
6. Backend creates or updates canonical memory entries
7. Backend writes provenance links and refreshes search chunks
8. Memory becomes queryable

---

## Why Store Raw Captures and Curated Memory

Storing both layers is required.

### Raw Captures
Raw captures preserve:
- original task context
- command outputs
- task summaries
- file changes
- test results
- lessons learned
- replayability for future recuration

### Curated Memory
Curated memory provides:
- stable canonical facts
- deduplicated searchable entries
- better retrieval precision
- lower noise during query

This two-layer model makes the system debuggable, reprocessable, and safer to evolve.

---

## Repository Structure

```text
memory-layer/
  .agents/
    skills/
      memory-layer/
        SKILL.md
        agents/
          openai.yaml
        scripts/
          query-memory.sh
          capture-task.sh
          curate-memory.sh
        references/
          architecture.md
          query-contract.md
          curation-rules.md

  crates/
    mem-api/
    mem-cli/
    mem-service/
    mem-ingest/
    mem-search/
    mem-curate/

  packaging/
    debian/
      control
      postinst
      prerm
      memory-layer.service
      memory-layer.env

  migrations/
  docs/
  tests/
```

---

## Rust Crates

### `mem-api`
Shared API contracts and DTOs.

Contains:
- request/response models
- validation types
- enums for memory types and statuses
- config types if shared

### `mem-cli`
Rust crate behind the public `memory` CLI binary.

Contains:
- command parsing
- HTTP client
- output formatting
- shell-friendly modes
- JSON output mode

### `mem-service`
Main backend service binary.

Contains:
- Axum app
- route registration
- config bootstrapping
- health endpoints
- middleware
- background jobs if needed

### `mem-ingest`
Capture normalization and source extraction.

Contains:
- payload normalization
- candidate assertion extraction
- chunking
- source typing

### `mem-search`
Search and ranking logic.

Contains:
- PostgreSQL FTS query builders
- BM25 ranking integration
- highlighting/snippets
- result grouping
- ranking boosts

### `mem-curate`
Deterministic curation logic.

Contains:
- classification
- dedupe
- canonicalization
- provenance linking
- merge strategies

---

## Data Model

### `projects`
Tracks logical projects.

Fields:
- `id`
- `slug`
- `name`
- `root_path`
- `created_at`

### `sessions`
Tracks agent/user sessions.

Fields:
- `id`
- `project_id`
- `external_session_id`
- `started_at`
- `ended_at`
- `agent_name`

### `tasks`
Tracks units of work.

Fields:
- `id`
- `session_id`
- `title`
- `user_prompt`
- `task_summary`
- `status`
- `created_at`
- `completed_at`

### `raw_captures`
Stores original raw memory input.

Fields:
- `id`
- `task_id`
- `capture_type`
- `payload_json`
- `idempotency_key`
- `created_at`

### `memory_entries`
Canonical memory records.

Fields:
- `id`
- `project_id`
- `canonical_text`
- `summary`
- `memory_type`
- `scope`
- `importance`
- `confidence`
- `status`
- `created_at`
- `updated_at`
- `archived_at`

### `memory_sources`
Provenance records for memory entries.

Fields:
- `id`
- `memory_entry_id`
- `task_id`
- `file_path`
- `git_commit`
- `source_kind`
- `excerpt`

### `memory_relations`
Links between memory entries.

Fields:
- `id`
- `src_memory_id`
- `relation_type`
- `dst_memory_id`

### `memory_tags`
Tags for filtering and ranking.

Fields:
- `memory_entry_id`
- `tag`

### `memory_chunks`
Searchable units for BM25 retrieval.

Fields:
- `id`
- `memory_entry_id`
- `chunk_text`
- `search_text`
- `tsv`

### `curation_runs`
Tracks curation execution.

Fields:
- `id`
- `project_id`
- `trigger_type`
- `input_count`
- `output_count`
- `model_name`
- `created_at`

---

## Search Architecture

### Retrieval Strategy
V1 uses PostgreSQL full-text search with BM25 ranking.

Search inputs:
- query text
- type filters
- tag filters
- project scope
- recency weighting
- importance weighting
- confidence weighting

Search targets:
- `memory_entries.canonical_text`
- `memory_entries.summary`
- `memory_chunks.chunk_text`

### Ranking
Ranking should combine:
- BM25 score
- exact phrase boosts
- tag matches
- type matches
- importance
- confidence
- light recency decay

### Output
Query returns:
- answer summary
- memory entries
- snippets
- provenance
- score breakdown
- confidence / insufficiency signal

### Future Extension
Leave a clear extension point for:
- vector reranking
- hybrid retrieval
- graph traversal over `memory_relations`

---

## Curation Architecture

### V1 Curation Approach
Use deterministic curation first.

Pipeline:
1. load uncured raw captures
2. normalize payload
3. extract candidate assertions
4. classify by memory type
5. dedupe against recent/existing memory
6. create or update canonical entry
7. attach provenance
8. re-chunk and reindex

### Classification Types
At minimum:
- `architecture`
- `convention`
- `decision`
- `incident`
- `debugging`
- `environment`
- `domain_fact`

### Dedupe Strategy
Use:
- normalized text comparison
- trigram similarity
- same provenance heuristics
- same file / task clustering

### Optional LLM Support
Optional model use should be behind a feature flag and limited to:
- canonical summarization
- merge suggestion
- assertion compression

Never allow the optional LLM path to create memory without provenance.

---

## API Design

### Endpoints

#### `GET /healthz`
Returns service liveness and dependency status.

#### `POST /v1/capture/task`
Stores a raw capture payload.

#### `POST /v1/curate`
Triggers curation for a project or scope.

#### `POST /v1/query`
Queries project memory.

#### `POST /v1/reindex`
Rebuilds search chunks/index state.

#### `GET /v1/memory/:id`
Returns one memory entry with provenance.

#### `GET /v1/stats`
Returns service stats.

#### `POST /v1/archive`
Archives or prunes low-value entries.

---

## Example Capture Payload

```json
{
  "project": "my-project",
  "task_title": "Add JWT refresh token rotation",
  "user_prompt": "Implement refresh token rotation",
  "agent_summary": "Added single-use refresh token rotation and revocation checks",
  "files_changed": [
    "auth/refresh.rs",
    "auth/tokens.rs"
  ],
  "git_diff_summary": "Introduced rotation and revocation logic",
  "tests": [
    {
      "command": "cargo test -p auth",
      "status": "passed"
    }
  ],
  "notes": [
    "Refresh tokens must be invalidated after use"
  ]
}
```

---

## Example Query Response

```json
{
  "answer": "Refresh tokens are single-use and rotated on renewal.",
  "confidence": 0.84,
  "results": [
    {
      "memory_id": "mem_123",
      "summary": "JWT refresh token rotation",
      "score": 14.22,
      "snippet": "Refresh tokens are invalidated after successful rotation...",
      "sources": [
        {
          "task_id": "task_42",
          "file_path": "auth/refresh.rs"
        }
      ]
    }
  ]
}
```

---

## Configuration

Support:
- config file
- environment variables
- CLI overrides where appropriate

Key config:
- bind address
- port
- database URL
- auth token
- log level
- feature flags
- curation batch size
- archive thresholds

Default behavior:
- bind to localhost only
- require API token for write endpoints
- structured logs enabled

---

## systemd Design

The backend runs as a hardened systemd service.

Service requirements:
- dedicated user/group
- no root runtime
- restricted filesystem access
- restart on failure
- environment file support
- journald-friendly structured logs

Runtime directories:
- `/etc/memory-layer/`
- `/var/lib/memory-layer/`
- `/var/log/memory-layer/`

---

## Debian Packaging

The `.deb` package should install:
- service binary
- CLI binary
- systemd unit
- environment file
- migrations/assets
- docs/examples

Post-install should:
- create service user if needed
- create directories
- reload systemd
- optionally enable service

---

## Security Requirements

- localhost-only by default
- token authentication for write endpoints
- structured audit trail for all writes
- no destructive curation without provenance
- no unaudited auto-delete
- idempotent capture where possible
- explicit archive rather than hard delete by default

---

## Observability

Expose:
- health status
- query counts
- zero-result queries
- curation run counts
- dedupe merges
- archive counts
- latency metrics

Use structured logs with correlation IDs.

---

## Failure Handling

### PostgreSQL unavailable
- service returns degraded health
- queries fail clearly
- writes do not silently drop

### Duplicate capture submission
- idempotency key prevents duplicate raw capture rows

### Curation conflicts
- store a curation run record
- do not overwrite canonical memory without auditability

### Search miss
- return explicit insufficient-evidence response

---

## Testing Strategy

### Unit tests
- normalization
- classification
- ranking
- dedupe

### Integration tests
- PostgreSQL-backed query
- capture persistence
- curation flow
- archive/reindex

### Golden tests
- canonical memory creation from fixture captures

### Benchmarks
- query latency
- curation batch throughput

---

## Milestones

### Phase 1
Scaffold workspace, service, CLI, migrations, config.

### Phase 2
Implement query + BM25 retrieval.

### Phase 3
Implement raw capture ingestion.

### Phase 4
Implement deterministic curation.

### Phase 5
Add Skill and scripts.

### Phase 6
Add systemd + Debian packaging.

### Phase 7
Add optional LLM-assisted canonicalization and archive policies.

---

## Key Design Principles

1. **Skill-driven, not prompt-bloated**
2. **Local-first**
3. **Durable provenance**
4. **Deterministic by default**
5. **Inspectable storage**
6. **Replayable raw capture**
7. **Operationally boring**
8. **Easy to package and install**
9. **Safe to extend later**
