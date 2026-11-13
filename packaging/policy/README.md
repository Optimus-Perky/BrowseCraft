# BrowseCraft policies

BrowseCraft's own settings. (Chromium's own policies, such as ExtensionInstallForcelist or
URLBlocklist, still work as in Chromium.)

| Setting | Windows registry (machine wins over user) | Linux |
|---|---|---|
| `ExtensionStoreUrl`: your BrowseCraft server, whose `/webstore` becomes the only extension store | `HKLM` or `HKCU\SOFTWARE\Policies\BrowseCraft`, `ExtensionStoreUrl` (REG_SZ) | any `*.json` in `/etc/browsecraft/policies/managed/` (see `browsecraft-linux-example.json`) |
| `UpdateCheckEnabled`: 0 turns the daily update check off (and users cannot turn it on) | `UpdateCheckEnabled` (REG_DWORD 0/1) | `"UpdateCheckEnabled": false` |
| `UpdateCheckUrl`: the latest.json to check instead of BrowseCraft's own (an organisation's tested builds) | `UpdateCheckUrl` (REG_SZ) | `"UpdateCheckUrl": "https://..."` |

**Group Policy:** copy `browsecraft.admx` to `PolicyDefinitions` and `en-US/browsecraft.adml` to
`PolicyDefinitions\en-US` in the central store
(`\\<domain>\SYSVOL\<domain>\Policies\PolicyDefinitions`). The setting is under
Administrative Templates > BrowseCraft, for computers and for users.

To give every PC an extension whoever signs in, use Chromium's `ExtensionInstallForcelist` with
your server's update address: `<extension id>;https://<your server>/service/update2/crx`.

## Chromium's own settings (`chromium-155/`)

BrowseCraft reads Chromium's policies from `Software\Policies\Chromium` (Windows) and
`/etc/chromium/policies/` (Linux), so the Chromium-branded templates apply: `chromium.admx` and
`en-US/chromium.adml` (739 settings, Chromium 155), installed in the domain's central store
2026-10-12. They show under Administrative Templates > Chromium.

Regenerating them for a new Chromium: build `components/policy:policy_templates` in the Windows
tree (TARGETS=components/policy:policy_templates with win-build.sh). That build passes
`-D _google_chrome=false`, and the template writer only checks that `_google_chrome` is present,
so it writes GOOGLE CHROME templates (`Software\Policies\Google\Chrome`). Rerun its
template_formatter.py command (`ninja -t commands components/policy:policy_templates | tail -1`
in out/Win) with `-D _chromium` in place of `-D _google_chrome=false` and without
`-D _is_chrome_for_testing_branded=false`; the files land in
`out/Win/gen/chrome/app/policy/windows/admx/` as chrome.admx / chrome.adml (UTF-16): rename to
chromium.*.
