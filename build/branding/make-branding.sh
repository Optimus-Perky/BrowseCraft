#!/bin/sh
# Makes AltBrowse's icons in Chromium's layout (out/<path in the Chromium tree>), from
# browsecraft-logo.svg (48 px and up) and browsecraft-logo-small.svg (16-32 px).
# Needs resvg (RESVG=path) and ImageMagick's convert (CONVERT="docker run ... convert" works).
set -e
cd "$(dirname "$0")"
RESVG=${RESVG:-resvg}
CONVERT=${CONVERT:-convert}
T=chrome/app/theme
rm -rf out work && mkdir -p work
r() { $RESVG -w $2 -h $2 $1 work/$3; }   # r svg size name
for s in 16 22 24 32; do r browsecraft-logo-small.svg $s s$s.png; done
for s in 48 64 128 176 256 512 600; do r browsecraft-logo.svg $s f$s.png; done
put() { mkdir -p "out/$(dirname "$2")"; cp "work/$1" "out/$2"; }
for d in "$T/chromium" "$T/chromium/linux"; do
  put s24.png $d/product_logo_24.png
  for s in 48 64 128 256; do put f$s.png $d/product_logo_$s.png; done
done
put s16.png $T/chromium/product_logo_16.png
put s16.png $T/default_100_percent/chromium/product_logo_16.png
put s32.png $T/default_100_percent/chromium/product_logo_32.png
put s16.png $T/default_100_percent/chromium/linux/product_logo_16.png
put s32.png $T/default_100_percent/chromium/linux/product_logo_32.png
put s32.png $T/default_200_percent/chromium/product_logo_16.png
put f64.png $T/default_200_percent/chromium/product_logo_32.png
put f600.png $T/chromium/win/tiles/Logo.png
put f176.png $T/chromium/win/tiles/SmallLogo.png
cp browsecraft-logo.svg out/$T/chromium/product_logo.svg
cp browsecraft-logo.svg out/$T/chromium/product_logo_animation.svg
$CONVERT work/s22.png -colorspace Gray work/mono22.png && put mono22.png $T/chromium/product_logo_22_mono.png
$CONVERT work/s32.png out/$T/chromium/linux/product_logo_32.xpm
for ico in chromium app_list chromium_doc chromium_pdf; do
  $CONVERT work/s16.png work/s24.png work/s32.png work/f48.png work/f64.png work/f128.png work/f256.png out/$T/chromium/win/$ico.ico
done
# The logo with the name beside it (About page), at 1x and 2x, dark and white text.
for scale in 1 2; do
  h=$((22 * scale)); dir=$T/default_$((scale * 100))_percent/chromium
  mkdir -p out/$dir
  for variant in dark white; do
    colour=$([ $variant = white ] && echo white || echo "#202124")
    name=$([ $variant = white ] && echo product_logo_name_22_white.png || echo product_logo_name_22.png)
    $CONVERT -background none -fill "$colour" -font /usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf -pointsize $((15 * scale)) label:AltBrowse PNG32:work/text.png
    $CONVERT work/$([ $scale = 1 ] && echo s22.png || echo f48.png) -resize ${h}x${h} work/logo.png
    $CONVERT work/text.png -background none -gravity west -splice $((4 * scale))x0 +repage PNG32:work/text2.png
    $CONVERT work/logo.png work/text2.png -background none -gravity center +append +repage PNG32:out/$dir/$name
  done
done
find out -type f | sort
