#!/usr/bin/env sh
set -eu

workspace="$1"
prompt_file="$2"
run_dir="$3"

mkdir -p "$run_dir"
cd "$workspace"

codex exec --json < "$prompt_file" > "$run_dir/codex-events.jsonl"
