# AltBrowse roadmap

> Renamed **AltBrowse** on 20 Nov 2026 (was BrowseCraft). Older entries below keep the old name; paths, ids and settings that kept it are listed in the top README ("The rename").

## Sync server

- **Recheck domain accounts (for larger / corporate use).** Domain sign-in binds as the user, so
  the server has no way to look at the domain later: an account disabled or removed in AD stays
  signed in on its browsers until an admin presses "Sign out all devices". Add an optional
  read-only service account (`BROWSECRAFT_LDAP_BIND_DN` / password file) and a periodic check
  (say hourly) that signs out the devices of accounts that are disabled, locked, expired, deleted
  or no longer in `BROWSECRAFT_LDAP_USER_GROUP`, and refreshes admin from the admin group.
- **Our own extension store, hosted on the sync server; BrowseCraft never talks to Google.**
  An admin picks extensions on the sync server's admin pages; the server fetches the CRX and
  details from the Chrome Web Store itself and keeps a copy (checking for new versions on a
  schedule). BrowseCraft's store and update addresses point at the sync server instead of
  Google's: the store page lists what the admin has approved, "Add to BrowseCraft" installs from
  it, and installed extensions update from it (the server answers the same Omaha update protocol
  as clients2.google.com, so Chromium's updater needs no changes). The genuine Chrome Web Store is
  then blocked in the browser (its install API only works on our store page, and the Google
  store and update hosts are refused), which also gives admins an allow-list of extensions for
  free. Decided (2026-10-10): keep Google's publisher signature on the CRX files for now (serve
  them byte for byte; re-signing with the server's own key can come later). Open point:
  extensions that are "Chrome only".
  - **Status 2026-10-11 night (server side built, test instance only):** catalogue, CRX3
    verification (RSA incl. 1024-bit legacy keys, ECDSA, Google publisher proof), both update
    protocols (gupdate XML and update_client 4.0 JSON), byte-identical package serving, icons,
    store page with sign-in and per-user scoping, admin pages (approve by link/ID, assign, hold,
    roll back, check now), 4-hourly checker. Verified against uBlock Origin Lite, Dark Reader and
    Grammarly on a separate server (port 8297). Not yet: AD group / PC matching on the store page,
    how "install automatically" reaches browsers, the browser patch (store/update addresses from
    policy, Google's update address mapped to ours, Google's store blocked), ADMX entries.
  - **Store address from Group Policy (2026-10-11).** Every organisation runs its own sync
    server, so the store / download / update addresses are not built in: a BrowseCraft policy
    (e.g. `ExtensionStoreUrl`, beside the sync server address) sets them, through the ADMX
    templates on Windows and the JSON policy file on Linux. With no policy set, the store is
    simply off (Google's stays blocked).
  - **Who gets which extension: per user, per AD group or per PC, chosen per extension
    (2026-10-11).** Each approved extension has assignments: everyone, named users, AD groups
    (through the server's existing LDAP connection), named PCs or AD computer groups; and a mode
    for each: offered in the store, or installed automatically. User and group assignments follow
    the person to any PC they sign in to (the server can hand them out through the sync connection,
    which already knows who is signed in); PC assignments stay with the machine whoever uses it
    (machine policy: Chromium's own force-install list, pointed at our update address, or the
    server matching the device name). The store page shows each person only what they may install.
  - **Automatic updates, pushed to the estate (2026-10-10).** The server checks the Chrome
    Web Store for new versions of every approved extension on a schedule (one batched update
    query for all of them, say every few hours, plus a "check now" button), downloads the new
    CRX, checks it is the same extension ID and that Google's signature verifies, and keeps the
    previous version so an admin can roll back. Browsers then pick the update up from the sync
    server: BrowseCraft checks the server's update address far more often than Chrome's 5 hours,
    and the server can nudge browsers straight away through the sync connection they already
    poll every minute, so an update reaches every PC within minutes. Per-extension choice of
    "update automatically" or "hold for an admin to approve"; an update that asks for new
    permissions is held for approval by default (Chrome would otherwise disable it on every PC
    until each user accepts). The admin page shows which version each device is running.
- ~~Single sign-on from domain PCs (Kerberos / Negotiate), so domain users never type a password.~~
  Built 2026-10-12 (Windows; needs the server's SPN, keytab and look-up account: server/docker-compose.yml).

## Management

- **Group Policy templates for Active Directory (2026-10-10).** Ship `browsecraft.admx` and
  its `.adml` (en-GB/en-US) so BrowseCraft can be run from GPO: copy them into the domain's
  Central Store (`\\<domain>\SYSVOL\<domain>\Policies\PolicyDefinitions`) and a "BrowseCraft"
  section appears in Group Policy Management, under Computer and User Configuration, with
  Mandatory and Recommended halves like Chrome's.
  - First, BrowseCraft gets its own policy location: today, like any Chromium build, it reads
    `HKLM/HKCU\SOFTWARE\Policies\Chromium` and `/etc/chromium/policies`, the same places as an
    installed (ungoogled-)Chromium, so a Chromium policy would also steer BrowseCraft. Move it to
    `SOFTWARE\Policies\BrowseCraft` and `/etc/browsecraft/policies`.
  - Generate the templates from Chromium's own policy definitions and template writer
    (components/policy/tools/template_writers), rebranded: every standard Chrome policy is then
    there (home page, search engine, password manager, extension allow/block/force-install lists,
    proxy, downloads, etc.), kept current with each Chromium update.
  - Add BrowseCraft's own policies: the sync server address; sign in automatically / require
    sign-in to it (with domain single sign-on once that exists); the extension store address,
    blocking the genuine Chrome Web Store, and force-installed extensions from our store.
  - Also the matching Linux JSON examples, and a short "deploying with GPO" page.

## Browser

- Windows installer (mini_installer needs Windows archiving tools; or our own MSI).
- Official (LTO/PGO) release builds.
