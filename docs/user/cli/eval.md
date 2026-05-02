# `memory eval`

Run automated evaluation suites that measure whether Memory improves retrieval,
grounding, resume quality, task success, latency, and token cost.

If you are new to evaluations, start with the
[Beginner Guide To Evaluations](../evaluation-guide.md). This page is the CLI
reference.

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
fixture = "memory-research-v1"
default_profile = "llm"
default_repeats = 5
min_items = 100
```

Suite manifest fields:

- `suite_version` is a human-managed suite format/version label.
- `label_status` should be `draft` while labels are still being tuned and `reviewed` before using `--fail-on-unreviewed-labels`.
- `fixture` names the fixture/corpus used for reproducibility.
- `default_profile` is usually `llm` for official evidence and `offline` for CI fixtures.
- `default_repeats` is the minimum repeat count `memory eval run` should use for that suite.
- `min_items` is checked by `memory eval doctor` so undersized suites are visible before expensive runs.

Each JSONL row is one eval item. Supported `eval_type` values are:

- `retrieval_qa`: asks a question and scores whether expected memories appear in the returned results.
- `grounded_answer`: asks a question and scores required/forbidden answer assertions.
- `resume_quality`: scores a get-up-to-speed briefing against required/forbidden topics.
- `command_task`: runs a shell command and scores the exit code.
- `agent_build_task`: copies a fixture project, runs a noninteractive agent command, then scores the resulting workspace with deterministic checks.

`agent_build_task` rows are for app/website build simulations. They use a
workspace copy under `target/memory-evals/build-runs/`, so the source fixture is
never edited. Minimal fields:

```json
{
  "eval_type": "agent_build_task",
  "id": "landing-page-build",
  "project": "memory",
  "prompt": "Build the requested landing page features.",
  "fixture": "fixtures/static-site",
  "agent_command": "codex exec --cd {workspace} --ask-for-approval never --sandbox workspace-write -- \"$(cat {prompt_file})\"",
  "timeout_seconds": 900,
  "score_commands": ["sh scripts/check.sh"],
  "required_files": ["index.html", "styles.css"],
  "forbidden_files": ["debug.log"],
  "required_content": [
    { "file": "index.html", "contains": "Launch" }
  ]
}
```

Command templates support `{suite_dir}`, `{run_dir}`, `{workspace}`,
`{prompt_file}`, `{condition}`, and `{project}`. The runner appends condition
instructions to the prompt: `no-memory` tells the agent not to use Memory and
clears common Memory environment variables; Memory-enabled conditions tell the
agent to use Memory when useful.

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

Run the build-simulation smoke suite without provider cost:

```bash
memory eval run \
  --suite evals/examples/app-build-smoke \
  --condition no-memory \
  --condition full-memory \
  --profile offline \
  --text
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

Condition mapping:

- `no-memory`: no retrieval channel; answer/resume items use the direct baseline path.
- `lexical`: query with `retrieval_mode = "lexical"` and deterministic retrieval-only scoring where applicable.
- `semantic`: query with `retrieval_mode = "semantic"`.
- `graph`: query with `retrieval_mode = "graph"`.
- `full-memory`: query with `retrieval_mode = "full-memory"`.

For grounded-answer items, Memory-backed LLM runs force `answer_mode = "llm"`;
offline runs force `answer_mode = "deterministic"` so CI can validate the
pipeline without provider calls.

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

Agent build tasks report agent exit code, setup command pass count, score
command pass count, required/forbidden file checks, required content checks,
`total_score`, and duration. Raw command output and the copied workspace are
stored beside the run artifacts so failed builds can be inspected.

Run artifacts include `run_group_id`, `repeat_index`, `suite_checksum`,
`fixture_checksum`, `duration_ms`, and provider token usage when the underlying
LLM response reports it. Comparisons report quality, latency, and token deltas
for plain LLM behavior versus Memory-backed behavior.

## Notes

The checked-in `research-v1` suite is a reviewed-seed location, not yet a
publication-grade benchmark. Expand it to 100+ held-out reviewed items before
making external statistical claims.
