# ADR 0002: Reinforcement Scores Live Outside the Immutable Version Chain

Status: Accepted

Date: 2026-07-05

## Context

Since migration `0013`, every row in `memory_entries` is an immutable
version: updates append a new row under a shared `canonical_id` and the old
row is preserved. The reinforcement system (see
`docs/developer/architecture/memory-reinforcement.md`) needs *mutable*,
frequently updated state per memory: a decaying activation score, access
counters, volatility, and validation status metadata. It also needs a place
for append-only access events, score audits, and validation runs.

Two options were considered:

1. Add columns to `memory_entries` and mutate the latest version in place.
2. Keep all mutable state in a separate `memory_scores` table keyed by
   `canonical_id`, plus dedicated append-only tables.

## Decision

Option 2. `memory_scores` is keyed by `canonical_id` with **no foreign key
to `memory_entries`**: retention may prune version 1 (whose `id` equals the
`canonical_id`), which would otherwise cascade away live score state or
block pruning. Orphaned score rows are removed by the scheduler's weekly
compaction sweep instead.

Validation corrections are stored on `memory_validation_runs`
(`proposed_candidate_json` + `review_status`) rather than reusing
`memory_replacement_proposals`, whose schema requires non-null
`task_id`/`raw_capture_id` from the capture pipeline; reuse would have
meant nullable-column surgery on a stable approval path.

## Consequences

- The version chain stays immutable and auditable; every content change the
  reinforcement system applies (rewording, approved corrections) is a new
  version created through `mem_curate::apply_validation_revision` and is
  revertible.
- Score updates are single-statement atomic upserts with decay computed
  inside the UPDATE, safe under concurrent writers.
- Ranking reads join `memory_scores` through `canonical_id`; a missing row
  simply means activation 0.
- Two review surfaces exist (replacement proposals from curation,
  validation corrections from reinforcement). They are conceptually
  different — new knowledge vs. maintenance of existing knowledge — and
  share the human-gated apply mechanism through mem-curate.
