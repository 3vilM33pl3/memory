#!/usr/bin/env bash
set -euo pipefail

PROJECT="${1:-${MEMORY_LAYER_PROJECT:-$(basename "$PWD")}}"
if [[ -n "${MEMCTL_BIN:-}" ]]; then
  :
elif command -v memctl >/dev/null 2>&1; then
  MEMCTL_BIN="memctl"
elif command -v mem-cli >/dev/null 2>&1; then
  MEMCTL_BIN="mem-cli"
else
  MEMCTL_BIN="cargo run --quiet --bin mem-cli --"
fi

exec bash -lc "$MEMCTL_BIN curate --project \"$PROJECT\""
