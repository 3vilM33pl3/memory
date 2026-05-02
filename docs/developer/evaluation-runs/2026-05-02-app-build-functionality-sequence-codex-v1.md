# App Build Functionality Sequence Codex v1 Evaluation Run

Date: 2026-05-02

## Summary

This run exercised the new `app-build-functionality-sequence-codex-v1` suite in
Docker. The suite starts from the saved final full-memory product site fixture
and asks Codex to add 10 dependency-free browser functionality changes.

The no-memory condition passed all 10 steps. The full-memory condition verified
all 10 required Memory queries and captured token usage for all 10 steps, but
the recorded run failed one score command on step 9. Inspection showed this was
a harness false negative: the generated page documented Escape-to-close in
HTML and used native dialog behavior, while the score command only looked for
`Escape` in `scripts/app.js`.

The scoring rule was corrected after the run to accept documented Escape
behavior in either `index.html` or `scripts/app.js`.

## Commands

```bash
docker compose -f evals/docker/app-build-sequence/compose.yml build eval

MEMORY_EVAL_SUITE=/workspace/evals/suites/app-build-functionality-sequence-codex-v1 \
docker compose -f evals/docker/app-build-sequence/compose.yml run --rm eval
```

## Artifacts

- Baseline run:
  `target/memory-evals-docker/app-build-functionality-sequence-codex-v1-no-memory-r0-20260502193800.json`
- Memory-backed run:
  `target/memory-evals-docker/app-build-functionality-sequence-codex-v1-full-memory-r0-20260502200338.json`
- Baseline workspace:
  `target/memory-evals-docker/build-runs/app-build-functionality-sequence-codex-v1-memory-product-site-functionality-sequence-no-memory-r0-a19518fc3b2746df8ada44d100b2def2`
- Memory-backed workspace:
  `target/memory-evals-docker/build-runs/app-build-functionality-sequence-codex-v1-memory-product-site-functionality-sequence-full-memory-r0-a19518fc3b2746df8ada44d100b2def2`
- Comparison JSON:
  `/tmp/app-build-functionality-sequence-codex-v1-comparison-20260502200338.json`

## Results

| Metric | No Memory | Full Memory | Delta |
| --- | ---: | ---: | ---: |
| Paired sequence items | 1 | 1 | 0 |
| Success rate in recorded run | 100.0% | 0.0% | -100.0 pp |
| `total_score` in recorded run | 1.000 | 0.000 | -1.000 |
| `score_commands_passed` | 10 | 9 | -1 |
| `score_commands_total` | 10 | 10 | +0 |
| `required_files_present` | 30 | 30 | +0 |
| `required_files_total` | 30 | 30 | +0 |
| `content_assertions_passed` | 22 | 22 | +0 |
| `content_assertions_total` | 22 | 22 | +0 |
| `memory_evidence_ok` | 1.000 | 1.000 | +0.000 |
| `memory_queries_required` | 0 | 10 | +10 |
| `memory_queries_verified` | 0 | 10 | +10 |
| `token_usage_ok` | 1.000 | 1.000 | +0.000 |
| Total tokens | 11,102,465 | 20,555,371 | +9,452,906 |
| Mean duration | 986,711 ms | 1,537,678 ms | +550,967 ms |

## Token Breakdown

| Token Field | No Memory | Full Memory | Delta |
| --- | ---: | ---: | ---: |
| Input tokens | 5,655,431 | 10,391,863 | +4,736,432 |
| Output tokens | 124,922 | 178,228 | +53,306 |
| Cache read tokens | 5,322,112 | 9,985,280 | +4,663,168 |
| Cache write tokens | 0 | 0 | +0 |
| Total tokens | 11,102,465 | 20,555,371 | +9,452,906 |

## Step Coverage

| Step | No Memory | Full Memory | Verified Memory Queries |
| --- | --- | --- | ---: |
| `feature-search` | pass | pass | 1 / 1 |
| `section-filters` | pass | pass | 1 / 1 |
| `token-estimator` | pass | pass | 1 / 1 |
| `eval-toggle` | pass | pass | 1 / 1 |
| `memory-type-tabs` | pass | pass | 1 / 1 |
| `graph-inspector` | pass | pass | 1 / 1 |
| `onboarding-checklist` | pass | pass | 1 / 1 |
| `summary-export` | pass | pass | 1 / 1 |
| `command-palette` | pass | score false negative | 1 / 1 |
| `final-functionality-polish` | pass | pass | 1 / 1 |

## Follow-Up

The suite definition was updated so step 9 checks for `Escape` in either
`index.html` or `scripts/app.js`. The generated full-memory workspace passes
the corrected score command:

```bash
cd target/memory-evals-docker/build-runs/app-build-functionality-sequence-codex-v1-memory-product-site-functionality-sequence-full-memory-r0-a19518fc3b2746df8ada44d100b2def2/workspace
sh scripts/check.sh
node --check scripts/app.js
grep -q 'FUNC-09-COMMAND-PALETTE' index.html
grep -q 'FUNC-09-COMMAND-PALETTE' scripts/app.js
grep -qi 'Escape' index.html scripts/app.js
```

Rerun the full suite when a fresh all-green artifact is needed. The recorded
run is still useful evidence that the new follow-up path works end to end:
Codex modified the saved website, Memory access was verified on every
full-memory step, and token usage was captured for both conditions.
