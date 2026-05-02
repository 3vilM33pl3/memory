# Automated Evaluation

Memory evaluation is built around paired ablations: run the same suite under
two conditions, then compare item-by-item results. This avoids misleading
aggregate comparisons and makes statistical tests meaningful.

## Architecture

- `mem-eval` owns suite parsing, result schemas, scoring, and statistics.
- `memory eval` owns service calls, command execution, dry-run behavior, and
  artifact writing.
- Eval artifacts are JSON files under `target/memory-evals/`; they are not
  stored in Postgres in the first implementation. Keep official artifacts with
  release or research notes because they include suite checksum, run group,
  repeat index, latency, and token usage.

## Conditions

Supported condition labels are `no-memory`, `lexical`, `semantic`, `graph`, and
`full-memory`. Retrieval-backed conditions request explicit query retrieval
modes, so `lexical`, `semantic`, and `graph` are isolated instead of relying on
the currently configured full service path.

The `no-memory` condition has no retrieval channel, so retrieval items receive
zero retrieval scores rather than being skipped. For `grounded_answer` and
`resume_quality` items, `memory eval run` calls the configured
OpenAI-compatible `[llm]` client directly and scores the resulting text without
Memory retrieval, project timeline, or resume context. This keeps provider I/O
in `mem-cli`; `mem-eval` remains responsible for file formats, scoring, and
statistics.

Memory-backed `resume_quality` conditions currently use the deterministic
`up-to-speed` briefing (`include_llm_summary = false`) so the default comparison
isolates Memory evidence and timeline value before adding another LLM summary
step.

## Profiles And Repeats

The default official profile is `llm`. It uses the configured provider for the
plain no-memory baseline and Memory-backed answer synthesis. Use
`--profile offline` for CI-safe validation that must not depend on external
network calls.

Use `--repeat` for provider-backed runs. Repeats are written as separate
immutable artifacts sharing one run group id so flaky LLM behavior can be
inspected instead of hidden.

## Statistics

`memory eval compare` pairs results by item id and reports:

- success-rate delta
- McNemar exact p-value for binary success changes
- per-metric baseline/candidate means
- paired bootstrap 95% confidence intervals for metric deltas
- token and latency deltas for paired items

This is enough to support claims such as “full memory improved Recall@k by 0.18
with a 95% CI of 0.09..0.26” once the suite has enough held-out items.

Eval results also preserve `duration_ms` and provider `TokenUsage` when the
called path reports it. Those fields support cost and latency comparisons across
plain LLM and Memory-backed conditions without changing the paired statistics
format.

## Benchmark Quality

The harness can automate execution and statistics, but benchmark validity still
depends on reviewed item labels. Keep a held-out suite that is not used while
tuning retrieval or prompts. The checked-in `research-v1` suite is a seed suite;
expand it to 100+ reviewed held-out items before making external claims.

Use `memory eval doctor` before expensive runs and `memory eval gate` before
release claims. Gate policies live under `evals/gates/` and should encode the
minimum evidence expected for the release train.
