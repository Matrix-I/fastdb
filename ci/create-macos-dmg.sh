#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 3 ]; then
    echo "Usage: $0 <source-folder> <output.dmg> <volume-name>" >&2
    exit 1
fi

SOURCE_DIR="$1"
DMG_PATH="$2"
VOLUME_NAME="$3"

# Older macOS versions lack `diskutil image create from`, and macOS 15 has it
# without --volumeName, so use diskutil only when the option is supported.
DISKUTIL_HELP="$(diskutil image create from --help 2>&1 || true)"
if [[ "$DISKUTIL_HELP" == *--volumeName* ]]; then
    rm -f "$DMG_PATH"
    diskutil image create from --format UDZO --volumeName "$VOLUME_NAME" \
        "$SOURCE_DIR" "$DMG_PATH"
else
    hdiutil create -volname "$VOLUME_NAME" -srcfolder "$SOURCE_DIR" \
        -ov -format UDZO "$DMG_PATH"
fi
