#!/usr/bin/env sh
set -eu

test -f index.html
test -f styles.css
grep -q "Sequence Smoke" index.html
grep -q "linear-gradient" styles.css
test ! -f debug.log
