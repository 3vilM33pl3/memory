# ADR 0005: Discovered consolidation groups are not persisted in their own table

Date: 2026-07-17
Status: accepted
Relates to: 3VI-774 (consolidation delta), 3VI-775/777 (superseded)

## Context

The original meta-memory design (3VI-775) called for a
`memory_consolidation_groups` audit table persisting every cluster the
discovery pass finds. The shipped `mem-consolidate` implementation is
stateless per run: discovery fuses relation, similarity, and co-access
signals on demand and the value gate decides acceptance at that moment.
The open question was whether to add the audit table after the fact
(next free migration: 0026).

## Decision

Do not add a dedicated persistence table. Discovered groups remain a
derived, recomputed-on-demand view.

## Rationale

- **Discovery is deterministic and cheap.** The same inputs produce the
  same clusters; recomputation (one SQL pass per signal plus in-process
  community detection) is fast enough for interactive use, which is what
  `GET /v1/projects/{project}/structure` and `memory structure` do.
- **An audit trail already exists.** Every consolidation run stores its
  full `ConsolidationReport` (accepted clusters with members and value-gate
  metrics) in `loop_runs.output_json.consolidation`, queryable through the
  loop-run ledger.
- **Accepted structure is already durable.** A cluster that survives
  synthesis and human approval becomes an `insight` memory plus
  `summarizes` relations — first-class, versioned memory rows.
- A groups table would be a denormalized cache of the fused signal graph:
  it goes stale the moment activation decays or new relations land, and
  keeping it fresh would need either write amplification on every access
  event or a refresh loop, for no consumer that the report + relations do
  not already serve.

## Consequences

- `memory structure` recomputes discovery per call; if very large projects
  ever make that slow, a materialized cache can be reintroduced behind the
  same response shape without changing the API.
- Historical "what did discovery see on date X" questions are answered via
  the loop-run ledger, not a dedicated table.
