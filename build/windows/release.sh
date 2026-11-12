#!/bin/bash
# BrowseCraft for Windows, from a patch to an installer on the share, in one command.
#
#   build/windows/release.sh            apply what patches/series has that the Windows tree
#                                       has not had yet, rebuild, package, publish
#   build/windows/release.sh --no-publish   the same, but leave the installer in dist/
#   build/windows/release.sh --build-only   patches and the build only (a full rebuild, say)
#
# 2026-10-21: "better to have that facility than work arounds". The Windows tree on
# the build host (browsecraft/win/src) keeps its out/Win, so ninja rebuilds only what a patch
# touches: a small patch is one or two minutes of compiling and a 40-75 s link. Only a new
# Chromium version (or anything that changes the toolchain, see README step 5) means the
# full build again.
#
# Steps:
#   1. patches: each one in patches/series not listed in win/src/.browsecraft-applied is
#      dry-run, applied and recorded
#   2. version: build/version.py next, stamped into the tree
#   3. build: win-build.sh in chromium-builder-win, in the foreground, memory capped so the
#      other services on it are never starved
#   4. zip: ungoogled's filescfg.py (FILES.cfg, snapshot_blob.bin included)
#   5. installer: packaging/windows/build-installer.sh (WiX on the Windows build host)
#   6. publish: BrowseCraft-v<version>.exe + .sha256 to the share, older ones to old/
set -euo pipefail
cd "$(dirname "$0")/../.."
PUBLISH=1; BUILD_ONLY=0
[ "${1:-}" = "--no-publish" ] && PUBLISH=0
[ "${1:-}" = "--build-only" ] && BUILD_ONLY=1   # patches + build, no version, zip or installer (e.g. a full rebuild)

BUILD_HOST=${BUILD_HOST:?set BUILD_HOST to the Linux build host (user@host)}
SSH="ssh $BUILD_HOST"
SCP="scp -q"
B=/srv/browsecraft
W=$B/win/src
SHARE=/srv/share/BrowseCraft
# Memory decides the job count: Chromium's big compiles want ~1.8 GB each, and earlier full
# builds at 18 jobs in 22 GB (1.2 GB a job) spent 10 minutes on single Blink files. The cap
# leaves 12 GB of what is free for everything else on the build host (other services).
avail=$($SSH "awk '/MemAvailable/ {print int(\$2/1048576)}' /proc/meminfo")
memgb=$(( avail - 12 )); [ $memgb -gt 40 ] && memgb=40
[ $memgb -lt 8 ] && { echo "only ${avail} GB free on the build host: not building now" >&2; exit 1; }
JOBS=${NINJA_JOBS:-$(( memgb * 10 / 18 ))}; [ $JOBS -gt 36 ] && JOBS=36; [ $JOBS -lt 4 ] && JOBS=4
MEM=${BUILD_MEMORY:-${memgb}g}
IMAGE=chromium-builder-win:trixie
t0=$(date +%s)
say() { echo "[$(date +%H:%M:%S)] $*"; }

# 1. Patches the Windows tree has not had.
$SSH "test -f $W/.browsecraft-applied" || { echo "$W/.browsecraft-applied is missing: list the patches already in the tree there first" >&2; exit 1; }
applied=$($SSH "cat $W/.browsecraft-applied")
while read -r p; do
  [ -z "$p" ] && continue
  case "$p" in \#*) continue ;; esac
  grep -qx "$p" <<<"$applied" && continue
  say "applying $p"
  $SCP "patches/$p" "$BUILD_HOST:/tmp/bc-$(basename "$p")"
  $SSH "cd $W && patch -p1 --forward --dry-run -s -i /tmp/bc-$(basename "$p") && patch -p1 --forward -s -i /tmp/bc-$(basename "$p") && echo '$p' >> .browsecraft-applied; rc=\$?; rm -f /tmp/bc-$(basename "$p"); exit \$rc"
done < patches/series

# 2. Version.
if [ $BUILD_ONLY = 1 ]; then VER=build-only; else
VER=$(python3 build/version.py next)
CHROMIUM=$($SSH "sed -n 's/^MAJOR=//p;s/^MINOR=//p;s/^BUILD=//p;s/^PATCH=//p' $W/chrome/VERSION" | paste -sd.)
say "BrowseCraft v$VER (Chromium $CHROMIUM)"
$SCP build/version.py "$BUILD_HOST:$B/version.py"
$SSH "docker run --rm -u 1000:1000 -v $W:/src -v $B/version.py:/version.py $IMAGE python3 /version.py stamp /src $VER"
fi

# 3. Build: only what changed.
say "building ($JOBS jobs, $MEM)"
$SSH "set -o pipefail; docker rm -f browsecraft-chromium-win >/dev/null 2>&1; docker run --rm --name browsecraft-chromium-win \
  --device /dev/fuse --cap-add SYS_ADMIN --memory=$MEM --memory-swap=$MEM -u 1000:1000 \
  -e TARGETS=chrome -e NINJA_JOBS=$JOBS -e GYP_MSVS_HASH_3bfcb536c8=3cc1a8b0ee -e DEPOT_TOOLS_WIN_TOOLCHAIN_BASE_URL=/tc/ \
  -v $B/portablelinux/ungoogled-chromium:/flags -v $B/portablelinux/build/src/out/Default/gn:/linux-gn \
  -v $W:/src -v $B/win-toolchain:/tc -v $B/win/win-build.sh:/win-build.sh \
  -e FORCE_FULL=${FORCE_FULL:-0} --cpuset-cpus=0-17,36-53 \
  $IMAGE bash /win-build.sh 2>&1 | grep -E '^\[[0-9]+/[0-9]+\] (LINK|CXX)|FAILED|error:|ninja: (build stopped|no work)|clang changed' | tail -30"
say "built in $(( ($(date +%s) - t0) / 60 )) min so far"
[ $BUILD_ONLY = 1 ] && exit 0

# 4. Zip.
ZIP=browsecraft-$CHROMIUM-v$VER-windows-x64.zip
$SSH "docker run --rm -u 1000:1000 -v $W:/src -v $B/portablelinux/ungoogled-chromium:/flags -w /src $IMAGE \
  python3 /flags/utils/filescfg.py -c chrome/tools/build/win/FILES.cfg --build-outputs out/Win --cpu-arch 64bit archive -o /src/out/$ZIP"
mkdir -p dist
$SCP "$BUILD_HOST:$W/out/$ZIP" dist/
$SSH "rm -f $W/out/$ZIP"
# (the listing is read whole: grep -q would close the pipe early and pipefail call it a failure)
listing=$(unzip -l "dist/$ZIP")
grep -q snapshot_blob.bin <<<"$listing" || { echo "the zip has no snapshot_blob.bin: renderers would die" >&2; exit 1; }

# 5. Installer.
say "installer"
packaging/windows/build-installer.sh "dist/$ZIP" dist
EXE=BrowseCraft-v$VER.exe

# 6. Publish.
if [ $PUBLISH = 1 ]; then
  $SSH "mkdir -p $SHARE/old && cd $SHARE && for f in BrowseCraft-v*.exe BrowseCraft-v*.exe.sha256; do [ -e \"\$f\" ] && mv \"\$f\" old/; done; true"
  $SCP "dist/$EXE" "dist/$EXE.sha256" "$BUILD_HOST:$SHARE/"
  $SSH "cd $SHARE && chown nobody:users $EXE $EXE.sha256 && chmod 664 $EXE $EXE.sha256 && sha256sum -c $EXE.sha256"
  say "published the release share\\$EXE"
fi
say "done in $(( ($(date +%s) - t0) / 60 )) min"
