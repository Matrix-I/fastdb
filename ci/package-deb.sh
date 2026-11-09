#!/usr/bin/env bash
# Package an existing release executable as a .deb without rebuilding it.
# Needs dpkg-deb and dpkg-shlibdeps (dpkg-dev) on Debian/Ubuntu.
set -euo pipefail
umask 022
root=$(cd "$(dirname "$0")/.." && pwd)
if [[ $# != 3 ]]; then
    echo "Usage: $0 BINARY VERSION OUTPUT_DIRECTORY" >&2
    exit 1
fi
binary=$1
version=$2
output=$3
# Release candidates use Debian's ~ (X.Y.Z~rc.N) so apt orders them before X.Y.Z.
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(~rc\.[0-9]+)?$ ]]; then
    echo 'VERSION must be a numeric MAJOR.MINOR.PATCH release, optionally with ~rc.N.' >&2
    exit 1
fi
if [[ ! -f "$binary" ]]; then
    echo 'BINARY must be an existing Linux x86_64 executable.' >&2
    exit 1
fi
# Check ELF64, little endian, ET_EXEC/ET_DYN, and the x86_64 machine type.
header=$(od -An -tx1 -N20 "$binary" | tr -d ' \n')
if [[ ! "$header" =~ ^7f454c46020101[0-9a-f]{18}0[23]003e00$ ]]; then
    echo 'BINARY must be a little-endian Linux x86_64 ELF executable.' >&2
    exit 1
fi
for tool in dpkg-deb dpkg-shlibdeps; do
    if ! command -v "$tool" >/dev/null; then
        echo "$tool is required (install dpkg-dev)." >&2
        exit 1
    fi
done

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
pkg="$stage/pkg"
mkdir -p "$pkg/DEBIAN" "$pkg/usr/bin" "$pkg/usr/share/applications" \
    "$pkg/usr/share/icons/hicolor/scalable/apps"
install -m 755 "$binary" "$pkg/usr/bin/fastdb"
install -m 644 "$root/resources/linux/fastdb.desktop" "$pkg/usr/share/applications/fastdb.desktop"
install -m 644 "$root/resources/icon.svg" "$pkg/usr/share/icons/hicolor/scalable/apps/fastdb.svg"

# dpkg-shlibdeps maps every linked library (GPUI's X11/Wayland/Vulkan stack
# included) to the package that provides it on the build host.
mkdir -p "$stage/debian"
printf 'Source: fastdb\n\nPackage: fastdb\nArchitecture: amd64\n' > "$stage/debian/control"
depends=$(cd "$stage" && dpkg-shlibdeps -O "-e$pkg/usr/bin/fastdb" | sed -n 's/^shlibs:Depends=//p')
if [[ -z "$depends" ]]; then
    echo 'dpkg-shlibdeps returned no dependencies.' >&2
    exit 1
fi

installed_size=$(du -sk "$pkg/usr" | awk '{print $1}')
cat > "$pkg/DEBIAN/control" <<CONTROL
Package: fastdb
Version: $version
Section: utils
Priority: optional
Architecture: amd64
Maintainer: FastDB Team <support@fastdb.dev>
Installed-Size: $installed_size
Depends: $depends
Homepage: https://github.com/Matrix-I/fastdb
Description: Next-Gen Pure Rust Database Client
 High-performance database client with zero-copy memory management,
 120 FPS virtualized data grid, and embedded AST completion.
CONTROL
chmod 755 "$pkg" "$pkg/DEBIAN"
chmod 644 "$pkg/DEBIAN/control"
mkdir -p "$output"
dpkg-deb --root-owner-group -Zxz --build "$pkg" "$output/fastdb_${version}_amd64.deb"
