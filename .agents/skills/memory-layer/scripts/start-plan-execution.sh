#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "${BASH_SOURCE[0]%/*}" && pwd)"
. "$SCRIPT_DIR/common.sh"

resolve_memctl_cmd

exec "${MEMCTL_CMD[@]}" checkpoint start-execution "$@"
