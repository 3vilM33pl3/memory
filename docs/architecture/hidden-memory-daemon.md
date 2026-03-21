# Hidden Memory Daemon

## Purpose

The hidden memory daemon is a local companion process that observes repository activity and automatically persists durable memory when the signal is strong enough.

Its purpose is to reduce the manual step of calling `remember` after meaningful work while still preserving provenance and keeping automatic writes inspectable.

## What Problem It Solves

Today, the skill and CLI can remember work, but they still require an explicit command path. That means durable project knowledge is only stored if the agent or user remembers to invoke the workflow.

The daemon closes that gap by:
- watching work as it happens
- grouping events into task windows
- deciding when a task likely settled
- calling the existing remember pipeline automatically

## Non-Goals

The daemon is not meant to:
- invent semantic summaries from weak signals
- write directly to PostgreSQL tables
- persist every small edit
- replace explicit user control over memory writes
- depend on a hidden Codex lifecycle hook that does not exist in this repo today

## Why It Must Be External

There is no native Codex CLI post-task lifecycle hook available in this repository. Because of that, background automation has to be implemented as a separate local process instead of a direct Codex integration point.

That process can still work well because the memory system already has:
- a stable CLI
- a stable backend API
- a project-scoped data model
- a `remember` path that captures and curates in one step

## Responsibilities

The daemon should:
- watch file activity under a repo root
- track optional wrapped command and test events
- build an in-memory task window
- evaluate whether the work is meaningful enough to remember
- call `memctl remember` or the equivalent API flow
- write an audit log of persisted and skipped decisions

## Observed Signals

The daemon should use:
- repository file changes
- changed file set from the current work window
- git status/diff snapshots when available
- explicit wrapped test commands and outcomes
- idle periods after activity
- explicit flush requests

It should ignore:
- configured ignored paths
- temporary files
- trivial edits without durable significance
- repeated duplicate windows

## Task Window Model

The daemon should maintain one active task window per project containing:
- project slug
- repo root
- start time
- last activity time
- changed files
- recent commands and test outcomes
- candidate durable notes
- the last persistence fingerprint

The window should roll forward as activity continues and should only be flushed when a trigger condition is met.

## Trigger Rules

Recommended trigger conditions:
- idle threshold reached after meaningful edits
- a passing wrapped test occurs after changes
- an explicit flush command is issued

Recommended cadence:
- create raw captures during work when one of the trigger conditions fires
- curate canonical memory later, either after several accumulated captures or on explicit flush

Recommended conservative default:
- only auto-persist when at least one durable note exists or the change set plus passing tests gives enough confidence
- otherwise skip in `auto` mode or keep the candidate as a suggestion in `suggest` mode

## Safety Model

Automatic writes must be conservative.

Rules:
- no direct SQL writes
- no write without provenance
- no write for ignored-only or trivial changes
- no write for duplicate fingerprints
- disabled by default
- audit every decision

Recommended modes:
- `suggest`: generate and log candidate captures but do not persist
- `auto`: persist only high-confidence candidates

## Persistence Path

The daemon should call the existing capture and curate pipelines rather than bypassing them:
1. build a capture request
2. store a raw capture through the backend
3. optionally trigger curation when the batch threshold or explicit flush rules say it is time
4. let the backend turn those accumulated captures into canonical memory

This keeps:
- validation centralized
- provenance rules centralized
- dedupe centralized
- curation logic centralized

## Auditability

Every decision should be recorded locally:
- timestamp
- project
- mode
- changed files
- trigger reason
- persisted or skipped
- skip reason if skipped
- resulting raw capture / curation run IDs when persisted

The daemon should be debuggable without guessing.
