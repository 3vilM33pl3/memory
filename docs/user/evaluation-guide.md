# Beginner Guide To Evaluations

This guide walks through your first Memory Layer evaluation. It is written for
someone who wants practical proof that memory is helping an agent, without
starting from the full command reference.

An evaluation answers a simple question: does the same task work better with
Memory enabled than without it? The useful evidence comes from paired runs. Run
the same suite under two conditions, compare item by item, then inspect the
quality, cost, and latency differences.

For the full command reference, see [`memory eval`](cli/eval.md).

## Before You Start

You need:

- a running Memory Layer service for non-dry-run evaluations
- project memory to search against
- an LLM provider configured when using `--profile llm`
- PostgreSQL with `pgvector` when testing semantic retrieval

If you are running from an installed package, use commands like this:

```bash
memory eval doctor --suite evals/examples/memory-smoke --text
```

If you are developing inside this repository, use the dev binary instead:

```bash
cargo run --bin memory -- eval doctor --suite evals/examples/memory-smoke --text
```

## Step 1: Understand The Pieces

An eval suite is a directory with two files:

- `suite.toml`: suite name, project, profile defaults, repeat count, and label
  status
- `items.jsonl`: one evaluation item per line

The main item types are:

- `retrieval_qa`: checks whether the right memories are returned
- `grounded_answer`: checks whether an answer includes required facts and avoids
  forbidden claims
- `resume_quality`: checks whether a get-up-to-speed briefing covers the right
  topics
- `command_task`: checks whether a command succeeds

Start with the checked-in smoke suite:

```text
evals/examples/memory-smoke/
```

It is intentionally tiny. Use it to learn the workflow, not to make quality
claims.

## Step 2: Check The Suite

Run `doctor` before expensive runs:

```bash
memory eval doctor --suite evals/examples/memory-smoke --text
```

`doctor` checks that the suite can be parsed and that the environment is ready
for the selected work. Fix these problems before running an LLM-backed
evaluation:

- the service is unreachable
- the suite has invalid JSONL rows
- the suite is smaller than its configured `min_items`
- required provider or retrieval configuration is missing

## Step 3: Do A Safe Dry Run

Use an offline dry run first:

```bash
memory eval run \
  --suite evals/examples/memory-smoke \
  --condition full-memory \
  --profile offline \
  --dry-run \
  --text
```

This validates the suite and scoring path without spending provider tokens or
executing shell tasks. A passing dry run means the harness shape is valid. It
does not prove that Memory improves model behavior.

## Step 4: Run A Paired Evaluation

For useful evidence, compare a baseline against a Memory-backed condition:

```bash
memory eval run \
  --suite evals/suites/research-v1 \
  --condition no-memory \
  --condition full-memory \
  --profile llm \
  --repeat 5 \
  --text
```

The important conditions are:

- `no-memory`: no retrieval channel; answer and resume items use the configured
  LLM directly
- `lexical`: only lexical retrieval
- `semantic`: only semantic retrieval
- `graph`: only graph retrieval
- `full-memory`: lexical, semantic, graph, and relation boosts together

Use `--repeat` for provider-backed runs. Repeats make flaky LLM behavior visible
instead of hiding it behind one lucky or unlucky result.

Run artifacts are written under `target/memory-evals/`. Keep the generated JSON
files for release notes, research notes, or regression tracking.

## Step 5: Compare The Runs

After the paired run, compare the baseline artifact with the candidate artifact:

```bash
memory eval compare \
  --baseline target/memory-evals/no-memory.json \
  --candidate target/memory-evals/full-memory.json \
  --out target/memory-evals/comparison.json \
  --text
```

Use the actual artifact paths printed by your run if they differ from the
example above.

The comparison is paired by item id. That means each item is compared against
itself under both conditions, which is much stronger than comparing unrelated
aggregate scores.

## Step 6: Read The Result

Begin with these fields:

- success-rate delta: whether the candidate condition passed more items
- McNemar p-value: whether pass/fail changes look meaningful for paired items
- confidence interval: uncertainty around numeric metric deltas
- recall metrics: whether expected memories were retrieved
- forbidden hits: whether answers included claims they should avoid
- token delta: extra or saved provider tokens
- latency delta: extra or saved time

A good result is not just "full-memory won once". Prefer a result where the
candidate improves quality, the confidence interval is not obviously weak, and
the token/latency cost is acceptable for the use case.

## Step 7: Gate The Result

Use a gate policy when an evaluation becomes part of release discipline:

```bash
memory eval gate \
  --comparison target/memory-evals/comparison.json \
  --policy evals/gates/research-v1.toml \
  --text
```

The gate encodes the minimum acceptable evidence for a release or experiment.
If it fails, inspect the comparison instead of weakening the gate immediately.

## Step 8: Make Your Own Suite

Scaffold a starter suite from recent project memories:

```bash
memory eval scaffold --project memory --out evals/suites/my-first-suite --text
```

Review every generated item before trusting it:

- expected memory ids should point to memories that really answer the question
- required assertions should be specific and observable
- forbidden assertions should catch plausible wrong answers
- command tasks should be deterministic and safe

Keep `label_status = "draft"` while tuning labels. Only mark a suite reviewed
when the labels have been manually checked and the suite is large enough for the
claim you want to make.

## Common Beginner Mistakes

- Treating the smoke suite as proof. It is only a workflow example.
- Running only `full-memory`. You need a baseline such as `no-memory` to measure
  improvement.
- Using `--profile offline` as evidence. Offline mode is for CI-safe validation,
  not provider-backed behavior.
- Ignoring token and latency deltas. Better answers may still be too expensive
  for a workflow.
- Marking labels reviewed too early. Bad labels produce bad evidence.
- Changing retrieval or prompts while also changing the suite. Keep experiments
  controlled so the result explains one change at a time.

## What Counts As Strong Evidence?

For internal development, a small paired suite can catch regressions and show
direction. For external claims, use a held-out reviewed suite with enough items
to support statistics. The checked-in `research-v1` suite is a reviewed-seed
location for repeatable work, but it still needs to grow before it can support
publication-grade claims.
