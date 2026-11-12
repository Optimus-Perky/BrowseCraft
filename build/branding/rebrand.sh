#!/bin/sh
# Puts BrowseCraft's name and icons on a Chromium source tree: rebrand.sh /path/to/src
# (run make-branding.sh first). Safe to run again.
set -e
SRC=${1:?usage: rebrand.sh /path/to/chromium/src}
HERE=$(cd "$(dirname "$0")" && pwd)
cp -r "$HERE/out/." "$SRC/"
cat > "$SRC/chrome/app/theme/chromium/BRANDING" <<'B'
COMPANY_FULLNAME=The BrowseCraft Authors
COMPANY_SHORTNAME=BrowseCraft
PRODUCT_FULLNAME=BrowseCraft
PRODUCT_SHORTNAME=BrowseCraft
PRODUCT_INSTALLER_FULLNAME=BrowseCraft Installer
PRODUCT_INSTALLER_SHORTNAME=BrowseCraft Installer
COPYRIGHT=Copyright @LASTCHANGE_YEAR@ The BrowseCraft Authors and The Chromium Authors. All rights reserved.
MAC_BUNDLE_ID=uk.co.browsecraft.BrowseCraft
MAC_CREATOR_CODE=Bc24
MAC_TEAM_ID=
B
# English UI text: "Chromium" -> "BrowseCraft" (not in file paths or lower-case URLs).
# Other languages fall back to English for the changed strings.
for f in chrome/app/chromium_strings.grd components/components_chromium_strings.grd $(cd "$SRC" && ls chrome/app/*chromium*.grdp 2>/dev/null); do
  [ -f "$SRC/$f" ] || continue
  sed -i -E '/(path=|file=|<output|<include|\.xtb|\.pak|\.h")/!s/Chromium/BrowseCraft/g' "$SRC/$f"
done
# Settings' "You and Google" section is about the BrowseCraft account here.
sed -i 's/^      You and Google$/      You and BrowseCraft/' "$SRC/chrome/app/settings_strings.grdp"
echo "rebranded $SRC"
