#!/usr/bin/env sh
set -eu

memory_cmd="/workspace/target/debug/memory --config /workspace/evals/docker/app-build-sequence/config.eval.toml"

mkdir -p "$CODEX_HOME"
if [ -d /codex-home-host ]; then
  cp -a /codex-home-host/. "$CODEX_HOME"/
fi

until $memory_cmd health >/tmp/memory-health.txt 2>/tmp/memory-health.err; do
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
