#!/usr/bin/env bash
set -euo pipefail

# Build a macOS .pkg installer for Memory Layer.
# Usage: ./packaging/build-pkg.sh [--sign "Developer ID Installer: ..."]
#
# The resulting .pkg installs:
#   /usr/local/bin/memory          (+ mem-cli symlink)
#   /usr/local/share/memory-layer/ (web UI, skill templates, example config)

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="$(awk -F '"' '/^version = / { print $2; exit }' "$ROOT_DIR/Cargo.toml" 2>/dev/null || true)"
if [[ -z "${VERSION:-}" ]]; then
  VERSION="0.1.0"
fi

SIGN_IDENTITY=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --sign) SIGN_IDENTITY="$2"; shift 2 ;;
    *) echo "Unknown option: $1" >&2; exit 1 ;;
  esac
done

STAGE_DIR="$ROOT_DIR/target/macos-pkg"
PAYLOAD="$STAGE_DIR/payload"
SCRIPTS="$STAGE_DIR/scripts"
PKG_PATH="$ROOT_DIR/target/memory-layer-${VERSION}-macos.pkg"

rm -rf "$STAGE_DIR"
mkdir -p \
  "$PAYLOAD/usr/local/bin" \
  "$PAYLOAD/usr/local/share/memory-layer/skill-template" \
  "$PAYLOAD/usr/local/share/memory-layer/web" \
  "$SCRIPTS"

# --- Build -----------------------------------------------------------------
echo "Building release binary..."
cargo build --release --manifest-path "$ROOT_DIR/Cargo.toml" --bin memory

echo "Building web UI..."
npm --prefix "$ROOT_DIR/web" ci
npm --prefix "$ROOT_DIR/web" run build

# --- Stage payload ----------------------------------------------------------
install -m 0755 "$ROOT_DIR/target/release/memory" "$PAYLOAD/usr/local/bin/memory"
ln -sf memory "$PAYLOAD/usr/local/bin/mem-cli"

cp -R "$ROOT_DIR/.agents/skills/." "$PAYLOAD/usr/local/share/memory-layer/skill-template/"
find "$PAYLOAD/usr/local/share/memory-layer/skill-template" -type f -name '*.sh' -exec chmod 0755 {} +

install -m 0644 "$ROOT_DIR/memory-layer.toml.example" "$PAYLOAD/usr/local/share/memory-layer/memory-layer.toml.example"
install -m 0644 "$ROOT_DIR/README.md" "$PAYLOAD/usr/local/share/memory-layer/README.md"
cp -R "$ROOT_DIR/web/dist/." "$PAYLOAD/usr/local/share/memory-layer/web/"

# --- Post-install script ----------------------------------------------------
cat > "$SCRIPTS/postinstall" << 'POSTINSTALL'
#!/usr/bin/env bash
# Ensure the shared Application Support directory exists for the installing user.
APP_SUPPORT="$HOME/Library/Application Support/memory-layer"
mkdir -p "$APP_SUPPORT/logs"

SHARE="/usr/local/share/memory-layer"
if [[ ! -f "$APP_SUPPORT/memory-layer.toml" && -f "$SHARE/memory-layer.toml.example" ]]; then
  cp "$SHARE/memory-layer.toml.example" "$APP_SUPPORT/memory-layer.toml"
  echo "Installed example config to $APP_SUPPORT/memory-layer.toml"
fi

if command -v /usr/local/bin/memory >/dev/null 2>&1; then
  CONSOLE_USER="$(stat -f %Su /dev/console 2>/dev/null || true)"
  if [[ -n "$CONSOLE_USER" && "$CONSOLE_USER" != "root" ]]; then
    CONSOLE_UID="$(id -u "$CONSOLE_USER" 2>/dev/null || true)"
    if [[ -n "$CONSOLE_UID" ]]; then
      launchctl asuser "$CONSOLE_UID" sudo -u "$CONSOLE_USER" /usr/local/bin/memory service restart-all --mark-tui-restart --json || true
    fi
  else
    /usr/local/bin/memory service restart-all --mark-tui-restart --json || true
  fi
fi

echo ""
echo "Memory Layer $PACKAGE_VERSION installed. Get started:"
echo "  memory wizard --global"
echo "  memory service enable"
POSTINSTALL
chmod 0755 "$SCRIPTS/postinstall"

# --- Build .pkg -------------------------------------------------------------
echo "Assembling .pkg..."

PKGBUILD_ARGS=(
  --root "$PAYLOAD"
  --scripts "$SCRIPTS"
  --identifier "com.memory-layer.pkg"
  --version "$VERSION"
  --install-location /
)

if [[ -n "$SIGN_IDENTITY" ]]; then
  PKGBUILD_ARGS+=(--sign "$SIGN_IDENTITY")
fi

pkgbuild "${PKGBUILD_ARGS[@]}" "$PKG_PATH"

echo ""
echo "Built $PKG_PATH ($(du -h "$PKG_PATH" | cut -f1))"
