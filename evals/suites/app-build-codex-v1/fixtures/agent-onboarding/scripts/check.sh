#!/usr/bin/env sh
set -eu

test -f index.html
test -f styles.css
grep -qi "get-up-to-speed" index.html
grep -qi "query" index.html
grep -qi "activities" index.html
grep -qi "graph context" index.html
grep -qi "runbook" index.html

if [ "${MEMORY_EVAL_CONDITION:-}" = "no-memory" ]; then
  test ! -f memory-evidence.md
else
  test -f memory-evidence.md
  grep -qi "query" memory-evidence.md
fi

test ! -f debug.log
