#!/usr/bin/env sh
set -eu

test -f index.html
test -f styles.css
grep -q "Memory-aware Launch Notes" index.html
grep -q "Memory advantage" index.html
grep -q "Status Panel" index.html
grep -q "linear-gradient" styles.css
test ! -f debug.log

