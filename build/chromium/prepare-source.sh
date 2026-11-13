#!/bin/bash
# BrowseCraft source tree from Chromium's own release tarball (2026-11-13: "the middle way":
# Chromium's release + ungoogled-chromium's CORE patches, pinned in third_party/, + ours; no
# dependency on ungoogled's per-platform projects).
#
#   prepare-source.sh <repo> <chromium version> <tarball dir> <tree>
#
# Runs inside chromium-builder (python3, xz, patch). Makes <tree>/src, ready for linux-build.sh
# or win-prepare.sh. Every stage leaves a stamp in <tree>, so a re-run continues where it stopped;
# a patch that does not apply stops the run (exit 2) with the patch named in the log.
#
# Order, ported from ungoogled-chromium-portablelinux scripts/shared.sh (156.0.8078.79-1):
#   1. unpack chromium-<ver>-lite.tar.xz (sha256 checked against Google's .hashes file)
#   2. prune binaries (core pruning.list; a listed file that is not there is only a warning,
#      so a point release of a pinned core still goes through)
#   3. patches, each dry-run first and then applied (-p1, whitespace ignored, as ungoogled's
#      utils/patches.py): ungoogled core series (less patches/core-skip), then patches/portablelinux/series (build-host
#      fixes for building on Linux, used by both the Linux and the Windows trees), then
#      BrowseCraft's patches/series
#   4. domain substitution (core lists)
#   5. BrowseCraft hooks that need the real addresses back: webstore.py, extstore.py,
#      profile.py, identity.py (in that order; they must run after 4); then the BrowseCraft
#      name and icons (build/branding)
#   6. tool downloads: put back the real addresses of Chromium's toolchain buckets
#      (portablelinux fix_tool_downloading, ungoogled-chromium issue 1846)
set -euo pipefail
REPO=$(cd "$1" && pwd); VER=$2; DL=$(cd "$3" && pwd); T=$4
UC=$REPO/third_party/ungoogled-chromium
SRC=$T/src
mkdir -p "$T"
say() { echo "[$(date +%H:%M:%S)] $*"; }
done_() { [ -f "$T/.stage-$1" ]; }
mark() { touch "$T/.stage-$1"; }

apply_series() {   # <name> <series file> <patch dir> [skip list]: each patch dry-run, then applied
  local name=$1 series=$2 dir=$3 skip=${4:-/dev/null} n=0 rec="$T/.applied-$1"
  done_ "patch-$name" && return 0
  touch "$rec"
  while read -r p; do
    p=${p%%#*}; p=$(echo "$p" | xargs)
    [ -z "$p" ] && continue
    if grep -v '^#' "$skip" | grep -qxF "$p"; then say "$name: leaving out $p (patches/core-skip)"; continue; fi
    grep -qxF "$p" "$rec" && continue    # applied by an earlier, interrupted run
    if ! patch -p1 --ignore-whitespace --no-backup-if-mismatch --forward --dry-run -s -d "$SRC" -i "$dir/$p" > "$T/patch.out" 2>&1; then
      say "PATCH FAILED (dry run): $name: $p"; cat "$T/patch.out"
      echo "$name: $p" > "$T/failed-patch"
      exit 2
    fi
    patch -p1 --ignore-whitespace --no-backup-if-mismatch --forward -s -d "$SRC" -i "$dir/$p"
    echo "$p" >> "$rec"
    n=$((n+1))
  done < "$series"
  say "$name: $n patches applied"
  mark "patch-$name"
}

# 1. Unpack
if ! done_ unpack; then
  TB=$DL/chromium-$VER-lite.tar.xz
  want=$(awk '$1=="sha256" {print $2}' "$TB.hashes")
  say "checking $TB"
  echo "$want  $TB" | sha256sum -c --quiet -
  rm -rf "$SRC"; mkdir -p "$SRC"
  say "unpacking"
  xz -T8 -dc "$TB" | tar -x --strip-components=1 -C "$SRC"
  grep -q "^MAJOR=${VER%%.*}$" "$SRC/chrome/VERSION" || { echo "tarball is not Chromium $VER" >&2; exit 1; }
  mark unpack
fi

# 2. Prune
if ! done_ prune; then
  python3 -I - "$UC/utils" "$SRC" "$UC/pruning.list" <<'PY'
import sys
from pathlib import Path
sys.path.insert(0, sys.argv[1])
import prune_binaries
gone = prune_binaries.prune_files(Path(sys.argv[2]), Path(sys.argv[3]).read_text().splitlines())
for f in sorted(gone):
    print("warning: not in this tarball, nothing to prune:", f)
PY
  mark prune
fi

# 3. Patches
apply_series core "$UC/patches/series" "$UC/patches" "$REPO/patches/core-skip"
apply_series portablelinux "$REPO/patches/portablelinux/series" "$REPO/patches/portablelinux"
apply_series browsecraft "$REPO/patches/series" "$REPO/patches"

# 4. Domain substitution
if ! done_ domsub; then
  say "domain substitution"
  python3 "$UC/utils/domain_substitution.py" apply -r "$UC/domain_regex.list" -f "$UC/domain_substitution.list" "$SRC" 2>&1 \
    | grep -v -e 'Path has no substitutions' -e 'Skipping non-existent path' || true
  mark domsub
fi

# 5. BrowseCraft hooks
for h in webstore extstore profile identity updatecheck; do
  done_ "hook-$h" && continue
  (cd "$SRC" && python3 "$REPO/build/$h.py")
  mark "hook-$h"
done

# 5b. Name and icons (build/branding: make-branding.sh needs resvg, RESVG=path)
if ! done_ branding; then
  [ -d "$REPO/build/branding/out" ] || RESVG=${RESVG:-resvg} sh "$REPO/build/branding/make-branding.sh" > "$T/branding.log"
  sh "$REPO/build/branding/rebrand.sh" "$SRC"
  mark branding
fi

# 6. Tool downloads
if ! done_ tools; then
  sed -i 's/commondatastorage.9oo91eapis.qjz9zk/commondatastorage.googleapis.com/g' \
    "$SRC/build/linux/sysroot_scripts/sysroots.json" "$SRC/tools/clang/scripts/update.py" "$SRC/tools/clang/scripts/build.py"
  sed -i 's/chromium.9oo91esource.qjz9zk/chromium.googlesource.com/g' \
    "$SRC/tools/clang/scripts/build.py" "$SRC/tools/rust/build_rust.py" "$SRC/tools/rust/build_bindgen.py"
  sed -i 's/chrome-infra-packages.8pp2p8t.qjz9zk/chrome-infra-packages.appspot.com/g' "$SRC/tools/rust/build_rust.py"
  mark tools
fi
echo "$VER" > "$T/chromium-version"
say "source ready: $SRC"
