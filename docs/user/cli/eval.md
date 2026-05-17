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
- `agent_build_sequence`: copies one fixture project, runs a sequence of agent prompts against the same workspace, then scores every step and the final accumulated result.

## Shell Trust Boundary

Eval suites are code execution inputs, not passive data files.

These item types can execute arbitrary shell commands from the suite:

- `command_task`
- `agent_build_task`
- `agent_build_sequence`

Real runs of suites containing those item types require `--allow-shell`.
Review the suite directory, scripts, fixtures, and JSONL before using that flag.
`--dry-run` does not execute shell commands and does not require `--allow-shell`.

Every item may also include optional benchmark metadata:

```json
{
  "reasoning_mode": "deductive",
  "memory_capability": "retrieval",
  "difficulty": "medium",
  "claim": "Memory retrieves explicit project rules."
}
```

`memory eval compare` groups results by `eval_type`, `reasoning_mode`, and
`memory_capability` when those fields are present.

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
  "agent_command": "sh {suite_dir}/scripts/run-codex.sh {workspace} {prompt_file} {run_dir}",
  "memory_questions": ["What should the agent know before building this?"],
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
agent to use Memory when useful. For Memory-enabled build tasks,
`memory_questions` are appended as required questions and the runner exposes the
Memory CLI in `$MEMORY_EVAL_MEMORY_COMMAND`.

For build tasks with `memory_questions`, the harness also writes
`./.memory-eval/query-memory` into the copied workspace. The generated prompt
requires the agent to run that helper for each required question. The helper
writes raw query JSON and status files under `.memory-eval/`, and the harness
fails the item unless every required question has a successful query response
with at least one returned memory. Hand-written `memory-evidence.md` is useful
for humans, but it is not accepted as proof of Memory access by itself.

`agent_build_sequence` rows are for longer product-build simulations where
continuity matters. The fixture is copied once, then every step receives its own
prompt, run directory, Memory helper, score commands, and file/content checks.
The workspace is preserved between steps, so the result measures whether the
agent can keep improving one app instead of solving isolated tasks.

```json
{
  "eval_type": "agent_build_sequence",
  "id": "product-site-sequence",
  "project": "memory",
  "fixture": "fixtures/product-site",
  "agent_command": "sh {suite_dir}/scripts/run-codex.sh {workspace} {prompt_file} {run_dir}",
  "timeout_seconds": 420,
  "steps": [
    {
      "id": "hero",
      "prompt": "Add the hero section.",
      "memory_questions": ["What should this product page emphasize?"],
      "score_commands": ["sh scripts/check.sh"],
      "required_files": ["index.html", "styles.css"],
      "forbidden_files": ["debug.log"],
      "required_content": [
        { "file": "index.html", "contains": "SEQ-01-HERO" }
      ]
    }
  ]
}
```

Codex-backed build suites run the wrapper with `codex exec --json`. The harness
stores raw events in `codex-events.jsonl`, normalizes token totals into
`codex-token-usage.json` when available, and includes aggregated token usage in
the item result. Official Codex sequence runs fail if no parseable token usage
is captured, because cost is part of the benchmark evidence.

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
  --allow-shell \
  --text
```

Run the real Codex-backed build simulation suite:

```bash
MEMORY_EVAL_CODEX_MODEL=gpt-5.4-mini \
memory eval run \
  --suite evals/suites/app-build-codex-v1 \
  --condition no-memory \
  --condition full-memory \
  --profile llm \
  --repeat 1 \
  --allow-shell \
  --text
```

Use the dev binary form inside this repository:

```bash
MEMORY_EVAL_CODEX_MODEL=gpt-5.4-mini \
cargo run --bin memory -- eval run \
  --suite evals/suites/app-build-codex-v1 \
  --condition no-memory \
  --condition full-memory \
  --profile llm \
  --repeat 1 \
  --allow-shell \
  --text
```

The real suite uses `evals/suites/app-build-codex-v1/scripts/run-codex.sh` to
wrap `codex exec`, write `codex-final.md`, and stop once the final message is
stable. Set `MEMORY_EVAL_CODEX_WATCHDOG_SECONDS` if a local Codex run needs a
longer watchdog. Set `MEMORY_EVAL_CODEX_SANDBOX` to override the Codex sandbox;
the real app-build suite defaults to `danger-full-access` because the evaluated
Codex process must reach the local Memory service on localhost. In that default
mode the wrapper uses Codex's explicit sandbox-bypass flag; only run this suite
against disposable fixtures.

Dockerized Codex evals use a sanitized `CODEX_HOME`: only minimal auth bootstrap
files are copied from the host. Do not copy host `AGENTS.md`, Codex config,
plugins, session state, history, or logs into benchmark containers, because
those files can introduce non-benchmark instructions and skew token totals.

Run the longer Dockerized sequence suite when you want an isolated full-stack
evaluation with PostgreSQL, pgvector, Memory service, seeded memories, and real
Codex agents:

```bash
docker compose -f evals/docker/app-build-sequence/compose.yml run --rm eval
```

Use this form for clean reruns:

```bash
docker compose -f evals/docker/app-build-sequence/compose.yml down -v
docker compose -f evals/docker/app-build-sequence/compose.yml run --rm eval
```

The Docker run writes artifacts to `target/memory-evals-docker/` on the host.
The sequence suite has around 20 ordered steps and runs both `no-memory` and
`full-memory`, so expect materially higher model cost than the smoke suites.

Run the 10-step functionality-change follow-up against the saved final website
fixture with the same Docker stack:

```bash
MEMORY_EVAL_SUITE=/workspace/evals/suites/app-build-functionality-sequence-codex-v1 \
docker compose -f evals/docker/app-build-sequence/compose.yml run --rm eval
```

That suite starts from the saved `final-full-memory` product site fixture and
asks Codex to add dependency-free browser functionality. The previous
`final-no-memory` and `final-full-memory` generated websites are stored under
`evals/fixtures/app-build-sequence-codex-v1/` so later follow-up tests can reuse
the exact prior outputs instead of depending on local `target/` artifacts.

Run the Memory improvement benchmark when you want stronger evidence that
Memory changes agent behavior, not only harness integration:

```bash
MEMORY_EVAL_REPEAT=5 \
docker compose -f evals/docker/memory-improvement/compose.yml run --rm eval
```

That suite combines retrieval, grounded answers, resume quality, and a 20-step
Codex continuity task. It seeds hidden benchmark memories, maps items to
deductive, inductive, and abductive reasoning modes, verifies Memory helper
queries, captures token usage, and supports optional hybrid judging:

```bash
MEMORY_EVAL_LLM_JUDGE=1 \
MEMORY_EVAL_REPEAT=5 \
docker compose -f evals/docker/memory-improvement/compose.yml run --rm eval
```

Run paired conditions:

```bash
memory eval run \
  --suite evals/suites/research-v1 \
  --condition no-memory \
  --condition full-memory \
  --profile llm \
  --repeat 5 \
  --fail-on-unreviewed-labels \
  --allow-shell
```

The `no-memory` condition uses the configured OpenAI-compatible `[llm]` client
directly for `grounded_answer` and `resume_quality` items. It does not call
Memory retrieval, project timeline, or resume endpoints for those item types.
`retrieval_qa` items still receive zero retrieval scores under `no-memory`
because there is intentionally no retrieval channel.

Use `--profile offline` for CI-safe checks. Offline runs do not call the direct
no-memory LLM baseline and can be used to validate suite shape and scoring.
Official evidence runs should use the default `llm` profile.

## External Retriever Direction

External retriever support should stay separate from production query code. The intended research interface is an executable contract:

- Memory writes a JSON request containing the item id, question or prompt, project, condition, and fixture context.
- The executable returns JSON with ranked results, citations, answer text when applicable, timing, token usage, and diagnostic notes.
- Scoring consumes that external-run artifact beside normal Memory run artifacts.
- External retriever failures are scored as eval failures or skips, not routed through production retrieval.

This keeps experiments reproducible while avoiding accidental coupling between benchmark adapters and the Memory service query path.

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

Compare repeated run artifacts with globs:

```bash
memory eval compare \
  --baseline 'target/memory-evals/*no-memory*.json' \
  --candidate 'target/memory-evals/*full-memory*.json' \
  --out target/memory-evals/comparison.json \
  --text
```

Render a Markdown report:

```bash
memory eval report \
  --comparison target/memory-evals/comparison.json \
  --markdown \
  --out target/memory-evals/report.md
```

Gate a comparison before release:

```bash
memory eval gate \
  --comparison target/memory-evals/comparison.json \
  --policy evals/gates/research-v1.toml \
  --text
```

## Metrics

Retrieval items report Recall@k, MRR, nDCG, citation precision, tag recall,
file recall, semantic candidate counts, and graph candidate counts. If a
`retrieval_qa` item declares `expected_tags` or `expected_files`, those
expectations affect success; they are not just documentation. Grounded-answer
and resume items report assertion/topic recall and forbidden-hit counts.
Comparisons are paired by item id, repeat index, and sequence sub-result id,
then include success-rate deltas, McNemar p-values, bootstrap confidence
intervals for numeric metric deltas, grouped summaries, token deltas, and
cost-adjusted success deltas.

Agent build tasks report agent exit code, setup command pass count, score
command pass count, required/forbidden file checks, required content checks,
`total_score`, and duration. Raw command output and the copied workspace are
stored beside the run artifacts so failed builds can be inspected.

Agent build sequences report the same fields aggregated across steps, plus
machine-readable step sub-results in the run JSON. They also write a `steps/`
artifact directory with each step's prompt, command output, Memory-query
evidence, Codex JSONL events, normalized token usage, and step-level summary.

When `--llm-judge` is passed, answer-like items also receive diagnostic judge
metrics: `judge_evidence_use`, `judge_reasoning_quality`, `judge_consistency`,
and `judge_maintainability`. These scores are extra evidence only;
deterministic checks still decide pass/fail.

Run artifacts include `run_group_id`, `repeat_index`, `suite_checksum`,
`fixture_checksum`, `duration_ms`, and provider token usage when the underlying
LLM response reports it. Comparisons report quality, latency, and token deltas
for plain LLM behavior versus Memory-backed behavior.

## Notes

The checked-in `research-v1` suite is a reviewed-seed location, not yet a
publication-grade benchmark. Expand it to 100+ held-out reviewed items before
making external statistical claims.
