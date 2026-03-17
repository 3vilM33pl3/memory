# Memory Layer Architecture

## Overview

Memory Layer is a local-first memory system for coding agents. It stores raw task captures and curated durable memory, supports project-scoped querying, and uses PostgreSQL as the canonical system of record.

The architecture is split into:
1. Skill layer for agent workflow orchestration
2. CLI layer for local command execution
3. Backend service for capture, curation, retrieval, and API handling
4. PostgreSQL storage for durable persistence and search
5. Optional automation daemon for background observation and automatic memory persistence

This keeps memory writes inspectable, replayable, and decoupled from any single agent runtime.

## Main Components

### Skill

The repo-local skill in `.agents/skills/memory-layer/` tells Codex when to:
- query memory before answering project-specific questions
- remember meaningful work
- curate durable project knowledge

### CLI (`memctl`)

`memctl` is the user and agent entrypoint for:
- repo bootstrap (`init`)
- query
- remember
- capture-task
- curate
- reindex
- TUI views
- automation status and controls

The CLI talks to the backend over localhost HTTP.

Initialized repositories keep local project metadata and overrides under `.mem/`, with `.mem/config.toml` as the repo-local override file and `.mem/runtime/` as the preferred watcher runtime directory. Shared secrets and defaults live in the global config and are merged underneath repo-local overrides.

### Backend Service (`memory-layer`)

The backend owns:
- API routes
- raw capture ingestion
- deterministic curation
- retrieval and ranking
- provenance
- stats and operational reporting

### PostgreSQL

PostgreSQL stores:
- projects
- sessions
- tasks
- raw captures
- canonical memories
- provenance
- search chunks
- curation runs

### Automation Daemon (`memory-watch`)

`memory-watch` is a local background process that observes repository activity, builds candidate task windows, and persists memory through the existing remember flow when configured to do so.

It does not write directly to database tables. It only orchestrates the existing persistence path.

## High-Level Flows

### Query Flow

1. User asks a project-specific question
2. Skill or CLI runs `memctl query`
3. Backend retrieves project memory from PostgreSQL
4. Ranked results and provenance are returned

### Remember Flow

1. Meaningful work is completed
2. Skill, user, or automation daemon runs `memctl remember`
3. CLI builds a capture request
4. Backend stores a raw capture
5. Backend curates it into canonical memory
6. Memory becomes queryable

### Automation Flow

1. `memory-watch` observes file and command activity for a repo
2. It accumulates a task window
3. After idle time, passing tests, or explicit flush, it evaluates whether the work is meaningful
4. If the confidence threshold is met, it runs the remember flow
5. It records the decision in a local audit log

## Design Principles

- Local-first
- Deterministic by default
- No canonical memory without provenance
- Project-scoped memory
- Auditability over hidden magic
- Background automation must be conservative
