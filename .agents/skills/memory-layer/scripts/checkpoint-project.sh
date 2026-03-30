#!/usr/bin/env bash
set -euo pipefail

PROJECT="${MEMORY_LAYER_PROJECT:-$(basename "$PWD")}"
NOTE=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --project)
      PROJECT="${2:?missing value for --project}"
      shift 2
      ;;
    --note)
      NOTE="${2:?missing value for --note}"
      shift 2
      ;;
    *)
      echo "Usage: $0 [--project <slug>] [--note <text>]" >&2
      exit 2
      ;;
  esac
done

if [[ -n "${MEMCTL_BIN:-}" ]]; then
  read -r -a MEMCTL_CMD <<< "$MEMCTL_BIN"
elif command -v memctl >/dev/null 2>&1; then
  MEMCTL_CMD=(memctl)
elif command -v mem-cli >/dev/null 2>&1; then
  MEMCTL_CMD=(mem-cli)
else
  MEMCTL_CMD=(cargo run --quiet --bin mem-cli --)
fi

ARGS=(checkpoint save --project "$PROJECT")
if [[ -n "$NOTE" ]]; then
  ARGS+=(--note "$NOTE")
fi

exec "${MEMCTL_CMD[@]}" "${ARGS[@]}"
