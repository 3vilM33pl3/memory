#!/usr/bin/env sh
set -eu

memory_cmd="cargo run --quiet --bin memory -- --config /workspace/evals/docker/app-build-sequence/config.eval.toml"

until $memory_cmd health --json >/tmp/memory-health.json 2>/tmp/memory-health.err; do
  sleep 2
done

sh /workspace/evals/docker/app-build-sequence/seed-memory.sh

$memory_cmd eval doctor --suite /workspace/evals/suites/app-build-sequence-codex-v1 --text
$memory_cmd eval run \
  --suite /workspace/evals/suites/app-build-sequence-codex-v1 \
  --condition no-memory \
  --condition full-memory \
  --profile llm \
  --repeat 1 \
  --text
