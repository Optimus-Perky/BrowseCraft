#!/bin/sh
# Puts AltBrowse's name and icons on a Chromium source tree: rebrand.sh /path/to/src
# (run make-branding.sh first). Safe to run again, on a pristine tree or on one branded with the
# old name, BrowseCraft (renamed AltBrowse 20 Nov 2026: "Craft" is ArtCraft's product family).
# Only what people read changes: the profile folder, registry keys, policies and prefs keep the
# old name on purpose (build/productdir.py, patches/browsecraft/own-profile-folder.patch), so an
# installed BrowseCraft carries straight over.
set -e
SRC=${1:?usage: rebrand.sh /path/to/chromium/src}
HERE=$(cd "$(dirname "$0")" && pwd)
cp -r "$HERE/out/." "$SRC/"
cat > "$SRC/chrome/app/theme/chromium/BRANDING" <<'B'
COMPANY_FULLNAME=The AltBrowse Authors
COMPANY_SHORTNAME=AltBrowse
PRODUCT_FULLNAME=AltBrowse
PRODUCT_SHORTNAME=AltBrowse
PRODUCT_INSTALLER_FULLNAME=AltBrowse Installer
PRODUCT_INSTALLER_SHORTNAME=AltBrowse Installer
COPYRIGHT=Copyright @LASTCHANGE_YEAR@ The AltBrowse Authors and The Chromium Authors. All rights reserved.
MAC_BUNDLE_ID=uk.co.browsecraft.BrowseCraft
MAC_CREATOR_CODE=Bc24
MAC_TEAM_ID=
B
# English UI text: "Chromium" (or the old name) -> "AltBrowse" (not in file paths or lower-case
# URLs), and "a AltBrowse" -> "an AltBrowse". Other languages fall back to English for the
# changed strings.
for f in chrome/app/chromium_strings.grd components/components_chromium_strings.grd $(cd "$SRC" && ls chrome/app/*chromium*.grdp 2>/dev/null); do
  [ -f "$SRC/$f" ] || continue
  sed -i -E -e '/(path=|file=|<output|<include|\.xtb|\.pak|\.h")/!s/Chromium|BrowseCraft/AltBrowse/g' \
            -e 's/\b([aA]) AltBrowse/\1n AltBrowse/g' "$SRC/$f"
done
# Put Chromium back where it means the project, not this browser (2026-11-13): the credit on
# Settings > About and chrome://version, the copyright (AltBrowse's authors and Chromium's), the
# debug pages' note, and ChromiumOS (ChromeOS builds only). Done after the rename, so a re-run
# lands in the same place.
for f in chrome/app/chromium_strings.grd components/components_chromium_strings.grd $(cd "$SRC" && ls chrome/app/*chromium*.grdp 2>/dev/null); do
  [ -f "$SRC/$f" ] || continue
  # (the copyright: whatever the earlier steps made of it, fresh "The AltBrowse Authors." or a
  # re-run's "...Authors and The AltBrowse Authors.", ends as one "and The Chromium Authors";
  # 20 Nov 2026: the old fix-up doubled it on a re-run.)
  sed -i -E -e 's#</ph>AltBrowse<ph name="END_LINK_CHROMIUM">#</ph>Chromium<ph name="END_LINK_CHROMIUM">#g' \
         -e 's#The AltBrowse Authors( and The (AltBrowse|Chromium) Authors)*\. All rights reserved\.#The AltBrowse Authors and The Chromium Authors. All rights reserved.#g' \
         -e 's#for use by AltBrowse developers#for use by Chromium developers#g' \
         -e 's#AltBrowseOS#ChromiumOS#g' "$SRC/$f"
done
# Settings' "You and Google" section is about the AltBrowse sync account here.
sed -i -E 's/^      You and (Google|BrowseCraft)$/      You and AltBrowse/' "$SRC/chrome/app/settings_strings.grdp"
echo "rebranded $SRC"
