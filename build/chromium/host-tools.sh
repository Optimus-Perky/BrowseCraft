# Sourced by linux-build.sh and win-prepare.sh (inside chromium-builder): Chromium's own clang
# and Rust, the Debian sysroot, and the build host's node/tsc/esbuild/gperf/go/clang-format where
# Chromium expects its prebuilt ones (the lite tarball has none). Ported from
# ungoogled-chromium-portablelinux scripts/shared.sh setup_toolchain (156.0.8078.79-1), x64 only.
#
#   setup_host_tools <src>     downloads once (stamp in <src>/.host-tools), then exports CC etc.
setup_host_tools() {
  local src=$1
  if [ ! -f "$src/.host-tools" ]; then
    (cd "$src" && python3 tools/rust/update_rust.py && python3 tools/clang/scripts/update.py)
    python3 "$src/build/linux/sysroot_scripts/install-sysroot.py" --arch=amd64
    mkdir -p "$src/third_party/node/linux/node-linux-x64/bin" "$src/third_party/gperf/cipd/bin" \
             "$src/third_party/dawn/tools/golang/linux-amd64/bin" "$src/buildtools/linux64-format"
    ln -sf "$(which node)" "$src/third_party/node/linux/node-linux-x64/bin/node"
    ln -sf "$(which gperf)" "$src/third_party/gperf/cipd/bin/gperf"
    ln -sf "$(which go)" "$src/third_party/dawn/tools/golang/linux-amd64/bin/go"
    ln -sf "$(which clang-format)" "$src/buildtools/linux64-format/clang-format"
    # (no trailing slash on the rm: with one, a re-run follows the link into the image)
    rm -rf "$src/third_party/devtools-frontend/src/node_modules/esbuild"
    ln -s /usr/lib/node_modules/esbuild/ "$src/third_party/devtools-frontend/src/node_modules/esbuild"
    touch "$src/.host-tools"
  fi
  local bin="$src/third_party/llvm-build/Release+Asserts/bin"
  export CC="$bin/clang" CXX="$bin/clang++" AR="$bin/llvm-ar" NM="$bin/llvm-nm" LLVM_BIN="$bin"
  local rd; rd="$("$CC" --print-resource-dir)"
  export CXXFLAGS="${CXXFLAGS:-} -resource-dir=$rd -B$bin" CPPFLAGS="${CPPFLAGS:-} -resource-dir=$rd -B$bin" \
         CFLAGS="${CFLAGS:-} -resource-dir=$rd -B$bin"
}

# Every patch dry-run first, then applied; stops (exit 2) at the first that does not fit.
#   apply_series_to <src> <series file> <patch dir> <record file> [-R]
apply_series_to() {
  local src=$1 series=$2 dir=$3 rec=$4 rev=${5:-}
  touch "$rec"
  while read -r p; do
    p=${p%%#*}; p=$(echo "$p" | xargs)
    [ -z "$p" ] && continue
    grep -qxF "$p" "$rec" && continue
    if ! patch -p1 $rev --ignore-whitespace --no-backup-if-mismatch --forward --dry-run -s -d "$src" -i "$dir/$p"; then
      echo "PATCH FAILED (dry run): $p" >&2
      echo "$p" > "$(dirname "$rec")/failed-patch"
      exit 2
    fi
    patch -p1 $rev --ignore-whitespace --no-backup-if-mismatch --forward -s -d "$src" -i "$dir/$p"
    echo "$p" >> "$rec"
  done < "$series"
}
