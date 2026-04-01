#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "${BASH_SOURCE[0]%/*}" && pwd)"
. "$SCRIPT_DIR/common.sh"

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

resolve_memctl_cmd

ARGS=(checkpoint save --project "$PROJECT")
if [[ -n "$NOTE" ]]; then
  ARGS+=(--note "$NOTE")
fi

exec "${MEMCTL_CMD[@]}" "${ARGS[@]}"
