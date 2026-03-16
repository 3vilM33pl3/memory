#!/usr/bin/env bash
set -euo pipefail

PROJECT="${1:-${MEMORY_LAYER_PROJECT:-$(basename "$PWD")}}"
MEMCTL_BIN="${MEMCTL_BIN:-cargo run --quiet --bin mem-cli --}"

exec bash -lc "$MEMCTL_BIN curate --project \"$PROJECT\""
