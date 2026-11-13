#!/bin/bash
# The Linux tarball, browsecraft-<chromium>-v<version>-linux-x64.tar.xz, from a built Linux tree.
#   package.sh <linux src> <out dir> <chromium version> <browsecraft version>
# Runs in chromium-builder. File list from ungoogled-chromium-portablelinux package.sh
# (156.0.8078.79-1); no AppImage. install-desktop-entry.sh adds a menu entry for the user.
set -euo pipefail
SRC=$1; OUTDIR=$2; CHROMIUM=$3; VER=$4
O=$SRC/out/Default
N=browsecraft-$CHROMIUM-v$VER-linux-x64
R=$OUTDIR/$N
rm -rf "$R" && mkdir -p "$R"
missing=0
for f in chrome chrome_100_percent.pak chrome_200_percent.pak chrome_crashpad_handler chromedriver chrome-wrapper \
         icudtl.dat libEGL.so libGLESv2.so libqt5_shim.so libqt6_shim.so libvk_swiftshader.so libvulkan.so.1 \
         locales product_logo_48.png resources.pak v8_context_snapshot.bin vk_swiftshader_icd.json; do
  if [ -e "$O/$f" ]; then cp -r "$O/$f" "$R/"; else echo "missing $f" >&2; missing=1; fi
done
# chromedriver and the Qt shims are optional; the browser itself is not.
for f in chrome resources.pak icudtl.dat v8_context_snapshot.bin locales; do
  [ -e "$R/$f" ] || { echo "the package has no $f" >&2; exit 1; }
done
cp "$SRC/chrome/app/theme/chromium/linux/product_logo_256.png" "$R/browsecraft.png"
cat > "$R/install-desktop-entry.sh" <<"EOS"
#!/bin/sh
# Adds BrowseCraft to your desktop menu, run from wherever you unpacked it.
HERE="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$HOME/.local/share/applications"
cat > "$HOME/.local/share/applications/browsecraft.desktop" <<EOD
[Desktop Entry]
Type=Application
Name=BrowseCraft
GenericName=Web Browser
Comment=Your own browser, synced to your own server
Exec="$HERE/chrome" %U
Icon=$HERE/browsecraft.png
Terminal=false
Categories=Network;WebBrowser;
MimeType=text/html;x-scheme-handler/http;x-scheme-handler/https;
StartupWMClass=BrowseCraft
EOD
echo "Added BrowseCraft to the menu (~/.local/share/applications/browsecraft.desktop)."
EOS
chmod +x "$R/install-desktop-entry.sh"
cd "$OUTDIR" && tar cf - "$N" | xz -T4 -6 > "$N.tar.xz" && rm -rf "$N"
sha256sum "$N.tar.xz" > "$N.tar.xz.sha256"
ls -la "$OUTDIR/$N.tar.xz"
