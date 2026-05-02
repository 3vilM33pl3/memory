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
  test ! -f memory-evidence.json
  test ! -d .memory-eval
else
  test -f memory-evidence.md
  test -f .memory-eval/q1.status.json
  test -f .memory-eval/q1.json
fi

test ! -f debug.log
