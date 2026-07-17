#!/usr/bin/env bash
set -euo pipefail

# Build a macOS .pkg installer for Memory Layer.
# Usage:
#   ./packaging/build-pkg.sh
#   ./packaging/build-pkg.sh --arch x86_64
#   ./packaging/build-pkg.sh --arch aarch64
#   ./packaging/build-pkg.sh \
#     --sign-app "Developer ID Application: ..." \
#     --sign-pkg "Developer ID Installer: ..."
#   ./packaging/build-pkg.sh \
#     --sign-app "Developer ID Application: ..." \
#     --sign-pkg "Developer ID Installer: ..." \
#     --notarize --notary-profile "<profile>"
#
# The resulting .pkg installs:
#   /usr/local/bin/memory          (+ mem-cli and memory-layer symlinks)
#   /usr/local/share/memory-layer/ (web UI, skill templates, example config)
#   /usr/local/share/*             (bash, zsh, and fish completion scripts)

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="$(awk -F '"' '/^version = / { print $2; exit }' "$ROOT_DIR/Cargo.toml" 2>/dev/null || true)"
if [[ -z "${VERSION:-}" ]]; then
  VERSION="0.1.0"
fi

APP_SIGN_IDENTITY=""
PKG_SIGN_IDENTITY=""
NOTARIZE=0
NOTARY_PROFILE=""
APPLE_ID=""
TEAM_ID=""
APPLE_PASSWORD=""
PKG_ARCH=""
RUST_TARGET=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --arch)
      PKG_ARCH="$2"
      shift 2
      ;;
    --target)
      RUST_TARGET="$2"
      shift 2
      ;;
    --sign)
      PKG_SIGN_IDENTITY="$2"
      shift 2
      ;;
    --sign-app)
      APP_SIGN_IDENTITY="$2"
      shift 2
      ;;
    --sign-pkg)
      PKG_SIGN_IDENTITY="$2"
      shift 2
      ;;
    --notarize)
      NOTARIZE=1
      shift
      ;;
    --notary-profile)
      NOTARY_PROFILE="$2"
      shift 2
      ;;
    --apple-id)
      APPLE_ID="$2"
      shift 2
      ;;
    --team-id)
      TEAM_ID="$2"
      shift 2
      ;;
    --apple-password)
      APPLE_PASSWORD="$2"
      shift 2
      ;;
    *) echo "Unknown option: $1" >&2; exit 1 ;;
  esac
done

if [[ -z "$PKG_ARCH" ]]; then
  case "$(uname -m)" in
    x86_64)
      PKG_ARCH="x86_64"
      ;;
    arm64|aarch64)
      PKG_ARCH="aarch64"
      ;;
    *)
      echo "Unsupported macOS architecture: $(uname -m)" >&2
      exit 1
      ;;
  esac
fi

case "$PKG_ARCH" in
  x86_64)
    RUST_TARGET="${RUST_TARGET:-x86_64-apple-darwin}"
    ;;
  arm64|aarch64)
    PKG_ARCH="aarch64"
    RUST_TARGET="${RUST_TARGET:-aarch64-apple-darwin}"
    ;;
  *)
    echo "Unsupported macOS package architecture: $PKG_ARCH" >&2
    echo "Supported values: x86_64, aarch64" >&2
    exit 1
    ;;
esac

if [[ "$NOTARIZE" -eq 1 && -z "$PKG_SIGN_IDENTITY" ]]; then
  echo "Notarization requires a signed installer. Pass --sign-pkg." >&2
  exit 1
fi

if [[ "$NOTARIZE" -eq 1 && -z "$NOTARY_PROFILE" ]]; then
  if [[ -z "$APPLE_ID" || -z "$TEAM_ID" || -z "$APPLE_PASSWORD" ]]; then
    echo "Notarization requires either --notary-profile or all of --apple-id, --team-id, and --apple-password." >&2
    exit 1
  fi
fi

STAGE_DIR="$ROOT_DIR/target/macos-pkg"
PAYLOAD="$STAGE_DIR/payload"
SCRIPTS="$STAGE_DIR/scripts"
PKG_PATH="$ROOT_DIR/target/memory-layer-${VERSION}-macos-${PKG_ARCH}.pkg"
BINARY_PATH="$ROOT_DIR/target/$RUST_TARGET/release/memory"

rm -rf "$STAGE_DIR"
mkdir -p \
  "$PAYLOAD/usr/local/bin" \
  "$PAYLOAD/usr/local/share/memory-layer/skill-template" \
  "$PAYLOAD/usr/local/share/memory-layer/web" \
  "$PAYLOAD/usr/local/share/bash-completion/completions" \
  "$PAYLOAD/usr/local/share/zsh/site-functions" \
  "$PAYLOAD/usr/local/share/fish/vendor_completions.d" \
  "$SCRIPTS"

# --- Build -----------------------------------------------------------------
echo "Building release binary for $PKG_ARCH ($RUST_TARGET)..."
cargo build --release --manifest-path "$ROOT_DIR/Cargo.toml" --bin memory --target "$RUST_TARGET"

echo "Building web UI..."
npm --prefix "$ROOT_DIR/web" ci
npm --prefix "$ROOT_DIR/web" run build

# --- Stage payload ----------------------------------------------------------
install -m 0755 "$BINARY_PATH" "$PAYLOAD/usr/local/bin/memory"

if [[ -n "$APP_SIGN_IDENTITY" ]]; then
  echo "Signing binary with Developer ID Application certificate..."
  codesign --force --sign "$APP_SIGN_IDENTITY" --options runtime --timestamp "$PAYLOAD/usr/local/bin/memory"
  codesign --verify --verbose=2 "$PAYLOAD/usr/local/bin/memory"
fi

ln -sf memory "$PAYLOAD/usr/local/bin/mem-cli"
ln -sf memory "$PAYLOAD/usr/local/bin/memory-layer"
"$PAYLOAD/usr/local/bin/memory" completion bash > "$PAYLOAD/usr/local/share/bash-completion/completions/memory"
"$PAYLOAD/usr/local/bin/memory" completion zsh > "$PAYLOAD/usr/local/share/zsh/site-functions/_memory"
"$PAYLOAD/usr/local/bin/memory" completion fish > "$PAYLOAD/usr/local/share/fish/vendor_completions.d/memory.fish"

cp -R "$ROOT_DIR/.agents/skills/." "$PAYLOAD/usr/local/share/memory-layer/skill-template/"
find "$PAYLOAD/usr/local/share/memory-layer/skill-template" -type f -name '*.sh' -exec chmod 0755 {} +

install -m 0644 "$ROOT_DIR/memory-layer.toml.example" "$PAYLOAD/usr/local/share/memory-layer/memory-layer.toml.example"
install -m 0644 "$ROOT_DIR/README.md" "$PAYLOAD/usr/local/share/memory-layer/README.md"
cp -R "$ROOT_DIR/web/dist/." "$PAYLOAD/usr/local/share/memory-layer/web/"

# --- Post-install script ----------------------------------------------------
cat > "$SCRIPTS/postinstall" << 'POSTINSTALL'
#!/usr/bin/env bash
# Ensure the shared Application Support directory exists for the logged-in user,
# not root, because Installer postinstall runs as root.
CONSOLE_USER="$(stat -f %Su /dev/console 2>/dev/null || true)"
if [[ -z "$CONSOLE_USER" || "$CONSOLE_USER" == "root" ]]; then
  CONSOLE_USER="${SUDO_USER:-}"
fi
if [[ -n "$CONSOLE_USER" ]]; then
  CONSOLE_HOME="$(dscl . -read "/Users/$CONSOLE_USER" NFSHomeDirectory 2>/dev/null | awk '{print $2}')"
fi
if [[ -z "${CONSOLE_HOME:-}" ]]; then
  CONSOLE_HOME="/var/root"
fi

APP_SUPPORT="$CONSOLE_HOME/Library/Application Support/memory-layer"
mkdir -p "$APP_SUPPORT/log" "$APP_SUPPORT/run"

SHARE="/usr/local/share/memory-layer"
if [[ ! -f "$APP_SUPPORT/memory-layer.toml" && -f "$SHARE/memory-layer.toml.example" ]]; then
  cp "$SHARE/memory-layer.toml.example" "$APP_SUPPORT/memory-layer.toml"
  echo "Installed example config to $APP_SUPPORT/memory-layer.toml"
fi

if [[ ! -f "$APP_SUPPORT/memory-layer.env" ]]; then
  cat > "$APP_SUPPORT/memory-layer.env" <<'EOF'
# Shared secrets and overrides for Memory Layer CLI and background services.
# The service API token is provisioned automatically during setup.
# Example:
# OPENAI_API_KEY=replace-me
EOF
  chmod 600 "$APP_SUPPORT/memory-layer.env"
  echo "Installed shared environment file at $APP_SUPPORT/memory-layer.env"
fi

if [[ -n "$CONSOLE_USER" && "$CONSOLE_USER" != "root" ]]; then
  chown -R "$CONSOLE_USER":staff "$APP_SUPPORT" || true
fi

if command -v /usr/local/bin/memory >/dev/null 2>&1; then
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

if [[ -n "$PKG_SIGN_IDENTITY" ]]; then
  PKGBUILD_ARGS+=(--sign "$PKG_SIGN_IDENTITY")
fi

pkgbuild "${PKGBUILD_ARGS[@]}" "$PKG_PATH"

if [[ "$NOTARIZE" -eq 1 ]]; then
  echo "Submitting package for notarization..."
  NOTARY_ARGS=(submit "$PKG_PATH" --wait)
  if [[ -n "$NOTARY_PROFILE" ]]; then
    NOTARY_ARGS+=(--keychain-profile "$NOTARY_PROFILE")
  else
    NOTARY_ARGS+=(--apple-id "$APPLE_ID" --team-id "$TEAM_ID" --password "$APPLE_PASSWORD")
  fi
  xcrun notarytool "${NOTARY_ARGS[@]}"

  echo "Stapling notarization ticket..."
  xcrun stapler staple "$PKG_PATH"
  xcrun stapler validate "$PKG_PATH"
fi

echo ""
echo "Built $PKG_PATH ($(du -h "$PKG_PATH" | cut -f1))"

if [[ -n "$APP_SIGN_IDENTITY" ]]; then
  echo "Binary signed with: $APP_SIGN_IDENTITY"
fi
if [[ -n "$PKG_SIGN_IDENTITY" ]]; then
  echo "Installer signed with: $PKG_SIGN_IDENTITY"
fi
if [[ "$NOTARIZE" -eq 1 ]]; then
  echo "Notarization: complete and stapled"
fi
