# Make BrowseCraft's sync patch from the (already ungoogled) source tree, without touching it.
import os, shutil, subprocess, sys
SRC = "/srv/browsecraft/portablelinux/build/src"
WORK = "/srv/browsecraft/patchwork/sync"
OUT = "/srv/browsecraft/patches/browsecraft/sync-to-own-server.patch"
files = {
 "components/sync/base/pref_names.h": [(
"""inline constexpr char kEnableLocalSyncBackend[] =
    "sync.enable_local_sync_backend";
""",
"""inline constexpr char kEnableLocalSyncBackend[] =
    "sync.enable_local_sync_backend";

// BrowseCraft: the sync server the user signed in to (with local sync on, sync
// goes there instead of to the on-disk loopback server), and the token it gave.
inline constexpr char kBrowseCraftSyncServer[] = "browsecraft.sync.server";
inline constexpr char kBrowseCraftSyncToken[] = "browsecraft.sync.token";
inline constexpr char kBrowseCraftSyncUser[] = "browsecraft.sync.user";
""")],
 "components/sync/service/sync_prefs.cc": [(
"""  registry->RegisterBooleanPref(prefs::kEnableLocalSyncBackend, false);
""",
"""  registry->RegisterBooleanPref(prefs::kEnableLocalSyncBackend, false);
  registry->RegisterStringPref(prefs::kBrowseCraftSyncServer, std::string());
  registry->RegisterStringPref(prefs::kBrowseCraftSyncToken, std::string());
  registry->RegisterStringPref(prefs::kBrowseCraftSyncUser, std::string());
"""),(
"""bool SyncPrefs::IsLocalSyncEnabled() const {
  return local_sync_enabled_;
}
""",
"""bool SyncPrefs::IsLocalSyncEnabled() const {
  return local_sync_enabled_;
}

std::string SyncPrefs::GetBrowseCraftSyncServer() const {
  return pref_service_->GetString(prefs::kBrowseCraftSyncServer);
}

std::string SyncPrefs::GetBrowseCraftSyncToken() const {
  return pref_service_->GetString(prefs::kBrowseCraftSyncToken);
}
""")],
 "components/sync/service/sync_prefs.h": [(
"""  bool IsLocalSyncEnabled() const;
""",
"""  bool IsLocalSyncEnabled() const;

  // BrowseCraft: the user's own sync server and sign-in token (empty when not
  // signed in). Used only with local sync on.
  std::string GetBrowseCraftSyncServer() const;
  std::string GetBrowseCraftSyncToken() const;
""")],
 "components/sync/engine/sync_engine.h": [(
"""    bool enable_local_sync_backend = false;
    base::FilePath local_sync_backend_folder;
    std::unique_ptr<EngineComponentsFactory> engine_components_factory;
""",
"""    bool enable_local_sync_backend = false;
    base::FilePath local_sync_backend_folder;
    // BrowseCraft: with local sync on, a token for the user's own server
    // (`service_url`), which is then used instead of the loopback server.
    std::string remote_access_token;
    std::unique_ptr<EngineComponentsFactory> engine_components_factory;
""")],
 "components/sync/engine/sync_manager.h": [(
"""    bool enable_local_sync_backend = false;
    base::FilePath local_sync_backend_folder;

    // Used to communicate with the sync server.
""",
"""    bool enable_local_sync_backend = false;
    base::FilePath local_sync_backend_folder;

    // BrowseCraft: with local sync on, the token for the user's own server at
    // `service_url`, used instead of the loopback server.
    std::string remote_access_token;

    // Used to communicate with the sync server.
""")],
 "components/sync/service/glue/sync_engine_backend.cc": [(
"""  args.local_sync_backend_folder = params.local_sync_backend_folder;
""",
"""  args.local_sync_backend_folder = params.local_sync_backend_folder;
  args.remote_access_token = params.remote_access_token;
""")],
 "components/sync/engine/sync_manager_impl.cc": [(
"""  if (args->enable_local_sync_backend) {
    VLOG(1) << "Running against local sync backend.";""",
"""  if (args->enable_local_sync_backend && !args->remote_access_token.empty()) {
    // BrowseCraft: no Google account; the user's own server, with the token
    // it gave at sign-in.
    VLOG(1) << "Running against the BrowseCraft server " << args->service_url;
    connection_manager_ = std::make_unique<SyncServerConnectionManager>(
        MakeConnectionURL(args->service_url, args->cache_guid),
        std::move(args->post_factory), args->cancelation_signal);
    signin::AccessTokenInfo token;
    token.token = args->remote_access_token;
    connection_manager_->SetAccessTokenInfo(token);
  } else if (args->enable_local_sync_backend) {
    VLOG(1) << "Running against local sync backend.";""")],
 "chrome/browser/ui/webui/BUILD.gn": [(
"""source_set("configs") {
  sources = [
""",
"""source_set("configs") {
  sources = [
    "browsecraft/browsecraft_ui.cc",
    "browsecraft/browsecraft_ui.h",
""")],
 "chrome/browser/ui/webui/chrome_web_ui_configs.cc": [(
"""  auto& map = content::WebUIConfigMap::GetInstance();
  map.AddWebUIConfig(std::make_unique<AccessibilityUIConfig>());
""",
"""  auto& map = content::WebUIConfigMap::GetInstance();
  map.AddWebUIConfig(std::make_unique<BrowseCraftUIConfig>());
  map.AddWebUIConfig(std::make_unique<AccessibilityUIConfig>());
"""),(
"""#include "chrome/browser/ui/webui/bookmarks/bookmarks_ui.h"
""",
"""#include "chrome/browser/ui/webui/bookmarks/bookmarks_ui.h"
#include "chrome/browser/ui/webui/browsecraft/browsecraft_ui.h"
""")],
 "components/sync/service/sync_service_impl.cc": [(
"""  if (sync_prefs_.IsLocalSyncEnabled()) {
    params.enable_local_sync_backend = true;
    params.local_sync_backend_folder =
        sync_client_->GetLocalSyncBackendFolder();
  }""",
"""  if (sync_prefs_.IsLocalSyncEnabled()) {
    params.enable_local_sync_backend = true;
    params.local_sync_backend_folder =
        sync_client_->GetLocalSyncBackendFolder();
    // BrowseCraft: signed in to the user's own sync server.
    const GURL browsecraft_server(sync_prefs_.GetBrowseCraftSyncServer());
    const std::string browsecraft_token = sync_prefs_.GetBrowseCraftSyncToken();
    if (browsecraft_server.is_valid() && !browsecraft_token.empty()) {
      params.service_url = browsecraft_server;
      params.remote_access_token = browsecraft_token;
    }
  }""")],
}
NEW = "/srv/browsecraft/patches/new"
if os.path.exists(WORK): shutil.rmtree(WORK)
for root, _, names in os.walk(NEW):
    for n in names:
        rel = os.path.relpath(os.path.join(root, n), NEW)
        os.makedirs(os.path.dirname(f"{WORK}/b/{rel}"), exist_ok=True)
        shutil.copy(os.path.join(root, n), f"{WORK}/b/{rel}")
for f, edits in files.items():
    for side in ("a", "b"):
        os.makedirs(os.path.dirname(f"{WORK}/{side}/{f}"), exist_ok=True)
        shutil.copy(f"{SRC}/{f}", f"{WORK}/{side}/{f}")
    p = f"{WORK}/b/{f}"
    s = open(p).read()
    for old, new in edits:
        if s.count(old) != 1:
            sys.exit(f"{f}: expected one match for {old[:60]!r}, found {s.count(old)}")
        s = s.replace(old, new)
    open(p, "w").write(s)
os.makedirs(os.path.dirname(OUT), exist_ok=True)
out = subprocess.run(["diff", "-ruN", "a", "b"], cwd=WORK, capture_output=True, text=True).stdout
open(OUT, "w").write(out)
print(OUT, len(out.splitlines()), "lines")
