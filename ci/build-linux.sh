#!/usr/bin/env bash
set -euo pipefail

echo "=========================================="
echo "  Building FastDB for Linux (x86_64)"
echo "=========================================="

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$WORKSPACE_ROOT"

TARGET="x86_64-unknown-linux-gnu"
APP_NAME="fastdb"
VERSION="0.1.0"
DIST_DIR="$WORKSPACE_ROOT/dist/linux"
DEB_DIR="$DIST_DIR/deb/fastdb_${VERSION}_amd64"

echo "Building release binary for $TARGET..."
cargo build --release --target "$TARGET" -p fastdb-app || {
    echo "Native compilation on current host fallback..."
    cargo build --release -p fastdb-app
}

BIN_SRC="$WORKSPACE_ROOT/target/$TARGET/release/fastdb"
if [ ! -f "$BIN_SRC" ]; then
    BIN_SRC="$WORKSPACE_ROOT/target/release/fastdb"
fi

echo "Creating Debian package structure..."
rm -rf "$DEB_DIR"
mkdir -p "$DEB_DIR/usr/bin"
mkdir -p "$DEB_DIR/usr/share/applications"
mkdir -p "$DEB_DIR/usr/share/icons/hicolor/scalable/apps"
mkdir -p "$DEB_DIR/DEBIAN"

cp "$BIN_SRC" "$DEB_DIR/usr/bin/$APP_NAME"
chmod +x "$DEB_DIR/usr/bin/$APP_NAME"

# Copy Icon
if [ -f "$WORKSPACE_ROOT/resources/icon.svg" ]; then
    cp "$WORKSPACE_ROOT/resources/icon.svg" "$DEB_DIR/usr/share/icons/hicolor/scalable/apps/$APP_NAME.svg"
fi

# Create desktop entry
cat <<EOF > "$DEB_DIR/usr/share/applications/$APP_NAME.desktop"
[Desktop Entry]
Name=FastDB
Comment=Next-Gen Pure Rust Database Client
Exec=$APP_NAME
Icon=$APP_NAME
Terminal=false
Type=Application
Categories=Development;Database;
StartupNotify=true
EOF

# Create Debian control file
cat <<EOF > "$DEB_DIR/DEBIAN/control"
Package: $APP_NAME
Version: $VERSION
Section: utils
Priority: optional
Architecture: amd64
Maintainer: FastDB Team <support@fastdb.dev>
Description: Next-Gen Pure Rust Database Client
 High-performance database client with zero-copy memory management,
 120 FPS virtualized data grid, and embedded AST completion.
EOF

# Build .deb if dpkg-deb is available
if command -v dpkg-deb &> /dev/null; then
    dpkg-deb --build "$DEB_DIR" "$DIST_DIR/fastdb_${VERSION}_amd64.deb"
    echo "Generated Debian package at $DIST_DIR/fastdb_${VERSION}_amd64.deb"
fi

# Build AppImage structure
APP_DIR="$DIST_DIR/AppDir"
mkdir -p "$APP_DIR/usr/bin"
mkdir -p "$APP_DIR/usr/share/icons"
cp "$BIN_SRC" "$APP_DIR/usr/bin/$APP_NAME"
cp "$DEB_DIR/usr/share/applications/$APP_NAME.desktop" "$APP_DIR/"
if [ -f "$WORKSPACE_ROOT/resources/icon.svg" ]; then
    cp "$WORKSPACE_ROOT/resources/icon.svg" "$APP_DIR/$APP_NAME.svg"
fi

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
