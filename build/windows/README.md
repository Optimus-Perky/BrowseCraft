# AltBrowse for Windows: cross-compiled on Linux

> Renamed **AltBrowse** on 20 Nov 2026 (was BrowseCraft). Older entries below keep the old name; paths, ids and settings that kept it are listed in the top README ("The rename").

1. **Toolchain** (once, on a Windows VM): Visual Studio 2026 Build Tools (C++ workload, ATL/MFC,
   ARM64 VC + MFC ARM64, which Chromium's packager insists on) and Windows SDK 10.0.28000 with
   the debuggers; then `python package_from_installed.py 2026 -w 10.0.28000.0` (with `vswhere
   -prerelease -products *` so Build Tools are found). Gives `<hash>.zip` (ours: 3cc1a8b0ee).
2. **Source**: a reflink copy of the patched Linux tree (`src` minus `out`), plus
   ungoogled-chromium-windows's patches from `series-used` (5 left out: their job is done by
   portablelinux's own patches, or they suit the downloaded Windows LLVM we don't use), then
   `patch -R undo-external-rust.patch` (use Chromium's own Rust, which builds std for Windows).
3. **Windows-only sources** pruned from the lite tarball (`win-sources.ini`): DirectX-Headers and
   webauthn (strip the top folder), windows-rs 0.52.0 (into rust vendor/ as is).
4. **Tools** from Chromium's bucket by their .sha1: `build/toolchain/win/rc/linux64/rc`,
   `build/ciopfs` (FUSE, case-insensitive SDK). Image: `Dockerfile` (adds fuse).
5. **More undoing**: `patch -R undo-rc-exe.patch` (rc via Chromium's rc.py, not Microsoft's
   rc.exe); `fetch-win-build-output.py` (MIDL/MC outputs pruned by ungoogled); clang's Windows
   runtime (clang_rt.builtins etc.): put `target_os = ["win"]` in a `.gclient` beside `src` and run
   `tools/clang/scripts/update.py` **before the first build** (it re-unpacks clang, so ninja then
   rebuilds everything).
6. **Build**: `win-build.sh` in the image with `--device /dev/fuse --cap-add SYS_ADMIN`,
   `GYP_MSVS_HASH_3bfcb536c8=3cc1a8b0ee`, `DEPOT_TOOLS_WIN_TOOLCHAIN_BASE_URL=/tc/`; it mounts the
   SDK, writes `out/Win/args.gn`, uses the Linux `gn` (the Windows patches make gn's bootstrap
   build gn.exe), and builds chrome, chromedriver and mini_installer.
7. **Linking**: `patch -p1 f16-builtins.patch` (Rust core calls the half-float conversions, and
   clang_rt.builtins isn't linked on Windows); copy the Linux `gn` to `buildtools/linux64/gn` too
   (licenses.py looks there).
8. **Package**: mini_installer needs Windows archiving tools, so ship a portable zip instead:
   `patch -p1 files-snapshot.patch`, then ungoogled's `utils/filescfg.py -c
   chrome/tools/build/win/FILES.cfg --build-outputs out/Win --cpu-arch 64bit archive -o <zip>`.
   Chromium doesn't build `v8_context_snapshot.bin` for Windows cross-builds, so V8 falls back
   to `snapshot_blob.bin`, which FILES.cfg doesn't list. Without it every renderer dies with
   "Error loading V8 startup snapshot file" and chrome:// pages show ERR_INVALID_URL.
9. **Testing on Windows over SSH**: OpenSSH kills the session's child processes, so start chrome
   from a scheduled task (`/RU SYSTEM`) and drive it over DevTools. chrome:// URLs given on the
   command line are ignored; navigate via CDP. In headless mode, chrome://browsecraft's Restart
   only closes the window; stop and start the browser yourself.
10. **Run it from a local disk.** From a network drive, the sandbox can't start the GPU process
   (`GPU process launch failed: error_code=18`) and the browser exits with "GPU process isn't
   usable. Goodbye." The installer will put it in `%LOCALAPPDATA%\Programs\BrowseCraft`.
