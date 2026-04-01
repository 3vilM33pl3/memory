#!/usr/bin/env bash

memory_layer_script_dir() {
  cd "${BASH_SOURCE[0]%/*}" && pwd
}

memory_layer_source_root() {
  local script_dir
  script_dir="$(memory_layer_script_dir)"
  cd "$script_dir/../../../.." && pwd
}

resolve_memctl_cmd() {
  if [[ -n "${MEMCTL_BIN:-}" ]]; then
    read -r -a MEMCTL_CMD <<< "$MEMCTL_BIN"
    return 0
  fi

  if command -v memctl >/dev/null 2>&1; then
    MEMCTL_CMD=(memctl)
    return 0
  fi

  if command -v mem-cli >/dev/null 2>&1; then
    MEMCTL_CMD=(mem-cli)
    return 0
  fi

  local source_root
  source_root="$(memory_layer_source_root)"
  if [[ -f "$source_root/Cargo.toml" && -f "$source_root/crates/mem-cli/Cargo.toml" ]]; then
    MEMCTL_CMD=(cargo run --quiet --bin mem-cli --manifest-path "$source_root/Cargo.toml" --)
    return 0
  fi

  echo "Memory Layer CLI not found. Install \`mem-cli\` or \`memctl\`, or set MEMCTL_BIN to an explicit command." >&2
  return 1
}
