# BrowseCraft

A self-hosted Chrome. BrowseCraft is Chromium (with the
[ungoogled-chromium](https://github.com/ungoogled-software/ungoogled-chromium) changes, so it
does not talk to Google) that you sign in to **your own server** instead of a Google or
Microsoft account. Your bookmarks, passwords, history, open tabs, extensions and settings then
follow you to every PC you sign in on, and live only in your home lab.

Two parts:

| Part | What it is |
|---|---|
| `server/` | **BrowseCraft sync server**: one small Rust program (or Docker container) with a SQLite file. It speaks Chrome's own sync protocol, so everything Chrome can sync, BrowseCraft can. Users and devices are managed on its web page. |
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

In BrowseCraft open `chrome://browsecraft`, enter your server's address, user name and
password, then restart BrowseCraft. Sign in the same way on every PC.

To keep your passwords unreadable even to the server, set a sync passphrase in BrowseCraft's
sync settings (Encryption options): synced data is then encrypted in the browser before it is
sent.

## How it works

Chromium already has a way to sync without a Google account: the *local sync backend* used for
roaming profiles, which keeps sync data in a file. BrowseCraft keeps that mode (no Google
sign-in needed) but sends sync traffic over the network to your BrowseCraft server, with the
token your server gave the browser when you signed in. The server is a port of Chromium's own
loopback sync server (BSD licence) onto SQLite, one store per user. Browsers check for changes
every minute (Google's servers push them; yours does not need to).

## Licence

BSD-3-Clause, like Chromium. The server's protocol handling is ported from Chromium's
`components/sync/engine/loopback_server` (Copyright The Chromium Authors).

## Windows installer

`packaging/windows/build-installer.sh <browsecraft-…-windows-x64….zip>` makes **BrowseCraft.exe**,
the installer (an MSI inside a WiX Burn bundle, built on a Windows host over SSH because WiX
needs Windows). Its version is in the file's properties: Chromium's three numbers and this
repo's commit count (155.0.8059.25), so every build is newer than the last.

- Per machine, in `Program Files\BrowseCraft` (folder under Options); an upgrade keeps it there.
- Start menu and desktop shortcuts are plain ones to `chrome.exe`, so a taskbar pin survives upgrades.
- Registered as a browser: pick it under Settings > Apps > Default apps.
- **Widevine** (Netflix, Audible, Spotify) is Google's and not ours to ship. `Update-Widevine.ps1`
  copies the newest module on the PC (from Edge, or Chrome) beside `chrome.dll` at install, and
  a SYSTEM task ("BrowseCraft Widevine", at startup and daily) keeps it current. Log:
  `%ProgramData%\BrowseCraft\widevine.log`.
- Profiles (`%LOCALAPPDATA%\BrowseCraft\User Data`) are never touched by an upgrade or uninstall.
- Silent: `BrowseCraft.exe /quiet`; uninstall from Add/Remove Programs or with its `/uninstall /quiet`.
