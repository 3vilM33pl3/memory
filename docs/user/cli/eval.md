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
```

Each JSONL row is one eval item. Supported `eval_type` values are:

- `retrieval_qa`: asks a question and scores whether expected memories appear in the returned results.
- `grounded_answer`: asks a question and scores required/forbidden answer assertions.
- `resume_quality`: scores a get-up-to-speed briefing against required/forbidden topics.
- `command_task`: runs a shell command and scores the exit code.

## Commands

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
  --suite evals/examples/memory-smoke \
  --condition no-memory \
  --condition full-memory
```

Compare two run artifacts:

```bash
memory eval compare \
  --baseline target/memory-evals/no-memory.json \
  --candidate target/memory-evals/full-memory.json \
  --text
```

## Metrics

Retrieval items report Recall@k, MRR, nDCG, citation precision, semantic
candidate counts, and graph candidate counts. Grounded-answer and resume items
report assertion/topic recall and forbidden-hit counts. Comparisons are paired
by item id and include success-rate deltas, McNemar p-values, and bootstrap
confidence intervals for numeric metric deltas.

## Notes

The first version automates objective and deterministic evaluation. Subjective
human scoring and calibrated LLM-as-judge scoring should be added as separate
suite fields once there is a reviewed benchmark set.
