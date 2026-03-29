#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 \"<question>\" [project-slug]" >&2
  exit 2
fi

QUESTION="$1"
PROJECT="${2:-${MEMORY_LAYER_PROJECT:-$(basename "$PWD")}}"
if [[ -n "${MEMCTL_BIN:-}" ]]; then
  :
elif command -v memctl >/dev/null 2>&1; then
  MEMCTL_BIN="memctl"
elif command -v mem-cli >/dev/null 2>&1; then
  MEMCTL_BIN="mem-cli"
else
  MEMCTL_BIN="cargo run --quiet --bin mem-cli --"
fi

exec bash -lc "$MEMCTL_BIN query --project \"$PROJECT\" --question \"$QUESTION\""
