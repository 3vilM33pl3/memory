#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 \"<question>\" [project-slug]" >&2
  exit 2
fi

QUESTION="$1"
PROJECT="${2:-${MEMORY_LAYER_PROJECT:-$(basename "$PWD")}}"
MEMCTL_BIN="${MEMCTL_BIN:-memctl}"
EXTRA_ARGS=()

if [[ "${MEMORY_LAYER_JSON:-0}" == "1" ]]; then
  EXTRA_ARGS+=(--json)
fi

exec "$MEMCTL_BIN" query \
  --project "$PROJECT" \
  --question "$QUESTION" \
  "${EXTRA_ARGS[@]}"
