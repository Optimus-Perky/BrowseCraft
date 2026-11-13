#!/bin/bash
# Moves the vendored ungoogled-chromium core (third_party/ungoogled-chromium) to another tag and
# records it in third_party/ungoogled-chromium.PIN. The files are upstream's, unchanged.
#   build/update-ungoogled.sh <tag, e.g. 158.0.8127.37-1>
# Then commit; BrowseCraft's own patches are re-tried against it by the next build.
set -euo pipefail
TAG=${1:?usage: update-ungoogled.sh <ungoogled-chromium tag>}
cd "$(dirname "$0")/.."
TMP=$(mktemp -d); trap 'rm -rf "$TMP"' EXIT
git -c advice.detachedHead=false clone -q --depth 1 --branch "$TAG" https://github.com/ungoogled-software/ungoogled-chromium "$TMP/uc"
COMMIT=$(git -C "$TMP/uc" rev-parse HEAD)
CHROMIUM=$(cat "$TMP/uc/chromium_version.txt")
rm -rf third_party/ungoogled-chromium
mkdir -p third_party/ungoogled-chromium
git -C "$TMP/uc" archive HEAD | tar -x -C third_party/ungoogled-chromium
cat > third_party/ungoogled-chromium.PIN <<EOF
# ungoogled-chromium core, vendored as is (BSD-3-Clause, see ungoogled-chromium/LICENSE).
# Source: https://github.com/ungoogled-software/ungoogled-chromium
tag=$TAG
commit=$COMMIT
chromium=$CHROMIUM
vendored=$(date +%Y-%m-%d)
# Update with build/update-ungoogled.sh <tag> (never edit files under ungoogled-chromium/ by hand:
# BrowseCraft's changes are its own patches in patches/ and build/*.py).
EOF
echo "ungoogled-chromium core now $TAG ($COMMIT, Chromium $CHROMIUM)"
