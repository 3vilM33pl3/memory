#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 \"<question>\" [project-slug]" >&2
  exit 2
fi

QUESTION="$1"
PROJECT="${2:-${MEMORY_LAYER_PROJECT:-$(basename "$PWD")}}"
MEMCTL_BIN="${MEMCTL_BIN:-cargo run --quiet --bin mem-cli --}"

exec bash -lc "$MEMCTL_BIN query --project \"$PROJECT\" --question \"$QUESTION\""
