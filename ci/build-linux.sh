#!/usr/bin/env bash
set -euo pipefail

echo "=========================================="
echo "  Building FastDB for Linux (x86_64)"
echo "=========================================="

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$WORKSPACE_ROOT"

# Building elsewhere would package a host binary into Linux artifacts.
if [ "$(uname -s)" != Linux ]; then
    echo "This script requires Linux." >&2
    exit 1
fi

TARGET="x86_64-unknown-linux-gnu"
APP_NAME="fastdb"
VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)"
if [ -z "$VERSION" ]; then
    echo "Could not read the workspace version from Cargo.toml." >&2
    exit 1
fi
DIST_DIR="$WORKSPACE_ROOT/dist/linux"

echo "Building release binary for $TARGET..."
cargo build --release --locked --target "$TARGET" -p fastdb-app
BIN_SRC="$WORKSPACE_ROOT/target/$TARGET/release/fastdb"

mkdir -p "$DIST_DIR"
cp "$BIN_SRC" "$DIST_DIR/fastdb-linux-x86_64"

if command -v dpkg-shlibdeps &> /dev/null; then
    bash "$SCRIPT_DIR/package-deb.sh" "$BIN_SRC" "$VERSION" "$DIST_DIR"
    echo "Generated Debian package at $DIST_DIR/fastdb_${VERSION}_amd64.deb"
else
    echo "dpkg-shlibdeps not found (install dpkg-dev); skipping the .deb." >&2
fi

# Build AppImage structure
APP_DIR="$DIST_DIR/AppDir"
rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/usr/bin"
cp "$BIN_SRC" "$APP_DIR/usr/bin/$APP_NAME"
cp "$WORKSPACE_ROOT/resources/linux/$APP_NAME.desktop" "$APP_DIR/"
cp "$WORKSPACE_ROOT/resources/icon.svg" "$APP_DIR/$APP_NAME.svg"

cat <<EOF > "$APP_DIR/AppRun"
#!/bin/sh
HERE="\$(dirname "\$(readlink -f "\${0}")")"
exec "\${HERE}/usr/bin/$APP_NAME" "\$@"
EOF
chmod +x "$APP_DIR/AppRun"

if command -v appimagetool &> /dev/null; then
    appimagetool "$APP_DIR" "$DIST_DIR/FastDB-${VERSION}-x86_64.AppImage"
    echo "Generated AppImage at $DIST_DIR/FastDB-${VERSION}-x86_64.AppImage"
fi

echo "Linux packaging complete!"
