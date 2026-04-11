# Memory Types Reference

This page explains the curated memory types used by Memory Layer today.

It is a developer reference, not a user tutorial. It focuses on:

- what each memory type means
- what kind of content it should contain
- how the type is produced by the current write paths
- how the type is stored in PostgreSQL
- how type-specific behavior affects query, curation, and the TUI

The live source of truth for the type list is [`MemoryType` in `mem-api`](../../../crates/mem-api/src/lib.rs).

## Table of Contents

- [What A Memory Type Is](#what-a-memory-type-is)
- [Current Type List](#current-type-list)
- [Type-By-Type Reference](#type-by-type-reference)
- [How Types Get Triggered](#how-types-get-triggered)
- [How Types Link To The Database](#how-types-link-to-the-database)
- [Lifecycle And Query Behavior](#lifecycle-and-query-behavior)
- [Practical Distinctions](#practical-distinctions)

## What A Memory Type Is

A memory type is the category of a curated canonical memory entry.

It is not:

- a raw capture type
- a database table name
- a watcher event kind
- an activity feed kind

The important split in Memory Layer is:

- raw captures hold task payloads and evidence
- curated memory entries hold durable reusable knowledge

`memory_type` exists on the curated side. A type answers the question:

> What kind of durable knowledge is this memory supposed to represent?

That type then affects:

- filtering in `memory query`
- rendering and filtering in the TUI
- curation and replacement behavior in a few special cases
- how future readers interpret the memory

## Current Type List

Memory Layer currently supports these curated memory types:

- `architecture`
- `convention`
- `decision`
- `incident`
- `debugging`
- `environment`
- `domain_fact`
- `plan`
- `implementation`

The type list is defined in code, but `memory_entries.memory_type` is stored in PostgreSQL as `TEXT`, not a database enum. The application layer is responsible for validating and interpreting the value.

## Type-By-Type Reference

### `architecture`

Use this for long-lived structural facts about the system.

Contains:

- component boundaries
- service responsibilities
- protocol or subsystem interaction patterns
- major data-flow or runtime-shape facts

Should not contain:

- temporary implementation details
- project workflow rules
- one-off debugging observations

Typical triggers:

- `scan` extracting repo structure
- `remember` when the captured outcome is about how the system is organized
- manually supplied structured candidates

Examples:

- “The backend service owns capture, curation, and query endpoints.”
- “The TUI uses a persistent Cap'n Proto stream for live updates.”

### `convention`

Use this for established team or project practice.

Contains:

- coding patterns used consistently in the repo
- operational or workflow rules
- conventions for where things belong or how they are done

Should not contain:

- rationale-heavy product or technical decisions
- environment setup facts that are purely machine-specific
- implementation outcomes from one finished task

Typical triggers:

- `scan` extracting stable workflow/coding practices
- `remember` when notes or inferred text describe how the project does things
- generic inferred curation when text mentions conventions, rules, or workflows

Examples:

- “Watcher manager state is tracked through the user systemd manager on Linux.”
- “Repo-local skill helpers are invoked through `go run` from the skill bundle.”

### `decision`

Use this for a deliberate choice that future work may need to understand.

Contains:

- what was chosen
- what alternative was rejected or avoided
- why the choice matters

Should not contain:

- generic conventions without a specific decision point
- debugging notes
- architecture facts that do not encode a deliberate choice

Typical triggers:

- `remember` when the note or summary uses decision language
- explicit manual captures
- curation of task outcomes that describe a chosen policy or tradeoff

Examples:

- “Memory Layer keeps PostgreSQL localhost-only and exposes `mem-service` instead.”
- “The watcher manager is Linux/systemd-first in v1 rather than cross-platform.”

### `incident`

Use this for a notable failure and its verified resolution.

Contains:

- the failure condition
- the verified fix or operational resolution
- enough context to recognize the problem later

Should not contain:

- transient troubleshooting attempts
- broad lessons that are better categorized as `debugging`
- generic implementation summaries

Typical triggers:

- explicit `remember` calls after a significant outage or failure
- manual structured capture
- curated watcher or service recovery work when the failure itself matters as historical context

Examples:

- “Systemd transient watcher startup failed when a loaded unit already existed; the manager now reuses or clears the unit before retry.”

### `debugging`

Use this for durable troubleshooting knowledge.

Contains:

- root causes
- reliable diagnostic signals
- repeatable fixes
- lessons that make future debugging faster

Should not contain:

- every failed attempt during a debugging session
- chain-of-thought style notes
- the final delivered feature result when that result should be visible as `implementation`

Typical triggers:

- inferred curation when captured text mentions `debug`, `fix`, or `bug`
- `remember` notes that preserve a troubleshooting lesson
- secondary memories alongside an `implementation` memory for the same task

Examples:

- “The TUI footer showed the manager as off because it only checked the systemd unit and not a live foreground process.”
- “A stale installed binary can reject new enum variants even when the repo code already supports them.”

### `environment`

Use this for tooling, setup, deployment, and runtime-environment facts.

Contains:

- configuration expectations
- runtime prerequisites
- platform-specific setup behavior
- local service wiring facts

Should not contain:

- structural architecture facts
- decisions without environment/setup content
- task-completion summaries

Typical triggers:

- `scan` extracting setup or tooling information
- inferred curation when captured text mentions setup, config, or environment
- explicit `remember` usage for deployment and operational facts

Examples:

- “The repo-local skill bundle requires `go` on `PATH` because helpers run through `go run`.”
- “The shared service token is stored in the adjacent `memory-layer.env` file.”

### `domain_fact`

Use this for stable facts about the external problem domain the project works in.

Contains:

- product-domain facts
- protocol or business rules external to the local code structure
- facts the system must respect even if implementation changes

Should not contain:

- repo conventions
- local architecture details
- temporary implementation choices

Typical triggers:

- `scan` or `remember` when the content is about the subject matter, not the repo itself
- explicit structured candidate capture

Examples:

- “SCTP message boundaries must be preserved across a single message send/receive exchange.”

### `plan`

Use this for an approved execution plan that should guide the current implementation thread.

Contains:

- the full approved plan markdown
- Markdown checkbox items that define completion

Should not contain:

- the final implemented outcome
- post-task lessons
- generic work summaries without checkbox structure

Typical triggers:

- `memory checkpoint start-execution`
- plan resaves for the same thread when the approved plan changes materially

Important current behavior:

- `plan` is stored as structured memory, not inferred
- its canonical text preserves multiline Markdown
- curation has deterministic same-thread replacement behavior based on `plan-thread:<thread_key>`

Examples:

- the approved plan captured at execution start for `3VI-520`

### `implementation`

Use this for what was actually delivered or completed.

Contains:

- the verified shipped outcome
- completed plan items or explicit outcome summaries
- optional durable notes that clarify what was implemented

Should not contain:

- raw troubleshooting history
- unchecked future work
- generic conventions unless the implementation result itself is the important artifact

Typical triggers:

- `memory checkpoint finish-execution` after successful checklist verification
- `memory remember` for non-plan completed work
- explicit structured candidates supplied by callers

Important current behavior:

- `finish-execution` now records an `implementation` memory automatically when plan verification succeeds
- `remember` now builds an explicit structured `implementation` candidate by default
- debugging lessons can still be curated as separate secondary memories for the same task

Examples:

- “Implemented first-class implementation memories.”
- “Recorded watcher manager status and service role detail in the TUI footer.”

## How Types Get Triggered

Memory Layer uses two broad paths for type assignment.

### 1. Explicit structured candidates

Some flows choose the type up front and send it as part of the capture payload.

Current important examples:

- `memory checkpoint start-execution`
  - writes a structured `plan` candidate
- `memory checkpoint finish-execution`
  - writes a structured `implementation` candidate after successful verification
- `memory remember`
  - now writes a structured `implementation` candidate by default
- `scan`
  - converts accepted scan candidates into explicit typed structured candidates

This is the highest-confidence path because the caller already knows what kind of memory is being recorded.

### 2. Inferred curation from raw task evidence

If the capture does not rely only on structured candidates, `mem-ingest` can still infer additional candidates from:

- notes
- task title
- prompt text
- summary text
- files changed
- tests
- command output

Current inference behavior is intentionally simple and deterministic. Examples:

- text containing `debug`, `fix`, or `bug` tends toward `debugging`
- text mentioning decisions tends toward `decision`
- setup/config language tends toward `environment`
- generic completed-work text now falls back to `implementation`

This inferred path is how secondary memories can coexist with a primary structured one. A completed task can therefore yield:

- one `implementation` memory for what shipped
- one `debugging` memory for the durable troubleshooting lesson

## How Types Link To The Database

Types live on the curated side of the data model.

The relevant write path is:

1. a command or watcher creates a `CaptureTaskRequest`
2. the backend writes a `raw_captures` row and related `sessions` / `tasks`
3. curation extracts candidate assertions
4. accepted candidates become rows in `memory_entries`
5. provenance, tags, relations, and search material are written beside them

The important PostgreSQL tables are:

- `raw_captures`
  - stores the original task payload before curation
- `memory_entries`
  - stores canonical curated memory
  - `memory_type` is a `TEXT` column
- `memory_sources`
  - stores provenance such as files, notes, tests, commits, or prompts
- `memory_tags`
  - stores searchable tags
- `memory_relations`
  - links one memory entry to another
- `memory_chunks`
  - stores search chunks used by lexical and semantic retrieval
- `curation_runs`
  - stores the audit trail of curation batches

So the type-to-database relationship is:

- the application assigns `memory_type`
- PostgreSQL stores it on `memory_entries`
- provenance and search material for that memory are split across the related tables

## Lifecycle And Query Behavior

All memory types share the same basic lifecycle:

- inserted as `active`
- optionally archived later
- returned through query/search/TUI filters by `memory_type`

Important current special cases:

- `plan`
  - same-thread updates replace the older active plan rather than creating ambiguous duplicates
- `implementation`
  - multiple implementation memories may exist over time
  - exact finish-execution reruns are made idempotent to avoid duplicate inserts for the same verified outcome

The type is visible in:

- `memory query --type <memory_type>`
- TUI memory filters and type pills
- project overview type breakdowns
- query/search result metadata

## Practical Distinctions

The most important boundaries are:

### `implementation` vs `debugging`

Use `implementation` for:

- what was delivered
- what changed in durable outcome terms

Use `debugging` for:

- the troubleshooting lesson
- the root cause and reliable fix pattern

A finished bug fix can legitimately produce both.

### `decision` vs `convention`

Use `decision` when:

- there was a meaningful choice or tradeoff

Use `convention` when:

- the important fact is simply how the project does something now

### `architecture` vs `domain_fact`

Use `architecture` for:

- repo/system structure
- service/component interaction

Use `domain_fact` for:

- facts about the problem domain that remain true even if the implementation changes

### `plan` vs `implementation`

Use `plan` for:

- what should happen
- checkbox-driven execution guidance

Use `implementation` for:

- what actually happened
- the verified completed result

These are intentionally separate memories so future readers can distinguish intent from outcome.
