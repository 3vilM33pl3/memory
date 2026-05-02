#!/usr/bin/env sh
set -eu

test -f index.html
test -f styles.css
grep -qi "no-memory" index.html
grep -qi "full-memory" index.html
grep -Eqi "total[_ -]?score" index.html
grep -qi "target/memory-evals" index.html
grep -qi "dashboard" styles.css

if [ "${MEMORY_EVAL_CONDITION:-}" = "no-memory" ]; then
  test ! -f memory-evidence.md
else
  test -f memory-evidence.md
  grep -qi "query" memory-evidence.md
fi

test ! -f debug.log
