#!/usr/bin/env bash
set -euo pipefail

echo "=========================================="
echo "  Building FastDB for macOS (Apple Silicon)"
echo "=========================================="

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$WORKSPACE_ROOT"

TARGET="aarch64-apple-darwin"
APP_NAME="FastDB"
VERSION="0.2.0"
DIST_DIR="$WORKSPACE_ROOT/dist/macos"
APP_BUNDLE="$DIST_DIR/$APP_NAME.app"

echo "Building release binary..."
cargo build --release --target "$TARGET" -p fastdb-app

echo "Creating macOS .app bundle at $APP_BUNDLE..."
rm -rf "$APP_BUNDLE"
mkdir -p "$APP_BUNDLE/Contents/MacOS"
mkdir -p "$APP_BUNDLE/Contents/Resources"

# Copy binary
cp "$WORKSPACE_ROOT/target/$TARGET/release/fastdb" "$APP_BUNDLE/Contents/MacOS/$APP_NAME"
chmod +x "$APP_BUNDLE/Contents/MacOS/$APP_NAME"

# Create Info.plist
cat <<EOF > "$APP_BUNDLE/Contents/Info.plist"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>$APP_NAME</string>
    <key>CFBundleIdentifier</key>
    <string>com.fastdb.FastDB</string>
    <key>CFBundleName</key>
    <string>$APP_NAME</string>
    <key>CFBundleDisplayName</key>
    <string>$APP_NAME</string>
    <key>CFBundleVersion</key>
    <string>$VERSION</string>
    <key>CFBundleShortVersionString</key>
    <string>$VERSION</string>
    <key>CFBundleIconFile</key>
    <string>$APP_NAME.icns</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key>
    <true/>
</dict>
</plist>
EOF

# Copy the app icon (UI icons are embedded in the binary)
cp "$WORKSPACE_ROOT/resources/$APP_NAME.icns" "$APP_BUNDLE/Contents/Resources/$APP_NAME.icns"

# Sign the bundle with ad-hoc signature
echo "Ad-hoc signing .app bundle..."
codesign --force --deep --sign - "$APP_BUNDLE"

# Create .dmg with the current macOS tool, or the older hdiutil fallback.
if command -v diskutil &> /dev/null || command -v hdiutil &> /dev/null; then
    DMG_PATH="$DIST_DIR/$APP_NAME-$VERSION-arm64.dmg"
    DMG_STAGING="$(mktemp -d "$DIST_DIR/dmg_staging.XXXXXX")"
    trap 'rm -rf "$DMG_STAGING"' EXIT
    cp -R "$APP_BUNDLE" "$DMG_STAGING/"
    ln -s /Applications "$DMG_STAGING/Applications"
    echo "Creating .dmg installer at $DMG_PATH..."
    bash "$SCRIPT_DIR/create-macos-dmg.sh" "$DMG_STAGING" "$DMG_PATH" "$APP_NAME"
    echo "Successfully generated $DMG_PATH"
fi

echo "macOS packaging complete!"
