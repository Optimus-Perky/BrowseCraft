#!/usr/bin/env python3
"""BrowseCraft: the organisation's own extension store (the sync server's /webstore), set by policy.

Policy ExtensionStoreUrl (the sync server's address, e.g. https://browsecraft.example.org):
  Windows  HKLM or HKCU \\SOFTWARE\\Policies\\BrowseCraft  ExtensionStoreUrl (REG_SZ; machine wins)
  Linux    /etc/browsecraft/policies/managed/*.json      {"ExtensionStoreUrl": "..."}
When it is set, at browser start-up it becomes Chromium's own store switches:
  --apps-gallery-url=<store>/webstore/                 (the store page; its origin gets the install API,
                                                        and the switch is passed on to page processes)
  --apps-gallery-update-url=<store>/service/update2/crx (update checks and downloads)
and three small changes make it hold:
  - extensions from the Chrome Web Store (Google's update address in their manifest) count as the
    store's, so they update from the organisation's store, installed or still pending;
  - no other page may install through the store API, so the Chrome Web Store cannot install anything;
  - the store's pages are off-limits to extensions by host AND port, so a store on a bare address
    does not shut extensions out of every other service on that machine.
With no policy, nothing changes (the Chrome Web Store works as before, see webstore.py).

Run in the src tree AFTER domain substitution and webstore.py."""
import sys

EDITS = {
    "chrome/app/chrome_main_delegate.cc": [
        ('#include "base/files/file_util.h"\n',
         '#include "base/files/file_util.h"\n#include "base/files/file_enumerator.h"\n#include "base/json/json_reader.h"\n#include "extensions/common/switches.h"\n#if BUILDFLAG(IS_WIN)\n#include "base/win/registry.h"\n#endif\n'),
        ("std::optional<int> ChromeMainDelegate::BasicStartupComplete() {",
         """// BrowseCraft: the organisation's extension store, from policy ExtensionStoreUrl.
[[maybe_unused]] static std::string BrowseCraftExtensionStoreUrl() {
#if BUILDFLAG(IS_WIN)
  for (HKEY root : {HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER}) {
    base::win::RegKey key(root, L"SOFTWARE\\\\Policies\\\\BrowseCraft", KEY_READ);
    std::wstring value;
    if (key.Valid() &&
        key.ReadValue(L"ExtensionStoreUrl", &value) == ERROR_SUCCESS &&
        !value.empty()) {
      return base::WideToUTF8(value);
    }
  }
#elif BUILDFLAG(IS_LINUX)
  base::FileEnumerator files(
      base::FilePath(FILE_PATH_LITERAL("/etc/browsecraft/policies/managed")),
      false, base::FileEnumerator::FILES, FILE_PATH_LITERAL("*.json"));
  for (base::FilePath f = files.Next(); !f.empty(); f = files.Next()) {
    std::string text;
    if (!base::ReadFileToString(f, &text)) {
      continue;
    }
    std::optional<base::Value> v = base::JSONReader::Read(text, base::JSON_PARSE_RFC);
    if (v && v->is_dict()) {
      const std::string* s = v->GetDict().FindString("ExtensionStoreUrl");
      if (s && !s->empty()) {
        return *s;
      }
    }
  }
#endif
  return std::string();
}

// Turn the policy into Chromium's store switches (an explicit --apps-gallery-url wins).
[[maybe_unused]] static void ApplyBrowseCraftExtensionStore(base::CommandLine* command_line) {
  if (command_line->HasSwitch(extensions::switches::kAppsGalleryURL)) {
    return;
  }
  // No GURL here: this runs before the URL schemes are registered.
  std::string base = BrowseCraftExtensionStoreUrl();
  if (!base.starts_with("http://") && !base.starts_with("https://")) {
    return;
  }
  while (!base.empty() && base.back() == '/') {
    base.pop_back();
  }
  // With the slash: Chromium resolves "category/extensions" against it and appends "detail/<id>".
  command_line->AppendSwitchASCII(extensions::switches::kAppsGalleryURL,
                                  base + "/webstore/");
  command_line->AppendSwitchASCII(::switches::kAppsGalleryUpdateURL,
                                  base + "/service/update2/crx");
}

std::optional<int> ChromeMainDelegate::BasicStartupComplete() {"""),
        ("  const bool is_browser = !command_line.HasSwitch(switches::kProcessType);\n",
         "  const bool is_browser = !command_line.HasSwitch(switches::kProcessType);\n"
         "  if (is_browser) {\n"
         "    ApplyBrowseCraftExtensionStore(base::CommandLine::ForCurrentProcess());\n"
         "  }\n"),
    ],
    "extensions/common/extension_urls.cc": [
        ("""bool IsWebstoreUpdateUrl(const GURL& update_url) {
  GURL store_url = GetWebstoreUpdateUrl();
  return (update_url.host() == store_url.host() &&
          update_url.path() == store_url.path());
}""",
         """bool IsWebstoreUpdateUrl(const GURL& update_url) {
  GURL store_url = GetWebstoreUpdateUrl();
  if (update_url.host() == store_url.host() &&
      update_url.path() == store_url.path()) {
    return true;
  }
  // BrowseCraft: with the organisation's store in use, extensions from the
  // Chrome Web Store (Google's update address) are the store's too.
  GURL google = GetDefaultWebstoreUpdateUrl();
  return store_url != google && update_url.host() == google.host() &&
         update_url.path() == google.path();
}"""),
        # Chromium keeps every extension off the store's pages, matching the host name only. A store
        # on a bare address (http://192.0.2.10:8295) then claimed every service on that machine:
        # LastPass and the rest could not run on the arr apps next to it. Match the port too.
        ("""bool IsWebstoreDomain(const GURL& url) {
  return url.DomainIs(GetWebstoreLaunchURL().host()) ||
         url.DomainIs(GetNewWebstoreLaunchURL().host());
}""",
         """bool IsWebstoreDomain(const GURL& url) {
  // BrowseCraft: the port too, so a store on a shared machine claims only itself.
  auto is_store = [&url](const GURL& store) {
    return url.DomainIs(store.host()) &&
           url.EffectiveIntPort() == store.EffectiveIntPort();
  };
  return is_store(GetWebstoreLaunchURL()) || is_store(GetNewWebstoreLaunchURL());
}"""),
    ],
    # The same host-only match hid every request to that machine from extensions (webRequest),
    # and blocked guest-view navigations to it.
    "extensions/browser/api/web_request/web_request_permissions.cc": [
        ("""      url.DomainIs(extension_urls::GetNewWebstoreLaunchURL().host())) {""",
         """      (url.DomainIs(extension_urls::GetNewWebstoreLaunchURL().host()) &&
       url.EffectiveIntPort() ==
           extension_urls::GetNewWebstoreLaunchURL().EffectiveIntPort())) {"""),
    ],
    "extensions/browser/extension_navigation_throttle.cc": [
        ("""      if (url.DomainIs(extension_urls::GetNewWebstoreLaunchURL().GetHost())) {""",
         """      if (url.DomainIs(extension_urls::GetNewWebstoreLaunchURL().GetHost()) &&
          url.EffectiveIntPort() ==
              extension_urls::GetNewWebstoreLaunchURL().EffectiveIntPort()) {"""),
    ],
    # Domain substitution also scrambled the update manifest's namespace, so every update
    # answer in Google's format (the store's or ours) failed: "parsing manifest failed".
    "extensions/browser/updater/safe_manifest_parser.cc": [
        ('"http://www.9oo91e.qjz9zk/update2/response";', '"http://www.google.com/update2/response";'),
    ],
    "extensions/browser/updater/extension_downloader.cc": [
        ("""  // Make sure we use SSL for store-hosted extensions.
  if (extension_urls::IsWebstoreUpdateUrl(update_url) &&
      !update_url.SchemeIsCryptographic()) {
    return extension_urls::GetWebstoreUpdateUrl();
  }""",
         """  // Store extensions check the store in use (BrowseCraft: the organisation's,
  // when set; this also covers Google's address in Chrome Web Store ones).
  if (extension_urls::IsWebstoreUpdateUrl(update_url)) {
    return extension_urls::GetWebstoreUpdateUrl();
  }"""),
    ],
    "extensions/browser/api/webstore_private/webstore_private_api.cc": [
        ("""WebstorePrivateBeginInstallWithManifest3Function::Run() {
  params_ = Params::Create(args());
  EXTENSION_FUNCTION_VALIDATE(params_);
""",
         """WebstorePrivateBeginInstallWithManifest3Function::Run() {
  params_ = Params::Create(args());
  EXTENSION_FUNCTION_VALIDATE(params_);

  // BrowseCraft: with the organisation's store set, only its page installs.
  {
    const base::CommandLine* cl = base::CommandLine::ForCurrentProcess();
    if (cl->HasSwitch(switches::kAppsGalleryURL) &&
        !url::Origin::Create(
             GURL(cl->GetSwitchValueASCII(switches::kAppsGalleryURL)))
             .IsSameOriginWith(source_url())) {
      return RespondNow(BuildResponse(
          api::webstore_private::Result::kBlockedByPolicy,
          "Extensions are installed from your organisation's store."));
    }
  }
"""),
        ('#include "extensions/browser/api/webstore_private/webstore_private_api.h"\n',
         '#include "extensions/browser/api/webstore_private/webstore_private_api.h"\n\n#include "base/command_line.h"\n#include "extensions/common/switches.h"\n#include "url/origin.h"\n'),
    ],
    # ungoogled-chromium's remove-uneeded-ui.patch takes the store out of the Extensions menu; put
    # it back: "Visit extension store" with the organisation's store, Chrome's own wording without.
    "chrome/browser/ui/toolbar/app_menu_model.cc": [
        ('#include "base/command_line.h"\n', '#include "base/command_line.h"\n#include "extensions/common/switches.h"\n'),
        ("""  SetElementIdentifierAt(
      GetIndexOfCommandId(IDC_EXTENSIONS_SUBMENU_MANAGE_EXTENSIONS).value(),
      kManageExtensionsMenuItem);
}""",
         """  SetElementIdentifierAt(
      GetIndexOfCommandId(IDC_EXTENSIONS_SUBMENU_MANAGE_EXTENSIONS).value(),
      kManageExtensionsMenuItem);
  // BrowseCraft: the store, which ungoogled-chromium removes from this menu.
  if (base::CommandLine::ForCurrentProcess()->HasSwitch(
          extensions::switches::kAppsGalleryURL)) {
    AddItem(IDC_EXTENSIONS_SUBMENU_VISIT_CHROME_WEB_STORE,
            u"Visit extension store");
  } else {
    AddItemWithStringId(IDC_EXTENSIONS_SUBMENU_VISIT_CHROME_WEB_STORE,
                        IDS_EXTENSIONS_SUBMENU_CHROME_WEBSTORE_ITEM);
  }
  SetElementIdentifierAt(
      GetIndexOfCommandId(IDC_EXTENSIONS_SUBMENU_VISIT_CHROME_WEB_STORE).value(),
      kVisitChromeWebStoreMenuItem);
}"""),
    ],
}


def main():
    failed = False
    for path, edits in EDITS.items():
        try:
            text = open(path, encoding="utf-8").read()
        except OSError as e:
            print(f"extstore.py: {path}: {e}", file=sys.stderr)
            failed = True
            continue
        for old, new in edits:
            if new in text:
                continue  # already applied
            n = text.count(old)
            if n != 1:
                print(f"extstore.py: {path}: expected one match, found {n}: {old[:70]!r}", file=sys.stderr)
                failed = True
                continue
            text = text.replace(old, new, 1)
        if not failed:
            open(path, "w", encoding="utf-8").write(text)
    if failed:
        sys.exit(1)
    print("extstore.py: organisation extension store applied")


if __name__ == "__main__":
    main()
