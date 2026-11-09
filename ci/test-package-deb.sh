#!/usr/bin/env bash
# Checks ci/package-deb.sh. Run on Debian/Ubuntu with dpkg-dev, binutils and
# desktop-file-utils installed, passing any Linux x86_64 executable.
set -euo pipefail
export LC_ALL=C
root=$(cd "$(dirname "$0")/.." && pwd)
if [[ $# != 1 ]]; then
    echo "Usage: $0 RELEASE_BINARY" >&2
    exit 1
fi
binary=$1
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
trap 'echo "test-package-deb.sh: check failed at line $LINENO" >&2' ERR
package="$work/output/fastdb_9.8.7_amd64.deb"
(umask 077; bash "$root/ci/package-deb.sh" "$binary" 9.8.7 "$work/output")

[[ $(dpkg-deb -f "$package" Package) == fastdb ]]
[[ $(dpkg-deb -f "$package" Version) == 9.8.7 ]]
[[ $(dpkg-deb -f "$package" Architecture) == amd64 ]]
# Depends comes from dpkg-shlibdeps, so it always names the C library.
dpkg-deb -f "$package" Depends | grep -Eq '(^|, )libc6 \(>= [0-9.]+\)'

dpkg-deb -x "$package" "$work/extracted"
cmp "$binary" "$work/extracted/usr/bin/fastdb"
cmp "$root/resources/icon.svg" "$work/extracted/usr/share/icons/hicolor/scalable/apps/fastdb.svg"
desktop="$work/extracted/usr/share/applications/fastdb.desktop"
desktop-file-validate "$desktop"
grep -Fxq 'Exec=fastdb' "$desktop"
grep -Fxq 'Icon=fastdb' "$desktop"
grep -Fxq 'Terminal=false' "$desktop"
[[ $(stat -c %a "$work/extracted/usr/bin/fastdb") == 755 ]]
[[ $(stat -c %a "$desktop") == 644 ]]
while IFS= read -r directory; do
    [[ $(stat -c %a "$directory") == 755 ]]
done < <(find "$work/extracted/usr" -type d)
dpkg-deb -c "$package" | awk '$2 != "root/root" { bad=1 } END { exit bad }'
readelf -h "$work/extracted/usr/bin/fastdb" | grep -q 'Advanced Micro Devices X86-64'

# Release candidates use Debian's ~ so apt orders them before the release.
bash "$root/ci/package-deb.sh" "$binary" 9.8.7~rc.2 "$work/rc" > /dev/null
[[ $(dpkg-deb -f "$work/rc/fastdb_9.8.7~rc.2_amd64.deb" Version) == 9.8.7~rc.2 ]]
dpkg --compare-versions 9.8.7~rc.2 lt 9.8.7

expect_failure() {
    if bash "$root/ci/package-deb.sh" "$@" > "$work/error" 2>&1; then
        echo "Expected packaging to reject: $*" >&2
        exit 1
    fi
}
expect_failure
grep -q 'Usage' "$work/error"
expect_failure "$binary" '1.2.3/invalid' "$work/output"
grep -q 'VERSION must' "$work/error"
expect_failure "$binary" '1.2.3-rc.1' "$work/output"
grep -q 'VERSION must' "$work/error"
expect_failure "$work/missing" 1.2.3 "$work/output"
grep -q 'BINARY must' "$work/error"
printf 'not an ELF executable\n' > "$work/invalid"
expect_failure "$work/invalid" 1.2.3 "$work/output"
grep -q 'ELF executable' "$work/error"
expect_failure "$binary" 1.2.3~beta.1 "$work/output"
grep -q 'VERSION must' "$work/error"
mkdir -p "$work/bin"
for tool in bash dirname od tr; do ln -s "$(command -v "$tool")" "$work/bin/$tool"; done
if PATH="$work/bin" "$work/bin/bash" "$root/ci/package-deb.sh" "$binary" 1.2.3 "$work/output" > "$work/error" 2>&1; then
    echo 'Expected packaging to fail without dpkg tools' >&2
    exit 1
fi
grep -q 'dpkg-deb is required' "$work/error"

echo 'Debian packaging tests passed (metadata, Depends, payload, launcher, icon, modes, ownership, release candidates, invalid inputs, missing tools).'
