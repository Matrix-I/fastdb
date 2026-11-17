#!/usr/bin/env bash
# Build and package FastDB for the local Linux desktop.
set -euo pipefail

if [[ $# -ne 0 ]]; then
    echo "Usage: $0" >&2
    exit 1
fi

if [[ "$(uname -s)" != Linux || "$(uname -m)" != x86_64 ]]; then
    echo "This script requires Linux x86_64." >&2
    exit 1
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec bash "$root/ci/build-linux.sh"
