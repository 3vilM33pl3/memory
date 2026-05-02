#!/usr/bin/env sh
set -eu

suite_dir=$(dirname "$0")/..
sh "$suite_dir/../app-build-codex-v1/scripts/run-codex.sh" "$@"
