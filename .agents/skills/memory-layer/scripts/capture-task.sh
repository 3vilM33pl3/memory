#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "${BASH_SOURCE[0]%/*}" && pwd)"
. "$SCRIPT_DIR/common.sh"

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 <payload.json>" >&2
  exit 2
fi

PAYLOAD_FILE="$1"
resolve_memctl_cmd

if [[ ! -f "$PAYLOAD_FILE" ]]; then
  echo "Payload file not found: $PAYLOAD_FILE" >&2
  exit 2
fi

exec "${MEMCTL_CMD[@]}" capture task --file "$PAYLOAD_FILE"
