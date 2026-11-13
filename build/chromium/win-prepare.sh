#!/bin/bash
# The Windows tree, from the prepared source (prepare-source.sh), inside chromium-builder.
#   win-prepare.sh <repo> <prepared tree> <win tree> <download cache>
# Then build/windows/win-build.sh builds it. Each stage leaves a stamp in <win tree>; a patch
# that does not fit stops the run (exit 2). The recipe is build/windows/README.md:
#   1. a reflink copy of the prepared src (btrfs: no extra space until files change)
#   2. host tools (host-tools.sh) with Windows' clang runtime too (.gclient target_os win,
#      BEFORE any build: re-unpacking clang later makes ninja rebuild everything)
#   3. gn bootstrapped for Linux before ucw's patches (they make bootstrap build gn.exe)
#   4. ucw's Windows patches (patches/ucw, from ungoogled-chromium-windows), then undo two of
#      their changes: undo-external-rust (Chromium's own Rust builds std for windows-msvc),
#      undo-rc-exe (rc via Chromium's rc.py, not Microsoft's rc.exe)
#   5. Windows-only sources the lite tarball prunes (build/windows/win-sources.ini)
#   6. rc and ciopfs by their .sha1 from Chromium's bucket
#   7. third_party/win_build_output (MIDL/MC outputs) from Chromium's gitiles at the version
#   8. f16-builtins.patch, snapshot_blob.bin in FILES.cfg (files-snapshot.patch)
set -euo pipefail
REPO=$(cd "$1" && pwd); PREP=$2; W=$3; DL=$4
WIN=$REPO/build/windows
SRC=$W/src
VER=$(cat "$PREP/chromium-version")
. "$REPO/build/chromium/host-tools.sh"
say() { echo "[$(date +%H:%M:%S)] $*"; }
done_() { [ -f "$W/.stage-$1" ]; }
mark() { touch "$W/.stage-$1"; }
mkdir -p "$W" "$DL"

if ! done_ copy; then
  rm -rf "$SRC"
  cp -a --reflink=always "$PREP/src" "$SRC"
  rm -rf "$SRC/out" "$SRC/.host-tools"
  mark copy
fi

if ! done_ tools; then
  echo 'target_os = ["win"]' > "$W/.gclient"
  setup_host_tools "$SRC"
  mark tools
else
  setup_host_tools "$SRC"
fi

if ! done_ gn; then
  mkdir -p "$SRC/out/Win" "$SRC/buildtools/linux64"
  (cd "$SRC" && ./tools/gn/bootstrap/bootstrap.py -o out/Win/gn --skip-generate-buildfiles)
  cp "$SRC/out/Win/gn" "$SRC/buildtools/linux64/gn"   # licenses.py looks there
  mark gn
fi

if ! done_ ucw; then
  apply_series_to "$SRC" "$REPO/patches/ucw/series" "$REPO/patches/ucw" "$W/.applied-ucw"
  say "ucw: $(wc -l < "$W/.applied-ucw") patches applied"
  mark ucw
fi

if ! done_ undo; then
  printf '%s\n' undo-external-rust.patch undo-rc-exe.patch > "$W/undo.series"
  apply_series_to "$SRC" "$W/undo.series" "$WIN" "$W/.applied-undo" -R
  mark undo
fi

if ! done_ winsrc; then
  python3 -I - "$WIN/win-sources.ini" "$DL" "$SRC" <<'PY'
import configparser, hashlib, os, re, shutil, sys, urllib.request, zipfile
ini, dl, src = sys.argv[1:]
deps = open(os.path.join(src, "DEPS")).read()
cp = configparser.RawConfigParser()
cp.read(ini)
for name in cp.sections():
    s = dict(cp[name])
    if "deps" in s:   # the commit Chromium's DEPS pins for this path
        m = re.search(r"'%s':\s*\{\s*'url':[^\n]*?'@'\s*\+\s*'([0-9a-f]{40})'" % re.escape(s["deps"]), deps)
        if not m:
            sys.exit(f"{name}: {s['deps']} not found in DEPS")
        s["version"] = m.group(1)
    for k in ("url", "download_filename", "strip_leading_dirs"):
        if k in s:
            s[k] = s[k].replace("%(version)s", s["version"])
    path = os.path.join(dl, s["download_filename"])
    if not os.path.exists(path):
        urllib.request.urlretrieve(s["url"], path + ".part")
        os.rename(path + ".part", path)
    if "sha512" in s:
        got = hashlib.sha512(open(path, "rb").read()).hexdigest()
        assert got == s["sha512"], f"{name}: sha512 mismatch"
    out = os.path.join(src, s["output_path"])
    strip = s.get("strip_leading_dirs")
    if strip and os.path.isdir(out):   # a folder of its own: start clean (a stale version must go)
        shutil.rmtree(out)
    with zipfile.ZipFile(path) as z:
        for m in z.infolist():
            rel = m.filename
            if strip:
                if not rel.startswith(strip + "/"):
                    continue
                rel = rel[len(strip) + 1:]
            if not rel:
                continue
            dest = os.path.join(out, rel)
            if m.is_dir():
                os.makedirs(dest, exist_ok=True)
                continue
            os.makedirs(os.path.dirname(dest), exist_ok=True)
            with z.open(m) as f, open(dest, "wb") as o:
                shutil.copyfileobj(f, o)
    print(name, s["version"], "->", s["output_path"])
PY
  mark winsrc
fi

if ! done_ bucket; then
  for t in build/toolchain/win/rc/linux64/rc:rc build/ciopfs:ciopfs; do
    f=${t%%:*}; b=${t##*:}; h=$(cut -c1-40 "$SRC/$f.sha1")
    curl -fsS -o "$SRC/$f" "https://storage.googleapis.com/chromium-browser-clang/$b/$h"
    [ "$(sha1sum "$SRC/$f" | cut -c1-40)" = "$h" ] || { echo "$f: sha1 mismatch" >&2; exit 1; }
    chmod +x "$SRC/$f"
  done
  mark bucket
fi

if ! done_ winbuildoutput; then
  (cd "$SRC" && python3 "$WIN/fetch-win-build-output.py" "$REPO/third_party/ungoogled-chromium/pruning.list" "$VER" > "$W/win-build-output.log")
  say "win_build_output: $(wc -l < "$W/win-build-output.log") files"
  mark winbuildoutput
fi

if ! done_ final; then
  printf '%s\n' f16-builtins.patch files-snapshot.patch > "$W/final.series"
  apply_series_to "$SRC" "$W/final.series" "$WIN" "$W/.applied-final"
  mark final
fi
echo "$VER" > "$W/chromium-version"
say "Windows tree ready: $SRC"
