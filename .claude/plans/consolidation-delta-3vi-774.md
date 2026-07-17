# Consolidation delta (3VI-774 / 3VI-780) — implementation plan

Dependabot triage is done and committed separately (`17a32ef`).

## Decisions
- **Naming (3VI-775 residue):** keep shipped `MemoryRelationType::Summarizes` +
  `MemoryType::Insight`. Audit found zero stale `Abstracts`/`Synthesis`-as-type
  references in code or docs — nothing to change; record decision on the ticket.
- **Persistence (3VI-775/777 residue):** do NOT add a `memory_consolidation_groups`
  audit table. Rationale: discovery is deterministic and cheap to recompute;
  every loop run already persists the full `ConsolidationReport` in
  `loop_runs.output_json` (a queryable audit trail); accepted+synthesized
  structure is persisted as insight memories + `summarizes` relations. A second
  table would be a denormalized cache with staleness risk. Migration 0026 stays
  free. Record decision + rationale on 3VI-774.

## `memory consolidate --project X [--dry-run]` (3VI-774)
Pure CLI convenience over the existing frozen route
`POST /v1/loops/memory_consolidation/run` (no new endpoint):
- `crates/mem-cli/src/commands/consolidate.rs` (new): resolve project slug,
  run the loop, pretty-print the stored `consolidation` report from
  `output_json` (candidates/accepted/rejected/covered + per-cluster trigger,
  size, density, member summaries). Non-dry-run: afterwards list pending
  proposals for the run (`/v1/loops/memory-proposals?run_id=`) and print the
  queued count + `memory proposals` hint.
- `runtime.rs`: `Consolidate(ConsolidateArgs { project, dry_run, json })` +
  dispatch + after-help.

## `memory structure --project X` (3VI-780)
One additive GET endpoint + CLI renderer:
- `mem-api/src/types.rs`: `ProjectStructureResponse`, `StructureGroupInfo`,
  `StructureMemberInfo`, recursive `StructureInsightNode`.
- `mem-service/src/repository/handlers/consolidation.rs`:
  `fetch_insight_tree(pool, project)` (active insights + `summarizes` edges →
  recursive tiers, leaves = summarized non-insight memories) and HTTP handler
  `project_structure` (primary-proxy like `memory_scores`, real
  `state.config.consolidation` + reinforcement half-life, runs the
  deterministic scan for current groups — no LLM).
- `routes.rs`: `.route("/v1/projects/{project}/structure", get(...))`.
- `docs/api/openapi.yaml`: add the path (openapi_contract test enforces parity).
- `crates/mem-cli/src/commands/structure.rs` (new): render insight tree
  (recursively, with member leaves) + discovered-group table + counts.
- CLI `Structure(StructureArgs { project, json })`.

## Tests / verification
- DB test: seed insight + summarizes relations → `fetch_insight_tree` tiers.
- Existing consolidation DB test still green; `openapi_contract` green.
- cargo fmt + clippy + workspace tests.
- Live smoke run of both commands against the dev stack; 3VI-780 asks for a
  smoke run on the real `rillforge` project memories (dev DB copy).

## Wrap-up
- Docs: add both commands to the CLI reference page(s).
- Linear: comment decisions + delivery on 3VI-774, close 3VI-780 → Done after
  smoke run; 3VI-774 Done when all four delta items are resolved.
- Commit per repo conventions; `memory remember` at the end.
