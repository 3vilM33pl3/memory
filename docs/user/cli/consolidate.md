# Consolidate Command

`memory consolidate` runs memory consolidation for one project: it discovers
clusters of related memories (relation edges, embedding similarity, and
co-access fused into one weighted graph) and synthesizes each accepted
cluster into a higher-level `insight` memory **proposal**. Nothing is written
to memory until a proposal is approved with
`memory loops memory-proposals`.

It is a first-class convenience over the `memory_consolidation` loop — the
run is recorded in the loop run ledger exactly as if it were triggered by
the scheduler or auto-trigger.

## Usage

```bash
memory consolidate --project memory --dry-run   # deterministic scan only
memory consolidate --project memory             # scan + LLM synthesis
memory consolidate --project memory --json      # full run detail as JSON
```

`--project` defaults to the current repository's project.

## What the output means

- **candidates / accepted / rejected / covered** — clusters found by
  community detection; `accepted` passed the value gate (size, cohesion,
  salience), `covered` are already summarized by an existing insight
  (novelty gate).
- **trigger** — `salient` (heavily used together) or `cold_dense`
  (dense but unused; consolidation as compression).
- With `--dry-run`, no LLM is called and no proposals are queued — use it
  to preview what consolidation would do.

## Requirements

- The `memory_consolidation` loop must be enabled
  (`memory loops enable memory_consolidation ... --explicit-user-approval`).
- Proposal synthesis additionally requires `[consolidation] enabled = true`
  and `dry_run = false` in the service config, plus a configured LLM.
- Synthesis makes two LLM calls per accepted cluster; the command waits up
  to five minutes.

Review queued proposals:

```bash
memory loops memory-proposals --project memory --status pending
```

See `memory structure` for the committed insight tree and the clusters a
scan would currently propose, and the consolidation chapter of the docs
site for the science behind the value gate.
