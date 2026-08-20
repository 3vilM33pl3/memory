#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Structural guards for the crate boundaries established by the pre-federation
# refactor. A cosmetic re-merge of the old mem-api kitchen sink, a re-coupled
# read path, or a fat default CLI build fails CI here.
set -euo pipefail
cd "$(dirname "$0")/.."

fail=0
err() {
  echo "BOUNDARY VIOLATION: $*" >&2
  fail=1
}

# 1. mem-api must not come back.
if [ -d crates/mem-api ]; then
  err "crates/mem-api exists again"
fi
if grep -rq "mem_api::" crates/ --include='*.rs'; then
  err "code references mem_api::"
fi

# 2. mem-record purity: pure serde types, no runtime/config/db machinery.
record_tree=$(cargo tree -p mem-record -e normal --prefix none | awk '{print $1}' | sort -u)
for dep in tokio config sqlx axum mem-platform duckdb; do
  if echo "$record_tree" | grep -qx "$dep"; then
    err "mem-record depends on $dep"
  fi
done

# 3. mem-config purity: no async runtime, transport, or database.
config_tree=$(cargo tree -p mem-config -e normal --prefix none | awk '{print $1}' | sort -u)
for dep in tokio sqlx axum duckdb; do
  if echo "$config_tree" | grep -qx "$dep"; then
    err "mem-config depends on $dep"
  fi
done

# 4. No monolith re-forms in the new crates.
while read -r lines file; do
  if [ "$lines" -gt 1200 ] && [ "$file" != "total" ]; then
    err "$file is $lines lines (>1200) - split it"
  fi
done < <(find crates/mem-record/src crates/mem-config/src -name '*.rs' ! -name legacy_tests.rs -exec wc -l {} + | awk '{print $1, $2}')

# 5. Read path stays free of the write path.
search_tree=$(cargo tree -p mem-search -e normal --prefix none | awk '{print $1}' | sort -u)
for dep in mem-reinforce mem-curate; do
  if echo "$search_tree" | grep -qx "$dep"; then
    err "mem-search depends on $dep"
  fi
done

# 6. Default CLI build stays lean: no server, no database, no DuckDB.
cli_tree=$(cargo tree -p mem-cli -e normal --prefix none | awk '{print $1}' | sort -u)
for dep in mem-service axum duckdb openidconnect; do
  if echo "$cli_tree" | grep -qx "$dep"; then
    err "default mem-cli build depends on $dep"
  fi
done
if grep -q '^sqlx' crates/mem-cli/Cargo.toml; then
  err "mem-cli declares sqlx directly"
fi

# 7. Dependents declare minimal needs.
for crate in mem-curate mem-graph mem-eval mem-loops; do
  if grep -q 'mem-config' "crates/$crate/Cargo.toml"; then
    err "$crate depends on mem-config (records only, please)"
  fi
done
if grep -q 'mem-record' crates/mem-skills/Cargo.toml; then
  err "mem-skills depends on mem-record (config only, please)"
fi

if [ "$fail" -ne 0 ]; then
  exit 1
fi
echo "boundaries OK"
