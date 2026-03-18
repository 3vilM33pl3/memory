#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALL_ROOT="${INSTALL_ROOT:-$HOME/.local}"
BIN_DIR="$INSTALL_ROOT/bin"
CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/memory-layer"
ENV_FILE="$CONFIG_DIR/memory-layer.env"
SHARE_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/memory-layer"
SKILL_TEMPLATE_DIR="$SHARE_DIR/skill-template"

mkdir -p "$BIN_DIR" "$CONFIG_DIR" "$SKILL_TEMPLATE_DIR"

echo "Building release binaries..."
cargo build --release --manifest-path "$ROOT_DIR/Cargo.toml" --bin mem-cli --bin mem-service --bin memory-watch

install -m 0755 "$ROOT_DIR/target/release/mem-cli" "$BIN_DIR/mem-cli"
install -m 0755 "$ROOT_DIR/target/release/mem-service" "$BIN_DIR/mem-service"
install -m 0755 "$ROOT_DIR/target/release/memory-watch" "$BIN_DIR/memory-watch"
rm -rf "$SKILL_TEMPLATE_DIR"
mkdir -p "$SKILL_TEMPLATE_DIR"
cp -R "$ROOT_DIR/.agents/skills/memory-layer/." "$SKILL_TEMPLATE_DIR/"
find "$SKILL_TEMPLATE_DIR" -type f -path '*/scripts/*' -exec chmod 0755 {} +

if [[ ! -f "$CONFIG_DIR/memory-layer.toml" ]]; then
  install -m 0644 "$ROOT_DIR/memory-layer.toml.example" "$CONFIG_DIR/memory-layer.toml"
  echo "Installed default config at $CONFIG_DIR/memory-layer.toml"
else
  echo "Keeping existing config at $CONFIG_DIR/memory-layer.toml"
fi

if [[ ! -f "$ENV_FILE" ]]; then
  cat > "$ENV_FILE" <<'EOF'
# Shared secrets and overrides for Memory Layer CLI and systemd --user watcher units.
# Example:
# OPENAI_API_KEY=replace-me
EOF
  chmod 600 "$ENV_FILE"
  echo "Installed default environment file at $ENV_FILE"
else
  echo "Keeping existing environment file at $ENV_FILE"
fi

cat <<EOF

Installed:
  $BIN_DIR/mem-cli
  $BIN_DIR/mem-service
  $BIN_DIR/memory-watch
  $SKILL_TEMPLATE_DIR

Next steps:
1. Edit the shared config:
   $CONFIG_DIR/memory-layer.toml
2. Add shared environment variables for CLI and watcher services:
   $ENV_FILE
3. In each repo, run:
   $BIN_DIR/mem-cli init
4. Start the backend from the repo root:
   $BIN_DIR/mem-service
5. Optional: enable the automation watcher user service:
   $BIN_DIR/mem-cli watch enable --project <slug>
6. Launch the TUI:
   $BIN_DIR/mem-cli tui

EOF
