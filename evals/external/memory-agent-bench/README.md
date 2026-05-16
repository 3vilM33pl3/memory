# MemoryAgentBench Integration

This directory contains Memory-owned glue for running a small native
MemoryAgentBench pilot against Memory Layer.

The benchmark itself is not vendored. `scripts/prepare.sh` clones
`https://github.com/HUST-AI-HYZ/MemoryAgentBench` into
`target/memory-agent-bench`, checks out the pinned commit
`569241d877899d5c36d7d3b789de6c2489ea6cba`, copies the Memory Layer adapter,
and applies a small local patch to `agent.py`.

## Shape

- MemoryAgentBench stays external and pinned.
- The adapter adds a `memory_layer` agent type.
- Memorization writes benchmark chunks into a unique Memory project through
  `/v1/capture/task`, then curates the raw capture through `/v1/curate`.
- Querying uses `/v1/query` with `retrieval_mode = "full-memory"` and
  `answer_mode = "llm"`.
- The default pilot targets a short Conflict Resolution slice with a default cap
  of four queries.

## Prerequisites

Start a disposable Memory service first. Use a dev service/database or another
isolated stack because the pilot creates benchmark project slugs.

Set the service token:

```bash
export MEMORY_AGENT_BENCH_MEMORY_API_TOKEN="<service api token>"
```

For a trusted local dev service, you can instead use local-origin auth:

```bash
export MEMORY_AGENT_BENCH_ORIGIN="http://127.0.0.1:4250"
```

Optionally override:

```bash
export MEMORY_AGENT_BENCH_MEMORY_URL="http://127.0.0.1:4040"
export MEMORY_AGENT_BENCH_PROJECT_PREFIX="mab"
export MEMORY_AGENT_BENCH_MAX_QUERIES=4
```

Memory query answer synthesis is recommended for meaningful answer-quality
results because the adapter requests `answer_mode = "llm"`. If synthesis is not
available, the service may return a deterministic fallback answer.

## Run

Prepare the external checkout only:

```bash
evals/external/memory-agent-bench/scripts/prepare.sh
```

Run the small pilot:

```bash
evals/external/memory-agent-bench/scripts/run-pilot.sh
```

The default dataset config is:

```text
configs/data_conf/Conflict_Resolution/Factconsolidation_sh_6k.yaml
```

Artifacts are written by MemoryAgentBench under its external checkout, normally
below `target/memory-agent-bench/outputs/`.

## Local Tests

Run the Memory-owned adapter tests without installing MemoryAgentBench:

```bash
python3 -m unittest discover -s evals/external/memory-agent-bench/tests
```
