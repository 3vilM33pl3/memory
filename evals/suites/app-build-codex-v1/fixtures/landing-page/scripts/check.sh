#!/usr/bin/env sh
set -eu

test -f index.html
test -f styles.css
grep -qi "multi-backend embeddings" index.html
grep -Eqi "distributed agent|watcher" index.html
grep -qi "graph-aware search" index.html
grep -qi "automated evaluations" index.html
grep -qi "linear-gradient" styles.css

if [ "${MEMORY_EVAL_CONDITION:-}" = "no-memory" ]; then
  test ! -f memory-evidence.md
else
  test -f memory-evidence.md
  grep -qi "query" memory-evidence.md
fi

test ! -f debug.log
