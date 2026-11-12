#!/bin/sh
# Runs profile-e2e.mjs in the builder image (as root, for the policy folder): docker run --rm -u 0
#   --cap-add SYS_NICE -v <portablelinux>:/repo -v <password file>:/pw:ro chromium-builder:trixie-slim
#   sh /repo/profile-e2e.sh <server> <user>
mkdir -p /etc/browsecraft/policies/managed
echo "{\"SyncServerUrl\": \"$1\"}" > /etc/browsecraft/policies/managed/sync.json
Xvfb :97 -screen 0 1280x900x24 >/dev/null 2>&1 &
export DISPLAY=:97
sleep 2
CHROME_LOG=/repo/profile-e2e-chrome.log /repo/build/src/third_party/node/linux/node-linux-x64/bin/node \
  /repo/profile-e2e.mjs /repo/build/src/out/Default/chrome "$1" "$2" /pw > /repo/profile-e2e.log 2>&1
