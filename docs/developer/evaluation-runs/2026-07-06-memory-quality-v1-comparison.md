# Memory Quality v1 — 2026-07-06 run vs 2026-07-05 baseline

Offline (deterministic) profile, dev stack (`target/debug/memory`, commit
`1e88a22` working tree), shared dev database. Suite grew from 24 to 26 items
since the baseline (two consolidation items) and the fixture gained the
ingest cluster plus a seeded insight.

## Headline

| Run | Paired | Baseline (no-memory) | Candidate (full-memory) | Gate |
|---|---|---|---|---|
| 2026-07-05 | 24 | 0.167 | **0.708** (17/24) | fail (adversarial floor) |
| 2026-07-06 | 26 | 0.154 | **0.462** (12/26) | fail (overall + adversarial floors) |

| Group | 07-05 | 07-06 |
|---|---|---|
| retrieval_qa | 9/9 | **10/10** (incl. new insight big-picture item) |
| grounded_answer | 8/8 | **1/9** ← regression |
| adversarial_stale | 0/7 | 1/7 (`as-codename-prefer-fresh` now passes) |

## Root cause of the grounded-answer collapse

Every failing grounded item now answers from the newly seeded **ingest
cluster** regardless of the question ("What flush window does the sync
engine use?" → "Ingest retries transient failures…"). Score explanations
show a compound cause, none of it a scoring-code change:

1. **Provenance decay asymmetry.** The background provenance sweep has since
   verified the fixture memories' *fictional* `docs/quality/*.md` sources
   and marked 14 of them `missing_file`, halving their rank (×0.50 decay).
   The four ingest memories seeded on 07-06 are still `unverified` — no
   decay. The correct sync-engine memory has the highest semantic
   similarity (0.70) and 78% term overlap for its own question, yet ranks
   #6 after the halving.
2. **Embeddings arrived.** On 07-05 the fixture had no chunk embeddings
   (lexical-only retrieval); all 22 memories now have them, so semantic
   scores reshape the ranking.
3. **Cluster self-amplification.** The ingest members are densely
   inter-related (relation boost +0.56 each), lifting the whole cluster on
   any weak-lexical query.

Interpretation: the ranking behaved as designed — missing evidence is
down-ranked, relations boost — but the *fixture* is not provenance-stable:
fictional file paths rot by design once the reverify sweep runs. Either the
fixture should seed real (or `note`-kind) sources for grounded items, or
grounded items need re-seeding freshness parity. Tracked as suite
maintenance; the canary did its job by surfacing the interaction.

Also observed: all fixture memories sit at the activation cap (20.0) from
repeated eval queries — uniform, so no rank distortion, but eval traffic
feeding activation is worth remembering when reading explanations.

## research-v1 (fresh datapoint, offline)

No stored prior run exists (the May 2026 published results used the Docker
LLM harness). Ran no-memory vs **semantic** because full-memory is currently
blocked: graph retrieval on the `memory` project takes 38–46 s (was ~14 s
on 07-05), exceeding the 30 s client timeout — a real latency regression to
investigate separately (ANALYZE did not help; the code-graph candidate scan
appears to have outgrown its query plan).

Result: baseline 0.25 → semantic 0.42 (+0.17, 12 paired), gate **pass**
(delta policy). retrieval_qa items score 0 locally because the
`memory-research-v1` fixture corpus is not seeded in the dev database (the
suite targets the Docker harness); command_task 3/3, grounded_answer 0 → 0.5.

## memory-improvement-v1

Not rerun: it requires the Docker eval stack plus the codex CLI and LLM
budget, and seeding its benchmark facts locally would pollute the real
`memory` project. The checked-in 2026-05-03 full run remains the reference
result.

## Follow-ups

1. Investigate the graph-retrieval latency regression on the `memory`
   project (38–46 s per query; blocks full-memory evals and slows real use).
2. Make the memory-quality fixture provenance-stable (note-kind sources for
   grounded items, or expected-file items pointed at real paths).
3. Once fixed, re-run and re-baseline; the gate's adversarial floor remains
   intentionally red until the synthesis refusal work lands.
