#!/usr/bin/env bash
set -euo pipefail

if [[ -n "${MEMCTL_BIN:-}" ]]; then
  read -r -a MEMCTL_CMD <<< "$MEMCTL_BIN"
elif command -v memctl >/dev/null 2>&1; then
  MEMCTL_CMD=(memctl)
elif command -v mem-cli >/dev/null 2>&1; then
  MEMCTL_CMD=(mem-cli)
else
  MEMCTL_CMD=(cargo run --quiet --bin mem-cli --)
fi

exec "${MEMCTL_CMD[@]}" remember "$@"
