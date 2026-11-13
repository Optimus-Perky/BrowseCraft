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

From Chromium 158, BrowseCraft's own version is `v<major>.<minor>.<yymmdd>[letter]` (13 Nov
2026): the major goes up one with each Chromium major (158 is v1, 159 v2), the minor is 0 for a
major's first build and goes up one with each Chromium point release in it, yymmdd is the day that
version was first built, and a build of ours on a Chromium already built (a fix, no new Chromium)
keeps the number and adds a letter: v1.0.261120, v1.0.261120a, v1.0.261120b, then v1.1.261201 for
the next point release. Up to 157 it was `v<release>.<yymmdd>.<build of the day>`, e.g.
v0.8.261013.1 (release in `VERSION`). `build/builds.log` records each version with its Chromium,
which is how the next one is worked out; `build/tests/test_version.py` tests the rules.
Before each build: `build/version.py next <chromium>` gives the version, then
`build/version.py stamp <src> <version>` puts it in the tree (Settings > About shows
"Version v0.8.261013.1 (Chromium 155.0.8059.39)"). Name the Windows zip
`browsecraft-<chromium>-v<version>-windows-x64.zip`; `packaging/windows/build-installer.sh`
reads the version from it, makes `BrowseCraft-v<version>.exe` and records it in `build/builds.log`.

## Update check, update tab, What's new, change history (from v0.8.261113.2)

`build/updatecheck.py` (after identity.py; new files in `build/updatecheck/`):

- **Check**: about 30 s after start-up, then every 24 h, a plain GET of
  `https://altcraft.perkin.me.uk/browsecraft/latest.json` (no cookies, no identifiers; 64 KB at
  most). A file that does not check out (`browsecraft_update_logic.cc`: schema, product, version,
  https or same-origin URLs, value ranges, printable text) is ignored and never shown.
- **Shown**: a newer, not skipped version is an item in the app menu and a line on Settings >
  About. A *security* update also opens chrome://browsecraft/update: at the check that follows a
  browser start, at most once a day, never mid-session. "Skip this version" (Local State
  `browsecraft.update.skipped_version`) silences that version; a newer one shows again. Nothing is
  ever downloaded or installed.
- **Off**: chrome://browsecraft "Check for a new BrowseCraft once a day"; policy
  `UpdateCheckEnabled` = 0; `--disable-browsecraft-update-check`. Elsewhere: policy
  `UpdateCheckUrl`, `--browsecraft-update-url`. Tests: `--browsecraft-update-check-delay`,
  `--browsecraft-update-check-interval`, `--browsecraft-update-today`.
- **What's new**: at the first start after the version changes, chrome://browsecraft/whatsnew
  shows the release notes since the last version run (with none recorded: the releases of the
  running Chromium major). Never on a new profile or a downgrade. Off on chrome://browsecraft.
- **Room for automatic installs**: the checker only finds versions and tells observers
  (`browsecraft::update::Observer`); latest.json carries each platform's installer URL and sha256.
- Unit tests: `ninja -C out/Default browsecraft_update_unittests` (Linux tree), plain gtest.

**Release notes** (`release-notes/<version>.json`, `build/release_notes.py`): what is new and
fixed, written for users, plus the Chromium security fixes from Google's Chrome Releases notes
(`build/chrome_releases.py`, filled once with `release_notes.py security <version>`). One
renderer for both places: `version.py stamp` bundles the entries into the build
(`browsecraft_release_notes.h`) and the site's change history is built from the same entries when
a release is published. Tests: `python3 build/tests/test_release_notes.py`.
