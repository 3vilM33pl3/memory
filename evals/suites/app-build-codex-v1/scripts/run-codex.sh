#!/usr/bin/env sh
set -eu

workspace="$1"
prompt_file="$2"
run_dir="$3"
final_file="$run_dir/codex-final.md"
model="${MEMORY_EVAL_CODEX_MODEL:-gpt-5.4-mini}"

rm -f "$final_file"

workspace_changed() {
  find "$workspace" -type f -newer "$prompt_file" | grep -q .
}

suite_dir=$(dirname "$0")/..
repo_root=$(cd "$suite_dir/../../.." && pwd)

GIT_CEILING_DIRECTORIES="$repo_root" \
codex exec \
  --cd "$workspace" \
  --skip-git-repo-check \
  --full-auto \
  --sandbox workspace-write \
  --ignore-rules \
  --ephemeral \
  --model "$model" \
  --output-last-message "$final_file" \
  - < "$prompt_file" &

pid="$!"
deadline=$(( $(date +%s) + ${MEMORY_EVAL_CODEX_WATCHDOG_SECONDS:-360} ))
stable_since=0
last_size=-1

while kill -0 "$pid" 2>/dev/null; do
  now=$(date +%s)
  if [ -s "$final_file" ]; then
    size=$(wc -c < "$final_file")
    if [ "$size" = "$last_size" ]; then
      if [ "$stable_since" -eq 0 ]; then
        stable_since="$now"
      elif [ $((now - stable_since)) -ge 8 ]; then
        kill "$pid" 2>/dev/null || true
        wait "$pid" 2>/dev/null || true
        exit 0
      fi
    else
      stable_since=0
      last_size="$size"
    fi
  fi
  if [ "$now" -ge "$deadline" ]; then
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
    test -s "$final_file" || workspace_changed
    exit $?
  fi
  sleep 1
done

if wait "$pid"; then
  exit 0
fi

test -s "$final_file" || workspace_changed
