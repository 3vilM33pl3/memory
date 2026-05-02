# App Build Sequence Codex v1 Docker Evaluation Run

Date: 2026-05-02

## Summary

This run executed the Dockerized `app-build-sequence-codex-v1` suite end to
end. The suite asks Codex to build one product site through 20 ordered steps in
a persistent workspace, once without Memory and once with Memory enabled.

Both conditions completed successfully. The Memory-backed condition also proved
that Codex used the Memory service: every step required one harness-provided
Memory query, and the harness verified all 20 query evidence files before
accepting the run.

The run is a valid systems test for:

- Dockerized full-stack evaluation execution
- real Codex execution inside the eval container
- Memory service access from Codex
- per-step token capture from Codex JSON events
- sequence state preservation across 20 ordered build steps
- paired no-memory versus full-memory comparison output

It is not statistical proof that Memory improves output quality yet. This run
contains one paired sequence item and both conditions reached the same
deterministic score. It is best treated as a repeatable integration benchmark
and a foundation for larger repeated quality experiments.

## Command

```bash
docker compose -f evals/docker/app-build-sequence/compose.yml run --rm eval
```

The run used the default Docker evaluation model:

```text
MEMORY_EVAL_CODEX_MODEL=gpt-5.4-mini
MEMORY_EVAL_CODEX_WATCHDOG_SECONDS=300
```

## Artifacts

- Baseline run:
  `target/memory-evals-docker/app-build-sequence-codex-v1-no-memory-r0-20260502174957.json`
- Memory-backed run:
  `target/memory-evals-docker/app-build-sequence-codex-v1-full-memory-r0-20260502181352.json`
- Baseline step workspace:
  `target/memory-evals-docker/build-runs/app-build-sequence-codex-v1-memory-product-site-sequence-no-memory-r0-72e93df1c4d94cf7b0171cdce7f4ce36`
- Memory-backed step workspace:
  `target/memory-evals-docker/build-runs/app-build-sequence-codex-v1-memory-product-site-sequence-full-memory-r0-72e93df1c4d94cf7b0171cdce7f4ce36`
- Generated comparison JSON:
  `/tmp/app-build-sequence-codex-v1-comparison-20260502181352.json`

## Results

| Metric | No Memory | Full Memory | Delta |
| --- | ---: | ---: | ---: |
| Paired sequence items | 1 | 1 | 0 |
| Success rate | 100.0% | 100.0% | +0.0 pp |
| `total_score` | 1.000 | 1.000 | +0.000 |
| `score_commands_passed` | 20 | 20 | +0 |
| `score_commands_total` | 20 | 20 | +0 |
| `required_files_present` | 40 | 40 | +0 |
| `required_files_total` | 40 | 40 | +0 |
| `content_assertions_passed` | 22 | 22 | +0 |
| `content_assertions_total` | 22 | 22 | +0 |
| `memory_evidence_ok` | 1.000 | 1.000 | +0.000 |
| `memory_queries_required` | 0 | 20 | +20 |
| `memory_queries_verified` | 0 | 20 | +20 |
| `token_usage_ok` | 1.000 | 1.000 | +0.000 |
| Total tokens | 9,577,801 | 16,703,702 | +7,125,901 |
| Mean duration | 911,570 ms | 1,435,152 ms | +523,582 ms |

## Token Breakdown

| Token Field | No Memory | Full Memory | Delta |
| --- | ---: | ---: | ---: |
| Input tokens | 4,936,965 | 8,549,114 | +3,612,149 |
| Output tokens | 91,716 | 143,196 | +51,480 |
| Cache read tokens | 4,549,120 | 8,011,392 | +3,462,272 |
| Cache write tokens | 0 | 0 | +0 |
| Total tokens | 9,577,801 | 16,703,702 | +7,125,901 |

The Memory-backed run used more tokens because each step had additional
Memory context and required query evidence. That cost is expected for this
benchmark shape; later experiments should compare whether the extra context
improves quality, reduces rework, or improves consistency on harder tasks.

## Step Coverage

| Step | No Memory | Full Memory | Verified Memory Queries |
| --- | --- | --- | ---: |
| `hero` | pass | pass | 1 / 1 |
| `navigation` | pass | pass | 1 / 1 |
| `feature-grid` | pass | pass | 1 / 1 |
| `workflow` | pass | pass | 1 / 1 |
| `query-panel` | pass | pass | 1 / 1 |
| `graph-section` | pass | pass | 1 / 1 |
| `distributed-agents` | pass | pass | 1 / 1 |
| `activity-ledger` | pass | pass | 1 / 1 |
| `evaluation-proof` | pass | pass | 1 / 1 |
| `docker-eval` | pass | pass | 1 / 1 |
| `token-cost` | pass | pass | 1 / 1 |
| `get-up-to-speed` | pass | pass | 1 / 1 |
| `memory-types` | pass | pass | 1 / 1 |
| `curation` | pass | pass | 1 / 1 |
| `privacy-local` | pass | pass | 1 / 1 |
| `cli-tui` | pass | pass | 1 / 1 |
| `architecture` | pass | pass | 1 / 1 |
| `interview-proof` | pass | pass | 1 / 1 |
| `final-polish` | pass | pass | 1 / 1 |
| `accessibility` | pass | pass | 1 / 1 |

## Comparison Command

```bash
target/debug/memory eval compare \
  --baseline target/memory-evals-docker/app-build-sequence-codex-v1-no-memory-r0-20260502174957.json \
  --candidate target/memory-evals-docker/app-build-sequence-codex-v1-full-memory-r0-20260502181352.json \
  --out /tmp/app-build-sequence-codex-v1-comparison-20260502181352.json \
  --text
```

The comparison output was:

```text
full-memory [llm] vs no-memory [llm] (1 paired item(s))
success: 100.0% -> 100.0% (+0.0 pp), McNemar p=1.0000
tokens: 9577801 -> 16703702 (+7125901), mean duration: 911570.0ms -> 1435152.0ms (+523582.0ms)
memory_queries_required: 0.000 -> 20.000 (+20.000, 95% CI +20.000..+20.000)
memory_queries_verified: 0.000 -> 20.000 (+20.000, 95% CI +20.000..+20.000)
score_commands_passed: 20.000 -> 20.000 (+0.000, 95% CI +0.000..+0.000)
token_usage_ok: 1.000 -> 1.000 (+0.000, 95% CI +0.000..+0.000)
total_score: 1.000 -> 1.000 (+0.000, 95% CI +0.000..+0.000)
```

## Interpretation

The important result is not that Memory improved the final score in this single
run; it did not. The important result is that the evaluation harness can now run
a realistic, stateful, 20-step Codex build in Docker while verifying that Memory
was actually used and while capturing token cost for both conditions.

That closes a major validity gap in earlier build simulations. Future benchmark
work can now focus on harder tasks, multiple repeats, quality-sensitive scoring,
and statistical comparison rather than first proving that Codex had real Memory
access.

## Follow-Up Work

- Run the 10-step functionality-change follow-up suite:
  `evals/suites/app-build-functionality-sequence-codex-v1`.
- Run multiple repeats to estimate variance and confidence intervals.
- Add harder project-building tasks where prior context should affect output
  quality, consistency, or rework.
- Add richer scoring beyond deterministic file/content assertions.
- Track wall-clock latency, token cost, and quality tradeoffs across repeated
  runs.
- Preserve comparison reports in a repo-owned artifact directory rather than
  `/tmp` once Docker-generated target files are no longer root-owned.
