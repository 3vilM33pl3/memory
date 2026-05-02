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

The query API fields used by eval are:

- `retrieval_mode`
  - `lexical`: lexical candidates only
  - `semantic`: semantic candidates only
  - `graph`: graph candidates only
  - `full-memory`: lexical, semantic, graph, and relation boosts
- `answer_mode`
  - `auto`: normal service behavior
  - `deterministic`: keep deterministic synthesis and skip LLM answer enrichment
  - `llm`: force LLM answer enrichment for Memory-backed grounded-answer evals

These fields are optional. Normal user queries omit them and keep full-memory,
auto-answer behavior.

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

Suite manifests can include `suite_version`, `label_status`, `fixture`,
`default_profile`, `default_repeats`, and `min_items`. Treat `label_status =
"reviewed"` as a benchmark quality claim: only set it after expected memories,
assertions, forbidden assertions, and task commands have been reviewed.

## Statistics

`memory eval compare` pairs results by item id and reports:

- success-rate delta
- McNemar exact p-value for binary success changes
- per-metric baseline/candidate means
- paired bootstrap 95% confidence intervals for metric deltas
- token and latency deltas for paired items

Run artifacts include `run_group_id`, `repeat_index`, `suite_checksum`,
`fixture_checksum`, `config_fingerprint` when supplied, `git_head`,
`service_version`, per-item `duration_ms`, and per-item token usage. These fields
are intended to make a published result reproducible enough to rerun against the
same code, suite, and provider configuration.

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
