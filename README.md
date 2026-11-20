# AltBrowse

A self-hosted Chrome. AltBrowse (called BrowseCraft until 20 Nov 2026) is Chromium (with the
[ungoogled-chromium](https://github.com/ungoogled-software/ungoogled-chromium) changes, so it
does not talk to Google) that you sign in to **your own server** instead of a Google or
Microsoft account. Your bookmarks, passwords, history, open tabs, extensions and settings then
follow you to every PC you sign in on, and live only in your home lab.

Two parts:

| Part | What it is |
|---|---|
| `server/` | **AltBrowse sync server**: one small Rust program (or Docker container) with a SQLite file. It speaks Chrome's own sync protocol, so everything Chrome can sync, AltBrowse can. Users and devices are managed on its web page. |
| `patches/` | **The browser**: patches on top of ungoogled-chromium. `chrome://browsecraft` signs in to your server; sync then goes there. |

## Run the server

```sh
cd server
docker compose up -d
```

Open `http://<host>:8295` and create the first user (the admin). Add the others on the admin
page, or with `docker exec browsecraft-sync browsecraft-server add-user NAME`.

Use HTTPS (your reverse proxy: Nginx Proxy Manager, Caddy, Traefik...) for anything beyond a
trusted LAN: each browser sends its sign-in token with every sync request.

Settings (environment): `BROWSECRAFT_LISTEN` (default `0.0.0.0:8295`), `BROWSECRAFT_DB`
(default `/data/browsecraft.db`), and on first start `BROWSECRAFT_ADMIN_USER` /
`BROWSECRAFT_ADMIN_PASSWORD` to make the admin without the web page. Back up by copying the
database file.

## Sign in from the browser

In AltBrowse open `chrome://browsecraft`, enter your server's address, user name and
password, then restart AltBrowse. Sign in the same way on every PC.

To keep your passwords unreadable even to the server, set a sync passphrase in AltBrowse's
sync settings (Encryption options): synced data is then encrypted in the browser before it is
sent.

## How it works

Chromium already has a way to sync without a Google account: the *local sync backend* used for
roaming profiles, which keeps sync data in a file. AltBrowse keeps that mode (no Google
sign-in needed) but sends sync traffic over the network to your AltBrowse server, with the
token your server gave the browser when you signed in. The server is a port of Chromium's own
loopback sync server (BSD licence) onto SQLite, one store per user. Browsers check for changes
every minute (Google's servers push them; yours does not need to).

## Licence

BSD-3-Clause, like Chromium. The server's protocol handling is ported from Chromium's
`components/sync/engine/loopback_server` (Copyright The Chromium Authors).

## Windows installer

`packaging/windows/build-installer.sh <altbrowse-…-windows-x64….zip>` makes **AltBrowse-<version>-x64.exe**,
the installer (an MSI inside a WiX Burn bundle, built on a Windows host over SSH because WiX
needs Windows). Its version is in the file's properties: Chromium's three numbers and this
repo's commit count (155.0.8059.25), so every build is newer than the last.

- Per machine, in `Program Files\AltBrowse` (folder under Options); an upgrade keeps the folder it
  is in (`Program Files\BrowseCraft` for an install from before the rename).
- Start menu and desktop shortcuts are plain ones to `chrome.exe`, so a taskbar pin survives upgrades.
- Registered as a browser: pick it under Settings > Apps > Default apps.
- **Widevine** (Netflix, Audible, Spotify) is Google's and not ours to ship. `Update-Widevine.ps1`
  copies the newest module on the PC (from Edge, or Chrome) beside `chrome.dll` at install, and
  a SYSTEM task ("AltBrowse Widevine", at startup and daily) keeps it current. Log:
  `%ProgramData%\BrowseCraft\widevine.log`.
- Profiles (`%LOCALAPPDATA%\BrowseCraft\User Data`) are never touched by an upgrade or uninstall.
- Silent: `AltBrowse-<version>-x64.exe /quiet`; uninstall from Add/Remove Programs or with its `/uninstall /quiet`.
- **The rename (BrowseCraft -> AltBrowse, 20 Nov 2026)** changed only the names people see. These
  keep the old name on purpose, so an installed BrowseCraft upgrades in place and keeps everything:
  the profile folders (`%LOCALAPPDATA%\BrowseCraft\User Data`, `~/.config/browsecraft`), the
  policies key (`Software\Policies\BrowseCraft`, `/etc/browsecraft/policies`), the prefs, the
  installer's UpgradeCodes and `HKLM\Software\BrowseCraft`, the default-browser registration
  (`StartMenuInternet\BrowseCraft`, ProgIDs `BCraftHTM`/`BCraftPDF`), `chrome://browsecraft`, the
  `--browsecraft-*` switches, the sync server's `BROWSECRAFT_*` settings and database, and the
  update check's `"product": "BrowseCraft"` in latest.json (every installed copy checks for it; new
  builds also take `"AltBrowse"`).
