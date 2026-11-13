#!/bin/bash
# BrowseCraft for Linux from a prepared tree (prepare-source.sh), inside chromium-builder.
#   linux-build.sh <repo> <linux tree>       (the tree's src is a copy of the prepared source)
# Env: NINJA_JOBS (the caller sizes it to free memory), TARGETS (default: chrome chromedriver).
# Ported from ungoogled-chromium-portablelinux scripts/shared.sh write_gn_args, gn_gen and
# maybe_build (156.0.8078.79-1).
set -euo pipefail
REPO=$(cd "$1" && pwd); SRC=$(cd "$2" && pwd)/src
. "$REPO/build/chromium/host-tools.sh"
setup_host_tools "$SRC"
cd "$SRC"
mkdir -p out/Default
cat "$REPO/third_party/ungoogled-chromium/flags.gn" "$REPO/build/linux/flags.gn" > out/Default/args.gn
[ -x out/Default/gn ] || ./tools/gn/bootstrap/bootstrap.py -o out/Default/gn --skip-generate-buildfiles
./out/Default/gn gen out/Default --fail-on-unused-args
ninja -j "${NINJA_JOBS:-10}" ${NINJA_EXTRA:-} -C out/Default ${TARGETS:-chrome chromedriver}
