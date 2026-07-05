# Memory Reinforcement and Validation

The reinforcement system keeps frequently used and volatile memories
accurate, current, and well-worded. It has two halves:

1. **Access-driven activation scoring** (always cheap, no LLM): every time a
   memory is retrieved by a query, cited in a synthesized answer, or read
   directly, its *activation* increases. Activation spreads to linked
   memories with graph-distance decay, decays exponentially over time, and
   feeds back into search ranking with a small, log-capped boost.
2. **Threshold-triggered validation** (opt-in, LLM-backed): when a memory's
   activation crosses a configurable threshold, a pipeline gathers evidence
   from the project (sources, provenance verifications, related memories,
   git history), asks an LLM for a verdict, and then either re-confirms the
   memory, improves its wording, queues a correction for human review, or
   flags it — it never silently rewrites content on weak evidence.

Crate: `crates/mem-reinforce`. Service wiring: `crates/mem-service`
(`src/reinforcement.rs`, `src/repository/handlers/reinforcement.rs`).

## Scoring model

The activation score is ACT-R base-level activation in Petrov's O(1)
incremental form: instead of replaying the access history, each memory keeps
a running `activation` plus `last_decay_at`, updated atomically inside a
single SQL statement:

```
activation = min(max_activation,
                 activation * 0.5^(elapsed / half_life) + boost)
```

Boosts (defaults): query retrieval `1.0`, answer citation `1.5` (subsumes
the retrieval for the same access), direct read `0.25`.

**Spreading activation** (Collins & Loftus): a direct access also boosts
memories linked through `memory_relations`, at
`boost * hop_decay^hops / fan(previous node)` up to `max_hops` (default 2).
Fan normalization divides by the linking node's degree (the ACT-R fan
effect) so hub memories do not inflate every neighbour. Increments below
`min_propagated_increment` are dropped, which zeroes out distant memories.
`supersedes` relations never propagate (version lineage, not semantic
association). Propagated increments raise activation but never count as
accesses.

**Where accesses are recorded**: only three handler-level hooks feed the
bounded access channel — `/v1/query`, `/v1/query/global` (results =
retrieval, answer citations = citation), and the single-memory detail read
(direct read). List endpoints, TUI browsing, curation, provenance
verification, and the validation pipeline itself never count, so scoring
cannot feed back on itself. The channel drops batches when full: scoring is
an advisory signal and load-shedding is deliberate.

**Ranking integration**: search LEFT JOINs `memory_scores` and adds
`min(weight * ln(1 + activation), cap)` to the final score (defaults 0.3 /
1.2 — the same magnitude class as relation boosts, so activation can break
ties but not swamp lexical or semantic signals). Memories flagged
`needs_review` are *penalized* (default ×0.6), not excluded, and the flag is
surfaced on `QueryResult.needs_review`. The log cap saturates the
score → rank → access feedback loop.

## Volatility

Each score row keeps an EWMA of provenance change events per day. The
provenance re-verify sweep counts each source whose verification status
flips as one change event for the owning memory. Volatility shortens the
revalidation interval:

```
due = validated_at + min_revalidation_interval
                     / (1 + volatility * volatility_revalidation_factor)
```

so memories grounded in frequently changing files are re-checked more often
(update-risk TTL model).

## Validation pipeline

Selection is a scan, not a queue: decay-corrected activation ≥ threshold,
not flagged for review, past cooldown, and past the volatility-adjusted
revalidation time. Both entry points share it:

- the **background scheduler** (`run_reinforcement_scheduler`, default every
  15 min) validates up to `validation_batch_size` memories per cycle within
  a global `daily_validation_cap` of non-dry-run LLM runs per day;
- the **curator workflow**: after `/v1/curate`, due memories are reported in
  `CurateResponse.validation_due` and the scheduler is nudged to run
  promptly.

Each run has three stages:

1. **Evidence (deterministic, read-only)** — the memory version, its tags,
   sources with latest provenance status, 1-hop related memories, previous
   validation runs, and a bounded read-only `git log` over the memory's
   source paths since the last validation. This also builds the *reference
   allowlist*.
2. **Verdict (pluggable)** — a `VerdictProvider` returns strict JSON:
   verdict (`valid | partially_valid | outdated | ambiguous | unsupported`),
   confidence, reasons, stance-annotated evidence, `clarity_ok`, and
   optional proposed rewording/correction. The in-service provider uses the
   configured OpenAI-compatible LLM through the shared `llm.rs` helper with
   the full LLM audit trail. **Anti-hallucination**: any cited reference not
   present in the stage-1 allowlist fails the run. The trait is the seam
   for a future agent-CLI/worktree runner (mem-loops sandbox).
3. **Apply (pure policy)** —

   | Verdict | Outcome |
   |---|---|
   | valid, clear wording | `validated_at`/confidence refreshed |
   | valid, unclear, confidence ≥ `auto_apply_min_confidence`, `auto_apply_rewording` on | rewording applied as a **new immutable version** (fully audited, revertible) |
   | valid rewording below the bar / auto-apply off | proposal stored, review pending |
   | outdated / partially valid with a proposed correction | correction stored for review, `last_invalidated_at` stamped; content untouched until a human applies it |
   | ambiguous / unsupported / confidence < `needs_review_min_confidence` | `needs_review` flagged with a reason; **content is never modified** |

   Every completed run sets a `validation_cooldown` (default 7 d) so no
   memory can loop; failed runs get a short 24 h cooldown. Dry runs record
   the run, evidence, and `would_*` action plus the cooldown, and change
   nothing else.

## Storage

Memory rows are immutable versions, so all mutable state lives in separate
tables keyed by `canonical_id` (migration `0023_memory_reinforcement.sql`):

- `memory_scores` — activation, counters, volatility, and validation status
  metadata (`validated_at`, `validation_confidence`, `needs_review`,
  `needs_review_reason`, `last_invalidated_at`, `validation_cooldown_until`).
- `memory_access_events` — compact per-access log (analysis only, never
  replayed for scoring), pruned after `access_event_retention`.
- `memory_score_audit` — significant transitions only: threshold crossings,
  validation completions, needs_review set/resolved, volatility shifts,
  compaction sweeps. Never per-access.
- `memory_validation_runs` + `memory_validation_evidence` — full run record
  with verdict, action, proposed candidate, model, token/timing details, and
  stance-annotated evidence references.

A weekly compaction inside the scheduler deletes score rows that decayed to
noise and rows orphaned by retention pruning.

## API and CLI

- `GET /v1/projects/{project}/memory-scores?needs_review=&limit=` /
  `memory scores --project P [--needs-review]`
- `POST /v1/memory/{id}/validate` / `memory validate <id> [--dry-run|--execute]`
  (manual trigger: bypasses the threshold, respects the daily budget)
- `GET /v1/projects/{project}/validation-runs?review=pending` /
  `memory review list --project P [--all]`
- `POST /v1/validation-runs/{id}/review` / `memory review apply|reject <run-id>`

Timeline events use `ActivityKind::MemoryValidation`; LLM calls appear in
the existing LLM audit stream.

## Configuration and tuning

Everything lives under `[reinforcement]` in the config file. Defaults are
safe for normal development: scoring is on (deterministic, no cost),
validation is **off**, and when first enabled it starts in **dry-run**.

| Key | Default | Meaning |
|---|---|---|
| `enabled` | `true` | scoring, decay, propagation, ranking boost |
| `validation_enabled` | `false` | LLM validation (scheduler + curator trigger) |
| `validation_dry_run` | `true` | report-only validation |
| `direct_access_boost` | `1.0` | boost per query retrieval |
| `citation_boost` | `1.5` | boost when cited in an answer |
| `direct_read_boost` | `0.25` | boost per direct memory read |
| `half_life` | `30d` | activation halves after this idle time |
| `hop_decay` | `0.5` | per-hop propagation factor |
| `max_hops` | `2` | propagation reach |
| `fan_normalization` | `true` | divide by linking node's degree |
| `min_propagated_increment` | `0.05` | drop smaller propagated boosts |
| `max_activation` | `20.0` | activation ceiling |
| `validation_threshold` | `8.0` | activation that makes a memory due |
| `validation_cooldown` | `7d` | wait after any run |
| `min_revalidation_interval` | `14d` | base re-check interval |
| `volatility_revalidation_factor` | `4.0` | how strongly volatility shortens it |
| `volatility_ewma_alpha` | `0.3` | volatility smoothing |
| `scheduler_interval` | `15m` | background sweep cadence |
| `validation_batch_size` | `3` | validations per cycle |
| `daily_validation_cap` | `20` | non-dry-run LLM runs per day |
| `auto_apply_rewording` | `false` | allow automatic wording fixes |
| `auto_apply_min_confidence` | `0.85` | bar for automatic rewording |
| `needs_review_min_confidence` | `0.5` | below this, flag instead of acting |
| `activation_rank_weight` | `0.3` | search boost weight |
| `activation_rank_cap` | `1.2` | search boost ceiling |
| `needs_review_rank_penalty` | `0.6` | rank multiplier for flagged memories |
| `access_event_retention` | `30d` | access log retention |
| `access_channel_capacity` | `1024` | bounded hook channel |

Tuning guidance:

- **Threshold**: with the default boosts and 30 d half-life, a memory that
  is retrieved *and cited* 3–4 times within a month crosses 8.0;
  retrieval-only takes ~8 hits. Lower it to validate more of the long tail,
  raise it to focus LLM budget on the hottest memories.
- **Half-life** controls how fast old popularity fades. Shorter half-life =
  more recency-sensitive validation.
- **Cost**: worst-case LLM usage is `daily_validation_cap` runs/day; each
  run is one bounded chat call (≤1200 output tokens by default).
- **Rollout**: enable `validation_enabled` with `validation_dry_run = true`
  first and watch `memory review list --all`; then disable dry-run; only
  enable `auto_apply_rewording` once you trust the verdicts.

## Degradation and safety

- Relay nodes and offline (DuckDB) mode: the runtime is absent and every
  hook is a no-op; queries behave exactly as before.
- Concurrency: decay is computed inside the UPDATE from `last_decay_at`, so
  concurrent writers never race a read-modify-write cycle; the worker and
  scheduler run only on the primary.
- Every content change is a new immutable version — anything the system
  applies can be reverted through the existing history mechanisms.
