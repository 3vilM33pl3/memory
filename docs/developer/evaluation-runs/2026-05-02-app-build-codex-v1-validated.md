# App Build Codex v1 Validated Evaluation Run

Date: 2026-05-02

## Summary

This run validates the real Codex-backed app build simulation suite after
tightening Memory evidence checks.

The key difference from the earlier invalidated run is that `full-memory` items
now must prove real Memory access. For every required Memory question, the
agent must run `./.memory-eval/query-memory`; the harness verifies the helper
status file and raw query JSON before the item can pass.

Final result: both conditions passed all three app-build tasks, and the
`full-memory` condition verified all required Memory queries.

## Commands

Full-memory validation:

```bash
MEMORY_EVAL_CODEX_MODEL=gpt-5.4-mini \
MEMORY_EVAL_CODEX_WATCHDOG_SECONDS=300 \
cargo run --bin memory -- eval run \
  --suite evals/suites/app-build-codex-v1 \
  --condition full-memory \
  --profile llm \
  --repeat 1 \
  --text
```

No-memory baseline:

```bash
MEMORY_EVAL_CODEX_MODEL=gpt-5.4-mini \
MEMORY_EVAL_CODEX_WATCHDOG_SECONDS=300 \
cargo run --bin memory -- eval run \
  --suite evals/suites/app-build-codex-v1 \
  --condition no-memory \
  --profile llm \
  --repeat 1 \
  --text
```

Comparison:

```bash
cargo run --quiet --bin memory -- eval compare \
  --baseline target/memory-evals/app-build-codex-v1-no-memory-r0-20260502162310.json \
  --candidate target/memory-evals/app-build-codex-v1-full-memory-r0-20260502160735.json \
  --text
```

## Artifacts

- Baseline run:
  `target/memory-evals/app-build-codex-v1-no-memory-r0-20260502162310.json`
- Candidate run:
  `target/memory-evals/app-build-codex-v1-full-memory-r0-20260502160735.json`
- Full-memory workspaces:
  `target/memory-evals/build-runs/app-build-codex-v1-*-full-memory-r0-46d9d582920e4f45a4ba8037ac29f18e`
- No-memory workspaces:
  `target/memory-evals/build-runs/app-build-codex-v1-*-no-memory-r0-1afc5798023a41f6b315e79364453787`

## Results

| Metric | No Memory | Full Memory | Delta |
| --- | ---: | ---: | ---: |
| Paired items | 3 | 3 | 0 |
| Success rate | 100.0% | 100.0% | +0.0 pp |
| `total_score` | 1.000 | 1.000 | +0.000 |
| `memory_evidence_ok` | 1.000 | 1.000 | +0.000 |
| `memory_queries_required` | 0.000 | 3.000 | +3.000 |
| `memory_queries_verified` | 0.000 | 3.000 | +3.000 |
| Score commands passed | 1.000 | 1.000 | +0.000 |
| Required files present | 3.000 | 3.000 | +0.000 |
| Mean duration | 299950.0 ms | 300418.3 ms | +468.3 ms |
| Tokens captured by eval harness | 0 | 0 | 0 |

Codex subprocess token usage is not yet fed back into `EvalItemResult`, so the
token count remains `0` even though Codex used model tokens.

## Per-Task Outcome

| Task | No Memory | Full Memory | Verified Memory Queries |
| --- | --- | --- | ---: |
| `memory-landing-page` | pass | pass | 3 / 3 |
| `evaluation-dashboard` | pass | pass | 3 / 3 |
| `agent-onboarding-page` | pass | pass | 3 / 3 |

## Interpretation

This run is a valid harness-level proof that Codex can use the local Memory
service during the build simulation and that the harness rejects unverified
Memory evidence. It is not yet statistical proof that Memory improves output
quality, because this single paired run has equal deterministic task scores in
both conditions. Use multiple repeats and harder tasks before making a quality
improvement claim.
