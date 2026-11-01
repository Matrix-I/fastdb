#!/bin/bash
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$DIR"

INSTALL=true
case "${1:-}" in
    --no-install) INSTALL=false ;;
    "") ;;
    *) echo "Usage: $0 [--no-install]" >&2; exit 1 ;;
esac
if [ "$#" -gt 1 ]; then
    echo "Usage: $0 [--no-install]" >&2
    exit 1
fi
if [ "$(uname -s)" != Darwin ]; then
    echo "This script requires macOS." >&2
    exit 1
fi

APP_NAME="FastDB"
DIST_DIR="$DIR/dist"
APP_DIR="$DIST_DIR/$APP_NAME.app"
CONTENTS="$APP_DIR/Contents"
VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)"
if [ -z "$VERSION" ]; then
    echo "Could not read the workspace version from Cargo.toml." >&2
    exit 1
fi

echo "==> Building optimized macOS release binary..."
if [ -z "${SDKROOT:-}" ]; then
    SDKROOT="$(xcrun --show-sdk-path)"
    export SDKROOT
fi
if [ -f "$HOME/.cargo/env" ]; then
    # shellcheck source=/dev/null
    . "$HOME/.cargo/env"
fi
# Use the host architecture, regardless of Cargo target settings in the environment.
HOST_TARGET="$(rustc -vV | sed -n 's/^host: //p')"
cargo build --release --target "$HOST_TARGET" -p fastdb-app
TARGET_DIR="${CARGO_TARGET_DIR:-$DIR/target}"
BINARY="$TARGET_DIR/$HOST_TARGET/release/fastdb"

echo "==> Packaging $APP_NAME.app..."
rm -rf "$APP_DIR"
mkdir -p "$CONTENTS/MacOS" "$CONTENTS/Resources"
cp "$BINARY" "$CONTENTS/MacOS/$APP_NAME"
chmod +x "$CONTENTS/MacOS/$APP_NAME"
cp "$DIR/resources/$APP_NAME.icns" "$CONTENTS/Resources/$APP_NAME.icns"

cat > "$CONTENTS/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key><string>$APP_NAME</string>
    <key>CFBundleIdentifier</key><string>com.fastdb.FastDB</string>
    <key>CFBundleName</key><string>$APP_NAME</string>
    <key>CFBundleDisplayName</key><string>$APP_NAME</string>
    <key>CFBundleVersion</key><string>$VERSION</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleIconFile</key><string>$APP_NAME.icns</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict>
</plist>
EOF

codesign --force --deep --sign - "$APP_DIR"
touch "$APP_DIR"

DMG_PATH="$DIST_DIR/$APP_NAME.dmg"
DMG_STAGING="$(mktemp -d "$DIST_DIR/dmg_staging.XXXXXX")"
trap 'rm -rf "$DMG_STAGING"' EXIT
echo "==> Creating installable disk image..."
cp -R "$APP_DIR" "$DMG_STAGING/"
ln -s /Applications "$DMG_STAGING/Applications"
hdiutil create -volname "$APP_NAME" -srcfolder "$DMG_STAGING" \
    -ov -format UDZO "$DMG_PATH"

echo "==> Creating standalone binary and zip archive..."
cp "$BINARY" "$DIST_DIR/fastdb-macos"
chmod +x "$DIST_DIR/fastdb-macos"
(cd "$DIST_DIR" && rm -f FastDB-macos.zip && zip -q -r FastDB-macos.zip "$APP_NAME.app")
ARCH="$(uname -m)"
[ "$ARCH" != arm64 ] || ARCH="aarch64"
cp "$DIST_DIR/fastdb-macos" "$DIST_DIR/fastdb-macos-$ARCH"
cp "$DIST_DIR/FastDB-macos.zip" "$DIST_DIR/FastDB-macos-$ARCH.zip"

if [ "$INSTALL" = true ] && [ -d "$HOME/Applications" ]; then
    echo "==> Installing to $HOME/Applications/$APP_NAME.app..."
    rm -rf "$HOME/Applications/$APP_NAME.app"
    cp -R "$APP_DIR" "$HOME/Applications/"
    codesign --force --deep --sign - "$HOME/Applications/$APP_NAME.app"
    touch "$HOME/Applications/$APP_NAME.app"
fi

echo "==> Done! Outputs ready in $DIST_DIR:"
ls -ldh "$APP_DIR" "$DMG_PATH" "$DIST_DIR/fastdb-macos" "$DIST_DIR/FastDB-macos.zip"
