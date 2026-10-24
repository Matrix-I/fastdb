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
VERSION="0.1.0"
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
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key>
    <true/>
</dict>
</plist>
EOF

# Copy application icons if available
if [ -d "$WORKSPACE_ROOT/assets/icons" ]; then
    cp -r "$WORKSPACE_ROOT/assets/icons" "$APP_BUNDLE/Contents/Resources/"
fi

# Create .dmg if hdiutil is available
if command -v hdiutil &> /dev/null; then
    DMG_PATH="$DIST_DIR/$APP_NAME-$VERSION-arm64.dmg"
    echo "Creating .dmg installer at $DMG_PATH..."
    rm -f "$DMG_PATH"
    hdiutil create -volname "$APP_NAME" -srcfolder "$APP_BUNDLE" -ov -format UDZO "$DMG_PATH"
    echo "Successfully generated $DMG_PATH"
fi

echo "macOS packaging complete!"
