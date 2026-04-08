#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALL_ROOT="${INSTALL_ROOT:-$HOME/.local}"
BIN_DIR="$INSTALL_ROOT/bin"
if [[ "$(uname -s)" == "Darwin" ]]; then
  CONFIG_DIR="$HOME/Library/Application Support/memory-layer"
  SHARE_DIR="$CONFIG_DIR"
  SERVICE_STEP="$BIN_DIR/memory service enable"
else
  CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/memory-layer"
  SHARE_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/memory-layer"
  SERVICE_STEP="$BIN_DIR/memory service run --config $CONFIG_DIR/memory-layer.toml"
fi
ENV_FILE="$CONFIG_DIR/memory-layer.env"
SKILL_TEMPLATE_DIR="$SHARE_DIR/skill-template"
WEB_DIR="$SHARE_DIR/web"

mkdir -p "$BIN_DIR" "$CONFIG_DIR" "$SKILL_TEMPLATE_DIR" "$WEB_DIR"

echo "Building release binaries..."
cargo build --release --manifest-path "$ROOT_DIR/Cargo.toml" --bin memory
echo "Building web UI..."
npm --prefix "$ROOT_DIR/web" ci
npm --prefix "$ROOT_DIR/web" run build

install -m 0755 "$ROOT_DIR/target/release/memory" "$BIN_DIR/memory"
rm -rf "$SKILL_TEMPLATE_DIR"
mkdir -p "$SKILL_TEMPLATE_DIR"
cp -R "$ROOT_DIR/.agents/skills/." "$SKILL_TEMPLATE_DIR/"
find "$SKILL_TEMPLATE_DIR" -type f -path '*/scripts/*' -exec chmod 0755 {} +
rm -rf "$WEB_DIR"
mkdir -p "$WEB_DIR"
cp -R "$ROOT_DIR/web/dist/." "$WEB_DIR/"

if [[ ! -f "$CONFIG_DIR/memory-layer.toml" ]]; then
  install -m 0644 "$ROOT_DIR/memory-layer.toml.example" "$CONFIG_DIR/memory-layer.toml"
  if [[ "$(uname -s)" == "Darwin" ]]; then
    mkdir -p "$CONFIG_DIR/run"
    perl -0pi -e "s|capnp_unix_socket = \"/tmp/memory-layer\\.capnp\\.sock\"|capnp_unix_socket = \"$CONFIG_DIR/run/memory-layer.capnp.sock\"|" \
      "$CONFIG_DIR/memory-layer.toml"
  fi
  echo "Installed default config at $CONFIG_DIR/memory-layer.toml"
else
  echo "Keeping existing config at $CONFIG_DIR/memory-layer.toml"
fi

if [[ ! -f "$ENV_FILE" ]]; then
  cat > "$ENV_FILE" <<'EOF'
# Shared secrets and overrides for Memory Layer CLI and background services.
# The service API token is provisioned automatically during setup.
# Example:
# OPENAI_API_KEY=replace-me
EOF
  chmod 600 "$ENV_FILE"
  echo "Installed default environment file at $ENV_FILE"
else
  echo "Keeping existing environment file at $ENV_FILE"
fi

"$BIN_DIR/memory" --config "$CONFIG_DIR/memory-layer.toml" service ensure-api-token --rotate-placeholder >/dev/null
echo "Ensured shared service API token at $ENV_FILE"

cat <<EOF

Installed:
  $BIN_DIR/memory
  $SKILL_TEMPLATE_DIR
  $WEB_DIR

Next steps:
1. In each repo, run:
   $BIN_DIR/memory wizard
2. Optional: if you want to configure shared/global defaults too, open:
   $CONFIG_DIR/memory-layer.toml
3. Optional: shared env file path:
   $ENV_FILE
4. Start the backend:
   $SERVICE_STEP
5. Optional: enable the automation watcher user service:
   $BIN_DIR/memory watcher enable --project <slug>
6. Launch the TUI:
   $BIN_DIR/memory tui
7. Open the web UI:
   http://127.0.0.1:4040/

EOF
