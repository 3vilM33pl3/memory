# Research V1 Dev Evaluation: 2026-05-02

This was a development-directory evaluation run for `evals/suites/research-v1`
using the repository dev binary.

## Run Configuration

- Linear issue: `3VI-572`
- Command profile: `llm`
- Conditions: `no-memory` baseline vs `full-memory` candidate
- Requested repeats: 5
- Completed paired repeats: 3
- Token guard: `--max-cost 100000`
- Stop reason: `eval token budget exceeded: used 132247 tokens, limit 100000`
- Run directory: `target/memory-evals/research-v1-dev-20260502T103807Z`
- Suite checksum: `3681d9e5fcfcb3c59406daf5c171bf0e4b641a8c88b9072187e4412223e49425`
- Fixture checksum: `memory-research-v1`
- Run group: `c054ef23-693f-44d7-bf11-8ebb29a05b1b`
- Git head recorded in artifacts: `ec76275`
- Suite label status: `draft`

The suite passed `memory eval doctor`, with the expected warning that labels are
still draft. An offline dry-run also passed before the provider-backed run.

## Artifacts

Completed run artifacts:

- `memory-research-v1-no-memory-r0-20260502103843.json`
- `memory-research-v1-full-memory-r0-20260502104118.json`
- `memory-research-v1-no-memory-r1-20260502104139.json`
- `memory-research-v1-full-memory-r1-20260502104408.json`
- `memory-research-v1-no-memory-r2-20260502104428.json`
- `memory-research-v1-full-memory-r2-20260502104703.json`

Partial extra artifact:

- `memory-research-v1-no-memory-r3-20260502104728.json`

Comparison artifacts:

- `comparison-r0.json`
- `comparison-r1.json`
- `comparison-r2.json`

## Results

| Repeat | Gate | Success Delta | McNemar p | Token Delta | Mean Latency Delta |
| --- | --- | ---: | ---: | ---: | ---: |
| r0 | pass | +0.0 pp | 1.0000 | +23,926 | +12,404.5 ms |
| r1 | pass | +8.3 pp | 1.0000 | +30,415 | +13,176.7 ms |
| r2 | pass | +8.3 pp | 1.0000 | +32,115 | +13,852.2 ms |

Across the completed pairs, `full-memory` improved retrieval metrics
consistently:

- `recall_at_k`: `0.000 -> 1.000`
- `mrr`: `0.000 -> 1.000`
- `ndcg`: `0.000 -> 1.000`
- `topic_recall`: `0.500 -> 0.750`

The candidate also showed tradeoffs:

- token usage increased substantially on every completed repeat
- mean latency increased by roughly 12.4s to 13.9s per item
- `citation_precision` reported `1.000 -> 0.000`
- `confidence` decreased on every completed repeat
- `assertion_recall` was flat or worse

## Interpretation

This run is useful development evidence, not publication-grade proof. The suite
has only 12 items and is still marked `draft`. The result does show that the
`full-memory` retrieval path is active and materially changes retrieval metrics
relative to `no-memory`, but the token, latency, citation, and confidence
tradeoffs need investigation before treating this as a release-quality win.

The next useful evaluation step is to either raise the token cap for a complete
five-repeat run or reduce per-item cost before rerunning the same paired setup.
