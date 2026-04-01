#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "${BASH_SOURCE[0]%/*}" && pwd)"
. "$SCRIPT_DIR/common.sh"

PROJECT="${1:-${MEMORY_LAYER_PROJECT:-$(basename "$PWD")}}"
resolve_memctl_cmd

exec "${MEMCTL_CMD[@]}" resume --project "$PROJECT"
