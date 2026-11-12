# Building BrowseCraft (Linux, in Docker)

Based on [ungoogled-chromium-portablelinux](https://github.com/ungoogled-software/ungoogled-chromium-portablelinux).
Changes to its checkout (portablelinux/):

- `flags.linux.gn`: a developer build while iterating — `is_official_build=false`,
  `chrome_pgo_phase=0`, `use_thin_lto=false`, `use_mold=false` (mold is not in the source
  tarball). Release builds go back to `is_official_build=true`.
- `scripts/shared.sh`: `ninja -j ${NINJA_JOBS:-10}` (limit jobs to the RAM there is), and
  `rm -rf .../node_modules/esbuild` without a trailing slash (with one, a re-run follows the
  symlink into the image and fails).
- `scripts/build.sh`: no `rm -rf out` at the start, so a re-run continues the build.

`mkpatch.py` writes `patches/browsecraft/sync-to-own-server.patch` from the patched source tree
plus the new files in `patches/new/`.

Run (the build host): `docker run -d --name browsecraft-chromium-build --cpus=32 --memory=26g -u 1000:1000
-e NINJA_JOBS=22 -v $PWD:/repo chromium-builder:trixie-slim bash /repo/scripts/build.sh`

## BrowseCraft patches (in order, after ungoogled-chromium's; `patches/series`)

1. `fix-non-official-link.patch`: a tests-only call left behind by ungoogled-chromium stops non-LTO builds linking.
2. `sync-to-own-server.patch`: chrome://browsecraft sign-in; with local sync on and a BrowseCraft token, sync goes to the user's server.
3. `restore-sync-service.patch`: ungoogled-chromium makes SyncServiceFactory return nothing; this puts Chromium's code back (and lets a signed-in BrowseCraft profile skip the roaming folder).
4. `sync-without-security-events.patch`: ungoogled-chromium removes the security event recorder; the sync controllers are built without SECURITY_EVENTS.

Generators: `mkpatch.py` (2), `restoresync.py` (3), `skipsec.py` (4), `fixlink.py` (1). They read the source tree as it is before the patch they make.

Note: Chromium gzips sync requests (`Content-Encoding: gzip`, components/sync/engine/net/http_bridge.cc); the server unpacks them.

## Extensions from the Chrome Web Store

`build/webstore.py` puts back what ungoogled-chromium removes so "Add to Chrome" and extension
updates work: the store's addresses, its install API, update checks and the store's own process.
It must run **after** domain substitution (the addresses are written out in full); portablelinux's
`scripts/shared.sh` runs it from `patches/browsecraft/webstore.py` right after `apply_domsub`.
Google is only contacted to install or update an extension; no Google account is involved.

`ungoogled-chromium/patches/extra/ungoogled-chromium/first-run-page.patch` is dropped from the
series (2026-10-12): no chrome://ungoogled-first-run tab on first start. In an existing tree,
reverse it with `patch -R -p1` (a copy is in $B/patchwork/first-run-page.patch.removed-20261012).

`build/extstore.py` (after webstore.py, from shared.sh): the organisation extension store, from
policy ExtensionStoreUrl. See its docstring.

`build/profile.py` (after extstore.py, from shared.sh; new files in `build/profile/`): signing in
with the Windows account (Kerberos, via SSPI, to the server's `/api/sign-in/negotiate`) at start-up
when policy `SyncServerUrl` (else `ExtensionStoreUrl`) names a server; "Sign in" in the profile
menu; sync that starts and stops with sign-in (no restart: BrowseCraft always runs Chromium's local
sync mode, gated on the sign-in token); the profile name from the directory's full name; and
"Upload a photo" in Settings > Customise profile. See its docstring. Linux builds have no
Kerberos (ungoogled-chromium sets `use_kerberos=false`), so single sign-on is Windows only.
Test: `test/profile-e2e.sh` (Linux build, password sign-in).

`build/identity.py` (after profile.py, from shared.sh): puts `chromiumapp.org` back in
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
