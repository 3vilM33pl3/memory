#!/usr/bin/env bash
set -euo pipefail

PROJECT="${1:-${MEMORY_LAYER_PROJECT:-$(basename "$PWD")}}"
MEMCTL_BIN="${MEMCTL_BIN:-memctl}"
EXTRA_ARGS=()

if [[ -n "${MEMORY_LAYER_BATCH_SIZE:-}" ]]; then
  EXTRA_ARGS+=(--batch-size "$MEMORY_LAYER_BATCH_SIZE")
fi

exec "$MEMCTL_BIN" curate --project "$PROJECT" "${EXTRA_ARGS[@]}"
