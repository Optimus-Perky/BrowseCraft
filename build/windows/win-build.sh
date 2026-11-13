#!/bin/bash
# BrowseCraft for Windows, cross-compiled on Linux (inside chromium-builder-win, with /dev/fuse).
# FLAGS: ungoogled core flags.gn (third_party/ungoogled-chromium/flags.gn; /flags/flags.gn before).
set -euo pipefail
cd /src
# Mounts the Windows SDK case-insensitively (ciopfs). On Linux the first update in a tree must be
# forced (it unpacks the packaged toolchain, ours is GYP_MSVS_HASH_*); later ones reuse the json.
python3 build/vs_toolchain.py update $([ -f build/win_toolchain.json ] || echo --force)
mkdir -p out/Win
{ cat "${FLAGS:-/flags/flags.gn}"; cat <<GN
target_os="win"
target_cpu="x64"
is_official_build=false
chrome_pgo_phase=0
use_thin_lto=false
symbol_level=0
blink_symbol_level=0
v8_symbol_level=0
ffmpeg_branding="Chrome"
proprietary_codecs=true
enable_swiftshader=false
is_clang=true
is_component_build=false
is_debug=false
dcheck_always_on=false
enable_rust=true
enable_mse_mpeg2ts_stream_parser=true
GN
  # No compiler cache: Chromium 155 builds with Clang modules (-fmodules-cache-path), which
  # sccache will not cache (tried 2026-10-21: 2,185 compiles, 0 cacheable). Turning modules off
  # to allow it would slow every uncached compile.
} > out/Win/args.gn

# Toolchain guard: a changed clang means every object is stale and the cache misses too,
# i.e. hours. That happened once by accident (README step 5); refuse unless asked for.
CLANG=third_party/llvm-build/Release+Asserts/bin/clang-cl
now=$(sha256sum "$CLANG" | cut -c1-16)
if [ -f out/Win/.clang-stamp ] && [ "$(cat out/Win/.clang-stamp)" != "$now" ] && [ "${FORCE_FULL:-0}" != 1 ]; then
  echo "clang changed since the last build ($(cat out/Win/.clang-stamp) -> $now): everything would rebuild. Set FORCE_FULL=1 to go ahead." >&2
  exit 3
fi
echo "$now" > out/Win/.clang-stamp
[ -x out/Win/gn ] || cp /linux-gn out/Win/gn   # the Linux gn (the Windows patches make bootstrap build gn.exe)
out/Win/gn gen out/Win --fail-on-unused-args
ninja -j "${NINJA_JOBS:-18}" ${NINJA_EXTRA:-} -C out/Win ${TARGETS:-chrome chromedriver mini_installer}
