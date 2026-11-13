# Building BrowseCraft

From Chromium 157 (13 Nov 2026, "the middle way"): **Chromium's own release tarball**, plus
**ungoogled-chromium's core** patches, lists and flags, vendored unchanged and pinned in
`third_party/ungoogled-chromium` (`third_party/ungoogled-chromium.PIN`; move it with
`build/update-ungoogled.sh <tag>`), plus BrowseCraft's patches. BrowseCraft no longer depends on
ungoogled's per-platform projects; what it needed from them lives here, with where it came from:

- `build/chromium/prepare-source.sh` (from portablelinux `scripts/shared.sh`): unpack the lite
  tarball (sha256 from Google's `.hashes`), prune, patches (each dry-run first: the core series,
  `patches/portablelinux/series`, then `patches/series`), domain substitution, the hooks below,
  BrowseCraft's name and icons (`build/branding`, resvg needed), the toolchain addresses.
- `build/chromium/host-tools.sh` (portablelinux `setup_toolchain`): Chromium's clang and Rust,
  the sysroot, the build host's node/tsc/esbuild/gperf/go.
- `build/chromium/linux-build.sh` + `build/linux/flags.gn`: the Linux build (a developer build:
  `is_official_build=false`, no PGO/ThinLTO/mold, DCHECKs off).
- `build/linux/package.sh`: `browsecraft-<chromium>-v<version>-linux-x64.tar.xz`.
- `build/chromium/win-prepare.sh` + `patches/ucw` (from ungoogled-chromium-windows) +
  `build/windows/*`: the Windows tree, cross-built on Linux (`build/windows/README.md`).
- `patches/portablelinux`: build-host patches (both trees); `SOURCE` lists what was fitted or
  dropped for 157.

Run, in the `chromium-builder` image (uid 1000), memory-capped and on one socket's CPUs:

    prepare-source.sh <repo> <chromium version> <tarball dir> <tree>
    win-prepare.sh <repo> <tree> <win tree> <download cache>        then build/windows/win-build.sh
    cp -a --reflink=always <tree>/src <linux tree>/src; linux-build.sh <repo> <linux tree>
    version.py stamp <src> <version>                                 in each tree, before building

`autobuild/` does all of this unattended (see its README).

## BrowseCraft patches (in order, after ungoogled-chromium's; `patches/series`)

1. `sync-to-own-server.patch`: chrome://browsecraft sign-in; with local sync on and a BrowseCraft token, sync goes to the user's server (from 157: `EngineComponentsFactoryImpl::CreateForBrowseCraftServer`). (`fix-non-official-link.patch`, a link fix for a tests-only call, is gone: 157 no longer has the call.)
2. `restore-sync-service.patch`: ungoogled-chromium makes SyncServiceFactory return nothing; this puts Chromium's code back (and lets a signed-in BrowseCraft profile skip the roaming folder).
3. `sync-without-security-events.patch`: ungoogled-chromium removes the security event recorder; the sync controllers are built without SECURITY_EVENTS.

The patch files are the source of truth. The generators that first made them (`mkpatch.py`, `restoresync.py`, `skipsec.py`, `fixlink.py`) read 155-era trees at fixed paths and are kept for reference only.

Note: Chromium gzips sync requests (`Content-Encoding: gzip`, components/sync/engine/net/http_bridge.cc); the server unpacks them.

## Extensions from the Chrome Web Store

`build/webstore.py` puts back what ungoogled-chromium removes so "Add to Chrome" and extension
updates work: the store's addresses, its install API, update checks and the store's own process.
It must run **after** domain substitution (the addresses are written out in full);
`build/chromium/prepare-source.sh` runs it right after domain substitution.
Google is only contacted to install or update an extension; no Google account is involved.

`extra/ungoogled-chromium/first-run-page.patch` (core) is left out (2026-10-12): no
chrome://ungoogled-first-run tab on first start. The core stays vendored unchanged; `patches/core-skip`
lists what prepare-source.sh leaves out. In an existing tree, reverse it with `patch -R -p1`.

`build/extstore.py` (after webstore.py, from prepare-source.sh): the organisation extension store, from
policy ExtensionStoreUrl. See its docstring.

`build/profile.py` (after extstore.py, from prepare-source.sh; new files in `build/profile/`): signing in
with the Windows account (Kerberos, via SSPI, to the server's `/api/sign-in/negotiate`) at start-up
when policy `SyncServerUrl` (else `ExtensionStoreUrl`) names a server; "Sign in" in the profile
menu; sync that starts and stops with sign-in (no restart: BrowseCraft always runs Chromium's local
sync mode, gated on the sign-in token); the profile name from the directory's full name; and
"Upload a photo" in Settings > Customise profile. See its docstring. Linux builds have no
Kerberos (ungoogled-chromium sets `use_kerberos=false`), so single sign-on is Windows only.
Test: `test/profile-e2e.sh` (Linux build, password sign-in).

`build/identity.py` (after profile.py, from prepare-source.sh): puts `chromiumapp.org` back in
chrome.identity's return address (domain substitution made it `ch40m1umapp.qjz9zk`), so
extensions that sign in with `launchWebAuthFlow` (the Claude extension, among others) keep their
login. The address is only matched by the browser, never contacted.

## Version

BrowseCraft's own version is `v<release>.<yymmdd>.<build of the day>`, e.g. v0.8.261013.1
(release in `VERSION`). Before each build: `build/version.py next` gives the version, then
`build/version.py stamp <src> <version>` puts it in the tree (Settings > About shows
"Version v0.8.261013.1 (Chromium 155.0.8059.39)"). Name the Windows zip
`browsecraft-<chromium>-v<version>-windows-x64.zip`; `packaging/windows/build-installer.sh`
reads the version from it, makes `BrowseCraft-v<version>.exe` and records it in `build/builds.log`.
