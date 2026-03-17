#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PKG_ROOT="$ROOT_DIR/target/debian/memory-layer"
VERSION="$(awk -F '\"' '/^version = / { print $2; exit }' "$ROOT_DIR/Cargo.toml" 2>/dev/null || true)"
if [[ -z "${VERSION:-}" ]]; then
  VERSION="0.1.0"
fi

rm -rf "$PKG_ROOT"
mkdir -p \
  "$PKG_ROOT/DEBIAN" \
  "$PKG_ROOT/usr/bin" \
  "$PKG_ROOT/etc/memory-layer" \
  "$PKG_ROOT/lib/systemd/system" \
  "$PKG_ROOT/usr/share/doc/memory-layer" \
  "$PKG_ROOT/usr/share/memory-layer/skill-template"

echo "Building release binaries..."
cargo build --release --manifest-path "$ROOT_DIR/Cargo.toml" --bin mem-cli --bin mem-service --bin memory-watch

install -m 0755 "$ROOT_DIR/target/release/mem-cli" "$PKG_ROOT/usr/bin/mem-cli"
install -m 0755 "$ROOT_DIR/target/release/mem-service" "$PKG_ROOT/usr/bin/mem-service"
install -m 0755 "$ROOT_DIR/target/release/memory-watch" "$PKG_ROOT/usr/bin/memory-watch"
install -m 0644 "$ROOT_DIR/packaging/debian/memory-layer.service" "$PKG_ROOT/lib/systemd/system/memory-layer.service"
install -m 0644 "$ROOT_DIR/packaging/debian/memory-watch.service" "$PKG_ROOT/lib/systemd/system/memory-watch.service"
install -m 0644 "$ROOT_DIR/packaging/debian/memory-layer.env" "$PKG_ROOT/etc/memory-layer/memory-layer.env"
install -m 0644 "$ROOT_DIR/memory-layer.toml.example" "$PKG_ROOT/etc/memory-layer/memory-layer.toml"
install -m 0644 "$ROOT_DIR/README.md" "$PKG_ROOT/usr/share/doc/memory-layer/README.md"
cp -R "$ROOT_DIR/.agents/skills/memory-layer/." "$PKG_ROOT/usr/share/memory-layer/skill-template/"
find "$PKG_ROOT/usr/share/memory-layer/skill-template" -type f -path '*/scripts/*' -exec chmod 0755 {} +

sed "s/^Version: .*/Version: $VERSION/" "$ROOT_DIR/packaging/debian/control" > "$PKG_ROOT/DEBIAN/control"
install -m 0755 "$ROOT_DIR/packaging/debian/postinst" "$PKG_ROOT/DEBIAN/postinst"
install -m 0755 "$ROOT_DIR/packaging/debian/prerm" "$PKG_ROOT/DEBIAN/prerm"

DEB_PATH="$ROOT_DIR/target/debian/memory-layer_${VERSION}_amd64.deb"
dpkg-deb --root-owner-group --build "$PKG_ROOT" "$DEB_PATH"

echo "Built $DEB_PATH"
