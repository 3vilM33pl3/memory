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
- `agent_build_task`: copies a fixture app or website, lets an agent modify it,
  then scores the finished workspace
- `agent_build_sequence`: runs many ordered agent-building steps against the
  same copied workspace and scores the accumulated app

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

You can also try the build-simulation smoke suite. It uses a fake deterministic
agent, so it proves the fixture-copying, agent-command, and scoring path without
model cost:

```bash
memory eval run \
  --suite evals/examples/app-build-smoke \
  --condition no-memory \
  --condition full-memory \
  --profile offline \
  --text
```

When you want the full token-spending version, run the Codex-backed suite:

```bash
MEMORY_EVAL_CODEX_MODEL=gpt-5.4-mini \
memory eval run \
  --suite evals/suites/app-build-codex-v1 \
  --condition no-memory \
  --condition full-memory \
  --profile llm \
  --repeat 1 \
  --text
```

That suite runs real `codex exec` agents against static app fixtures. The
`full-memory` condition receives required Memory questions and must run the
generated `./.memory-eval/query-memory` helper for each question. The harness
verifies the helper's raw query JSON before accepting the item. The `no-memory`
condition is forbidden from using Memory and fails if Memory evidence artifacts
appear.

For a longer software-building test, use the Dockerized sequence suite:

```bash
docker compose -f evals/docker/app-build-sequence/compose.yml run --rm eval
```

This starts PostgreSQL with pgvector, starts the Memory service, seeds
deterministic project memories, and runs the 20-step Codex app-build sequence
under `no-memory` and `full-memory`. Each step keeps the previous workspace
state, so the run tests continuity across a realistic product build rather than
isolated prompt answers. Use `docker compose -f evals/docker/app-build-sequence/compose.yml down -v`
before a clean rerun if you want to reset the database volume.

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

For software-building proof, use `agent_build_task`. It gives both conditions
the same starter project, same prompt, same model, same timeout, and same
deterministic checker. The no-memory run is told not to use Memory and has
common Memory environment variables cleared; the full-memory run is told to use
Memory where useful. This makes the result easier to explain than a pure Q&A
test: Memory is valuable if the agent ships more of the requested app, passes
more checks, or needs fewer interventions under the same budget.

Use `agent_build_sequence` when the claim is about long-running development.
The sequence runner preserves one workspace across ordered steps, verifies
Memory helper calls step by step, and aggregates Codex token usage from
`codex-events.jsonl`. That lets you inspect whether Memory changed quality,
continuity, latency, and token cost across the whole build.

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
