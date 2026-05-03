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

## Agent Build Tasks

`agent_build_task` is the build-simulation path. It is intentionally generic:
the suite supplies a shell command template, so the harness can run Codex,
Claude, a fake local agent, or any other noninteractive runner.

For each item, `memory eval run`:

- resolves the fixture directory relative to the suite root
- copies it to `target/memory-evals/build-runs/<suite>-<item>-<condition>-rN/workspace`
- writes the final prompt to `prompt.md`
- runs optional setup commands, the agent command, and score commands in the copied workspace
- captures stdout, stderr, exit status, timeout status, and a summary JSON file
- scores deterministic file/content assertions and score command exits

The checked-in `evals/examples/app-build-smoke` suite uses a fake deterministic
agent. It exists to test the harness without provider cost. Real research suites
should replace `agent_command` with a noninteractive agent invocation such as
`codex exec --cd {workspace} ... "$(cat {prompt_file})"`.

The checked-in `evals/suites/app-build-codex-v1` suite is the first real
Codex-backed build simulation suite. It runs paired `no-memory` and
`full-memory` agents against dependency-free static app fixtures. Memory-enabled
runs receive `memory_questions`, the `$MEMORY_EVAL_MEMORY_COMMAND` environment
variable, and a generated `./.memory-eval/query-memory` helper. The prompt
requires the agent to run that helper for every required question. The harness
then verifies the helper-produced status and raw query JSON files before scoring
the item as successful. A hand-written `memory-evidence.md` is only a human
summary; it is not proof of Memory access.

The suite calls Codex through `scripts/run-codex.sh` rather than invoking
`codex exec` directly. The wrapper isolates git discovery from the parent repo,
uses the workspace-local `AGENTS.md`, writes `codex-final.md`, and exits once
the final message is stable so a completed agent does not hold the eval run open.
It defaults `MEMORY_EVAL_CODEX_SANDBOX` to `danger-full-access` so the evaluated
Codex process can reach the localhost Memory service. In that mode the wrapper
uses Codex's explicit sandbox-bypass flag, so keep fixtures disposable and
isolated. Override the environment variable only when the alternative sandbox
can still reach Memory.

Dockerized Codex benchmarks must use a sanitized `CODEX_HOME`. The eval
containers may copy authentication bootstrap files such as `auth.json`,
`installation_id`, and `version.json`, but must not copy host `AGENTS.md`,
`config.toml`, plugin caches, history, logs, rules, or session databases.
Copying those files makes token and quality claims invalid because global
agent instructions or tools can affect the evaluated run.

Condition isolation for build tasks is practical rather than absolute in v1.
The `no-memory` prompt explicitly forbids Memory usage and the runner removes
common Memory environment variables. Memory-enabled conditions receive a prompt
that requires verified Memory helper calls and exposes `MEMORY_LAYER_PROJECT`.
The harness also fails `no-memory` items if `memory-evidence.md`,
`memory-evidence.json`, or `.memory-eval` artifacts appear. Stronger isolation,
such as separate services or databases per condition, can be added later if the
benchmark needs adversarial guarantees.

## Agent Build Sequences

`agent_build_sequence` extends build simulations from isolated tasks to a
stateful product build. The suite supplies one fixture and an ordered `steps`
array. The runner copies the fixture once, keeps the same workspace for the
whole sequence, and gives every step its own prompt file, run directory,
Memory-query helper, score commands, and deterministic assertions.

The score is aggregated through the same `AgentBuildScoreInput` schema used by
`agent_build_task`, but the result `eval_type` is `agent_build_sequence`.
Sequence artifacts are laid out as:

- `summary.json`: aggregate sequence status, Memory evidence totals, token
  totals, and step summaries
- `workspace/`: the final accumulated application state
- `steps/<NN-step-id>/prompt.md`: the exact prompt for a step
- `steps/<NN-step-id>/agent.*`: stdout, stderr, status, and timeout artifacts
- `steps/<NN-step-id>/memory-eval/`: verified Memory helper query artifacts
- `steps/<NN-step-id>/codex-events.jsonl`: raw Codex JSON events when using the
  Codex wrapper
- `steps/<NN-step-id>/codex-token-usage.json`: normalized token totals when
  parseable

The checked-in `evals/examples/app-build-sequence-smoke` suite uses a fake
agent and three steps so CI can exercise the sequence path without provider
cost. The checked-in `evals/suites/app-build-sequence-codex-v1` suite uses real
Codex agents and around 20 ordered steps to build a Memory product site.
Codex-backed sequence runs require parseable token usage; missing
`codex-events.jsonl`/`codex-token-usage.json` data makes the item fail so cost
evidence cannot silently disappear.

The Docker stack under `evals/docker/app-build-sequence/` runs the long suite
against PostgreSQL with pgvector, a Memory service container, deterministic
seed memories, and a separate eval runner container:

```bash
docker compose -f evals/docker/app-build-sequence/compose.yml run --rm eval
```

Artifacts are bind-mounted to `target/memory-evals-docker/` on the host. Use
`docker compose -f evals/docker/app-build-sequence/compose.yml down -v` for a
clean database before rerunning. The eval container mounts the host Codex home
read-only, then copies only the minimal auth bootstrap files into an otherwise
clean `CODEX_HOME` so credentials are available without importing global agent
configuration.

The same Docker stack can run follow-up suites by setting `MEMORY_EVAL_SUITE`.
`evals/suites/app-build-functionality-sequence-codex-v1` is the first follow-up:
it starts from the saved final website fixture generated by the earlier
20-step sequence and applies 10 dependency-free functionality changes. The
paired run still compares `no-memory` and `full-memory`; the full-memory side
must verify one Memory query per step and both sides must emit Codex token
usage.

```bash
MEMORY_EVAL_SUITE=/workspace/evals/suites/app-build-functionality-sequence-codex-v1 \
docker compose -f evals/docker/app-build-sequence/compose.yml run --rm eval
```

The preserved prior websites live in
`evals/fixtures/app-build-sequence-codex-v1/`. Keep those fixtures immutable
unless intentionally replacing the historical baseline for a new evaluation
version.

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
OpenAI-compatible or Ollama `[llm]` client directly and scores the resulting
text without Memory retrieval, project timeline, or resume context. This keeps
provider I/O in `mem-cli`; `mem-eval` remains responsible for file formats,
scoring, and statistics.

Memory-backed `resume_quality` conditions currently use the deterministic
`up-to-speed` briefing (`include_llm_summary = false`) so the default comparison
isolates Memory evidence and timeline value before adding another LLM summary
step.

## Profiles And Repeats

The default official profile is `llm`. It uses the configured provider
(`openai_compatible` or `ollama`) for the plain no-memory baseline and
Memory-backed answer synthesis. Use
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
