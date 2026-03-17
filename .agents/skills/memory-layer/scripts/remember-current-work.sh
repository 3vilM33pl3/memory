#!/usr/bin/env bash
set -euo pipefail

if [[ -n "${MEMCTL_BIN:-}" ]]; then
  read -r -a MEMCTL_CMD <<< "$MEMCTL_BIN"
else
  MEMCTL_CMD=(cargo run --quiet --bin mem-cli --)
fi

exec "${MEMCTL_CMD[@]}" remember "$@"
