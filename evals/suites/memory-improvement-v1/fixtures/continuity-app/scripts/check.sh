#!/usr/bin/env sh
set -eu

test -f index.html
test -f styles.css
! grep -R "https://\\|http://\\|unpkg\\|jsdelivr\\|cdn" index.html styles.css scripts 2>/dev/null

if [ -f scripts/app.js ]; then
  node --check scripts/app.js
fi
