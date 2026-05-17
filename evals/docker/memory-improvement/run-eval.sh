#!/usr/bin/env sh
set -eu

memory_cmd="/usr/local/bin/memory --config /workspace/evals/docker/memory-improvement/config.eval.toml"
eval_suite="${MEMORY_EVAL_SUITE:-/workspace/evals/suites/memory-improvement-v1}"
repeat="${MEMORY_EVAL_REPEAT:-5}"
conditions="${MEMORY_EVAL_CONDITIONS:-no-memory lexical semantic graph full-memory}"
judge_flag=""

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

MEMORY_EVAL_MEMORY_CMD="$memory_cmd" sh /workspace/evals/docker/memory-improvement/seed-memory.sh

$memory_cmd graph extract --project memory --text || true
$memory_cmd eval doctor --suite "$eval_suite" --text

if [ "${MEMORY_EVAL_LLM_JUDGE:-0}" = "1" ]; then
  judge_flag="--llm-judge"
fi

set -- $conditions
condition_args=""
for condition in "$@"; do
  condition_args="$condition_args --condition $condition"
done

# shellcheck disable=SC2086
$memory_cmd eval run \
  --suite "$eval_suite" \
  $condition_args \
  --profile llm \
  --repeat "$repeat" \
  --allow-shell \
  $judge_flag \
  --text
