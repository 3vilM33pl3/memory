# Memory Improvement Benchmark

`evals/suites/memory-improvement-v1` measures whether Memory improves agent
work, not just whether the eval harness runs.

The suite combines four evidence types:

- `retrieval_qa` checks whether the right memories, tags, and source files are
  returned.
- `grounded_answer` checks whether answers include required facts and avoid
  stale or unsafe claims.
- `resume_quality` checks whether a new agent receives the right project state.
- `agent_build_sequence` runs a 20-step Codex continuity task against one copied
  app fixture and scores each step.

## Reasoning Taxonomy

The suite uses the taxonomy from
[An Incomplete Loop: Deductive, Inductive, and Abductive Learning in Large Language Models](https://arxiv.org/abs/2404.03028v1).
The paper is useful here because it treats deductive, inductive, and abductive
reasoning as separable behaviors rather than one generic reasoning score.

Benchmark mapping:

- `deductive`: apply an explicit remembered rule, decision, or constraint.
- `inductive`: infer a convention from remembered examples.
- `abductive`: select the best explanation or diagnosis from remembered
  evidence.

Every item can carry `reasoning_mode`, `memory_capability`, `difficulty`, and
`claim`. `memory eval compare` groups results by `eval_type`, `reasoning_mode`,
and `memory_capability`, so a high aggregate score cannot hide that Memory only
helped one kind of task.

## Running It

Use Docker for an isolated full-stack run:

```bash
MEMORY_EVAL_REPEAT=5 \
docker compose -f evals/docker/memory-improvement/compose.yml run --rm eval
```

Add optional hybrid judging:

```bash
MEMORY_EVAL_LLM_JUDGE=1 \
MEMORY_EVAL_REPEAT=5 \
docker compose -f evals/docker/memory-improvement/compose.yml run --rm eval
```

The Docker stack starts Postgres with pgvector, the Memory service,
deterministic seed memories, graph extraction, and the eval runner. The seed
facts are not in the fixture repository, so the no-memory condition cannot
discover them by reading files.

## Comparing And Reporting

Compare repeated run artifacts:

```bash
memory eval compare \
  --baseline 'target/memory-evals/*no-memory*.json' \
  --candidate 'target/memory-evals/*full-memory*.json' \
  --out target/memory-evals/memory-improvement-v1-comparison.json \
  --text
```

Render a Markdown report:

```bash
memory eval report \
  --comparison target/memory-evals/memory-improvement-v1-comparison.json \
  --markdown \
  --out target/memory-evals/memory-improvement-v1-report.md
```

Read the report by group first. A convincing result should show positive
full-memory deltas for retrieval, grounded answers, resume quality, and the
coding sequence, with no severe regression in any reasoning mode.

## Metrics To Watch

- `tag_recall_at_k` and `file_recall_at_k`: prove retrieval found the intended
  class of memory and source, even when UUIDs are generated during seeding.
- `memory_queries_verified`: proves the Codex agent really queried Memory in
  memory-enabled sequence steps.
- `total_score`: deterministic pass/fail score for build items and sequence
  steps.
- `token_delta`: provider token cost paid by the candidate condition.
- `cost_adjusted_success_delta_per_1k_tokens`: quality improvement normalized by
  additional token spend.
- `judge_*`: optional diagnostic quality metrics when `--llm-judge` is enabled.

## Interpretation

This benchmark is stronger than the earlier app-build suites because it has
hidden memory-only facts, grouped reasoning modes, retrieval expectations that
affect success, step-level sequence scoring, and token-aware reporting.

It is still not publication-grade proof by itself. For external claims, grow the
suite with more held-out items, run enough repeats for stable confidence
intervals, and preserve every run artifact.
