#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 <payload.json>" >&2
  exit 2
fi

PAYLOAD_FILE="$1"
if [[ -n "${MEMCTL_BIN:-}" ]]; then
  :
elif command -v memctl >/dev/null 2>&1; then
  MEMCTL_BIN="memctl"
elif command -v mem-cli >/dev/null 2>&1; then
  MEMCTL_BIN="mem-cli"
else
  MEMCTL_BIN="cargo run --quiet --bin mem-cli --"
fi

if [[ ! -f "$PAYLOAD_FILE" ]]; then
  echo "Payload file not found: $PAYLOAD_FILE" >&2
  exit 2
fi

exec bash -lc "$MEMCTL_BIN capture-task --file \"$PAYLOAD_FILE\""
