# `memory eval`

Run automated evaluation suites that measure whether Memory improves retrieval,
grounding, resume quality, task success, latency, and token cost.

The command is intentionally file-based: suites live in the repository and run
results are written as immutable JSON artifacts under `target/memory-evals/`.
That makes comparisons reproducible and easy to attach to release notes.

## Suite Format

Create a directory containing `suite.toml` and `items.jsonl`:

```toml
name = "memory smoke"
project = "memory"
items = "items.jsonl"
suite_version = "1"
label_status = "draft"
default_profile = "llm"
default_repeats = 5
```

Each JSONL row is one eval item. Supported `eval_type` values are:

- `retrieval_qa`: asks a question and scores whether expected memories appear in the returned results.
- `grounded_answer`: asks a question and scores required/forbidden answer assertions.
- `resume_quality`: scores a get-up-to-speed briefing against required/forbidden topics.
- `command_task`: runs a shell command and scores the exit code.

## Commands

Check a suite and environment before spending provider tokens:

```bash
memory eval doctor --suite evals/suites/research-v1 --text
```

Scaffold a starter suite from recent memories:

```bash
memory eval scaffold --project memory --out evals/suites/memory-smoke
```

Preview a run without LLM calls or shell task execution:

```bash
memory eval run --suite evals/examples/memory-smoke --condition full-memory --dry-run --text
```

Run paired conditions:

```bash
memory eval run \
  --suite evals/suites/research-v1 \
  --condition no-memory \
  --condition full-memory \
  --profile llm \
  --repeat 5 \
  --fail-on-unreviewed-labels
```

The `no-memory` condition uses the configured OpenAI-compatible `[llm]` client
directly for `grounded_answer` and `resume_quality` items. It does not call
Memory retrieval, project timeline, or resume endpoints for those item types.
`retrieval_qa` items still receive zero retrieval scores under `no-memory`
because there is intentionally no retrieval channel.

Use `--profile offline` for CI-safe checks. Offline runs do not call the direct
no-memory LLM baseline and can be used to validate suite shape and scoring.
Official evidence runs should use the default `llm` profile.

`lexical`, `semantic`, `graph`, and `full-memory` now request explicit
retrieval modes from the query API. This means condition names are enforced by
the backend instead of merely recorded in artifacts.

Compare two run artifacts:

```bash
memory eval compare \
  --baseline target/memory-evals/no-memory.json \
  --candidate target/memory-evals/full-memory.json \
  --text
```

Gate a comparison before release:

```bash
memory eval gate \
  --comparison target/memory-evals/comparison.json \
  --policy evals/gates/research-v1.toml \
  --text
```

## Metrics

Retrieval items report Recall@k, MRR, nDCG, citation precision, semantic
candidate counts, and graph candidate counts. Grounded-answer and resume items
report assertion/topic recall and forbidden-hit counts. Comparisons are paired
by item id and include success-rate deltas, McNemar p-values, and bootstrap
confidence intervals for numeric metric deltas.

Run artifacts include suite checksums, run group IDs, repeat indexes,
`duration_ms`, and provider token usage when the underlying LLM response reports
it. This lets paired runs compare quality, latency, and token volume for plain
LLM behavior versus Memory-backed behavior.

## Notes

The checked-in `research-v1` suite is a reviewed-seed location, not yet a
publication-grade benchmark. Expand it to 100+ held-out reviewed items before
making external statistical claims.
