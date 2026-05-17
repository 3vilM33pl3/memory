#!/usr/bin/env sh
set -eu

memory_cmd="/workspace/target/debug/memory --config /workspace/evals/docker/app-build-sequence/config.eval.toml"
eval_suite="${MEMORY_EVAL_SUITE:-/workspace/evals/suites/app-build-sequence-codex-v1}"

mkdir -p "$CODEX_HOME"
if [ -d /codex-home-host ]; then
  for file in auth.json installation_id version.json; do
    if [ -f "/codex-home-host/$file" ]; then
      cp "/codex-home-host/$file" "$CODEX_HOME/$file"
    fi
  done
fi

until $memory_cmd health >/tmp/memory-health.txt 2>/tmp/memory-health.err; do
  sleep 2
done

sh /workspace/evals/docker/app-build-sequence/seed-memory.sh

$memory_cmd eval doctor --suite "$eval_suite" --text
$memory_cmd eval run \
  --suite "$eval_suite" \
  --condition no-memory \
  --condition full-memory \
  --profile llm \
  --repeat 1 \
  --allow-shell \
  --text
