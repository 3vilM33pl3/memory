# Structure Command

`memory structure` shows a project's meta-memory structure in two parts:

1. **The committed insight tree** — every active `insight` memory with the
   memories it `summarizes`, nested recursively: insights that summarize
   other insights form higher tiers, exactly as stored in the relation
   graph.
2. **Currently discovered groups** — the clusters the deterministic
   consolidation scan finds right now that pass the value gate and are not
   yet covered by an insight. This is what `memory consolidate` would
   synthesize next.

The command is read-only and makes no LLM calls.

## Usage

```bash
memory structure --project memory
memory structure --project memory --json
```

`--project` defaults to the current repository's project.

## Reading the output

- `◆` marks insight memories; `·` marks leaf memories, with each line
  showing `[memory-type] <canonical-id> <summary>`.
- Indentation depth is the consolidation tier: a tier-2 insight summarizes
  tier-1 insights, which summarize atomic memories.
- Each group prints its value-gate metrics — trigger (`salient` /
  `cold_dense`), intra-cluster density, co-access mass, and activation
  mass — followed by its members.

An empty insight tree simply means no consolidation proposals have been
approved yet; run `memory consolidate` and review the queued proposals
with `memory loops memory-proposals`.
