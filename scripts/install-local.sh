#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALL_ROOT="${INSTALL_ROOT:-$HOME/.local}"
BIN_DIR="$INSTALL_ROOT/bin"
CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/memory-layer"

mkdir -p "$BIN_DIR" "$CONFIG_DIR"

echo "Building release binaries..."
cargo build --release --manifest-path "$ROOT_DIR/Cargo.toml" --bin mem-cli --bin mem-service

install -m 0755 "$ROOT_DIR/target/release/mem-cli" "$BIN_DIR/mem-cli"
install -m 0755 "$ROOT_DIR/target/release/mem-service" "$BIN_DIR/mem-service"

if [[ ! -f "$CONFIG_DIR/memory-layer.toml" ]]; then
  install -m 0644 "$ROOT_DIR/memory-layer.toml.example" "$CONFIG_DIR/memory-layer.toml"
  echo "Installed default config at $CONFIG_DIR/memory-layer.toml"
else
  echo "Keeping existing config at $CONFIG_DIR/memory-layer.toml"
fi

cat <<EOF

Installed:
  $BIN_DIR/mem-cli
  $BIN_DIR/mem-service

Next steps:
1. Edit $CONFIG_DIR/memory-layer.toml
2. Start the backend:
   $BIN_DIR/mem-service $CONFIG_DIR/memory-layer.toml
3. Launch the TUI:
   $BIN_DIR/mem-cli --config $CONFIG_DIR/memory-layer.toml tui

EOF
