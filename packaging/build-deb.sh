#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="$(awk -F '\"' '/^version = / { print $2; exit }' "$ROOT_DIR/Cargo.toml" 2>/dev/null || true)"
if [[ -z "${VERSION:-}" ]]; then
  VERSION="0.1.0"
fi

DEB_ARCH="amd64"
RUST_TARGET=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --arch)
      DEB_ARCH="$2"
      shift 2
      ;;
    --target)
      RUST_TARGET="$2"
      shift 2
      ;;
    --help|-h)
      cat <<'HELP'
Build a Memory Layer Debian package.

Usage:
  ./packaging/build-deb.sh [--arch amd64|arm64] [--target <rust-target>]

Defaults:
  amd64 -> x86_64-unknown-linux-gnu
  arm64 -> aarch64-unknown-linux-gnu
HELP
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 1
      ;;
  esac
done

case "$DEB_ARCH" in
  amd64)
    RUST_TARGET="${RUST_TARGET:-x86_64-unknown-linux-gnu}"
    ;;
  arm64)
    RUST_TARGET="${RUST_TARGET:-aarch64-unknown-linux-gnu}"
    ;;
  *)
    echo "Unsupported Debian architecture: $DEB_ARCH" >&2
    echo "Supported values: amd64, arm64" >&2
    exit 1
    ;;
esac

PKG_ROOT="$ROOT_DIR/target/debian/memory-layer-$DEB_ARCH"
BIN_PATH="$ROOT_DIR/target/$RUST_TARGET/release/memory"

rm -rf "$PKG_ROOT"
mkdir -p \
  "$PKG_ROOT/DEBIAN" \
  "$PKG_ROOT/usr/bin" \
  "$PKG_ROOT/etc/memory-layer" \
  "$PKG_ROOT/lib/systemd/system" \
  "$PKG_ROOT/usr/share/doc/memory-layer" \
  "$PKG_ROOT/usr/share/memory-layer/skill-template" \
  "$PKG_ROOT/usr/share/memory-layer/web" \
  "$PKG_ROOT/usr/share/bash-completion/completions" \
  "$PKG_ROOT/usr/share/zsh/vendor-completions" \
  "$PKG_ROOT/usr/share/fish/vendor_completions.d"

echo "Building release binary for $DEB_ARCH ($RUST_TARGET)..."
cargo build --release --manifest-path "$ROOT_DIR/Cargo.toml" --bin memory --target "$RUST_TARGET"
echo "Building web UI..."
npm --prefix "$ROOT_DIR/web" ci
npm --prefix "$ROOT_DIR/web" run build

install -m 0755 "$BIN_PATH" "$PKG_ROOT/usr/bin/memory"
"$PKG_ROOT/usr/bin/memory" completion bash > "$PKG_ROOT/usr/share/bash-completion/completions/memory"
"$PKG_ROOT/usr/bin/memory" completion zsh > "$PKG_ROOT/usr/share/zsh/vendor-completions/_memory"
"$PKG_ROOT/usr/bin/memory" completion fish > "$PKG_ROOT/usr/share/fish/vendor_completions.d/memory.fish"
install -m 0644 "$ROOT_DIR/packaging/debian/memory-layer.service" "$PKG_ROOT/lib/systemd/system/memory-layer.service"
install -m 0644 "$ROOT_DIR/packaging/debian/memory-watch.service" "$PKG_ROOT/lib/systemd/system/memory-watch.service"
install -m 0644 "$ROOT_DIR/packaging/debian/memory-layer.env" "$PKG_ROOT/etc/memory-layer/memory-layer.env"
install -m 0644 "$ROOT_DIR/memory-layer.toml.example" "$PKG_ROOT/etc/memory-layer/memory-layer.toml"
install -m 0644 "$ROOT_DIR/README.md" "$PKG_ROOT/usr/share/doc/memory-layer/README.md"
cp -R "$ROOT_DIR/.agents/skills/." "$PKG_ROOT/usr/share/memory-layer/skill-template/"
find "$PKG_ROOT/usr/share/memory-layer/skill-template" -type f -name '*.sh' -exec chmod 0755 {} +
cp -R "$ROOT_DIR/web/dist/." "$PKG_ROOT/usr/share/memory-layer/web/"

sed \
  -e "s/^Version: .*/Version: $VERSION/" \
  -e "s/^Architecture: .*/Architecture: $DEB_ARCH/" \
  "$ROOT_DIR/packaging/debian/control" > "$PKG_ROOT/DEBIAN/control"
install -m 0644 "$ROOT_DIR/packaging/debian/conffiles" "$PKG_ROOT/DEBIAN/conffiles"
install -m 0755 "$ROOT_DIR/packaging/debian/postinst" "$PKG_ROOT/DEBIAN/postinst"
install -m 0755 "$ROOT_DIR/packaging/debian/prerm" "$PKG_ROOT/DEBIAN/prerm"

DEB_PATH="$ROOT_DIR/target/debian/memory-layer_${VERSION}_${DEB_ARCH}.deb"
dpkg-deb --root-owner-group --build "$PKG_ROOT" "$DEB_PATH"

echo "Built $DEB_PATH"
