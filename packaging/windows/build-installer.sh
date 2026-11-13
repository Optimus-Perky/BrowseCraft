#!/bin/bash
# Makes BrowseCraft.exe, the Windows installer (an MSI inside a WiX Burn bundle), from the
# Windows build zip.
#
#   packaging/windows/build-installer.sh <browsecraft-<chromium>-v<browsecraft>-windows-x64.zip> [out dir]
#
# The file is BrowseCraft-v<browsecraft>.exe, e.g. BrowseCraft-v1.0.261120.exe: the version
# build/version.py stamped into that build (Settings > About shows it), read from the zip's name
# and added to build/builds.log. Windows Installer cannot hold a date in a version, so the
# installer's own version is Chromium's three numbers and BrowseCraft's build (this repo's commit count),
# e.g. 155.0.8059.25: the fourth Chromium number gives way so that every BrowseCraft build of
# one Chromium version is newer than the last.
#
# WiX needs Windows (it builds the database through msi.dll), so the files go over SSH to a
# host with the WiX 3.14 binaries in C:\wix314. Set WIX_HOST / WIX_USER / WIX_KEY to move it.
set -euo pipefail
cd "$(dirname "$0")/../.."
ZIP=${1:?usage: build-installer.sh <windows build zip> [out dir]}
OUT=${2:-dist}
CHROMIUM=$(basename "$ZIP" | sed -n 's/^browsecraft-\([0-9]*\.[0-9]*\.[0-9]*\)\..*/\1/p')
[ -n "$CHROMIUM" ] || { echo "cannot read the Chromium version from $ZIP" >&2; exit 1; }
BCVER=$(basename "$ZIP" | sed -n 's/.*-v\([0-9]*\.[0-9]*\.[0-9]\{6\}\(\.[0-9]*\|[a-z]\)\{0,1\}\)-.*/\1/p')   # 0.8.261013.1, 1.0.261120, 1.0.261120a
CHROMIUM_FULL=$(basename "$ZIP" | sed -n 's/^browsecraft-\([0-9.]*[0-9]\)-v.*/\1/p')
[ -n "$BCVER" ] || { echo "no BrowseCraft version (-v<version>-) in $ZIP: stamp the build with build/version.py" >&2; exit 1; }
EXE="BrowseCraft-v$BCVER.exe"
VERSION="$CHROMIUM.$(git rev-list --count HEAD)"
WIX_HOST=${WIX_HOST:-wix-build-host}
WIX_USER=${WIX_USER:-Administrator}
WIX_KEY=${WIX_KEY:-$HOME/.ssh/id_ed25519}
SSH="ssh -i $WIX_KEY -o StrictHostKeyChecking=no -o BatchMode=yes $WIX_USER@$WIX_HOST"
SCP="scp -q -i $WIX_KEY -o StrictHostKeyChecking=no"

$SSH "powershell -NoProfile -Command \"Remove-Item C:\bcbuild -Recurse -Force -EA 0; New-Item -ItemType Directory -Path C:\bcbuild\obj -Force | Out-Null\"" >/dev/null
$SCP "$ZIP" assets/browsecraft.ico assets/browsecraft-logo-256.png packaging/windows/browsecraft.wxs packaging/windows/bundle.wxs \
     packaging/windows/wixbuild.ps1 packaging/windows/Update-Widevine.ps1 "$WIX_USER@$WIX_HOST:C:/bcbuild/"
$SSH "powershell -NoProfile -ExecutionPolicy Bypass -File C:\\bcbuild\\wixbuild.ps1 -Version $VERSION -BrowseCraftVersion $BCVER"
mkdir -p "$OUT"
$SCP "$WIX_USER@$WIX_HOST:C:/bcbuild/BrowseCraft.exe" "$OUT/$EXE"
( cd "$OUT" && sha256sum "$EXE" | tee "$EXE.sha256" )
echo "$VERSION" > "$OUT/$EXE.version"   # the bundle's version (what Windows lists as installed)
python3 build/version.py record "$BCVER" "$CHROMIUM_FULL"
$SSH "powershell -NoProfile -Command \"Remove-Item C:\bcbuild -Recurse -Force -EA 0\"" >/dev/null
