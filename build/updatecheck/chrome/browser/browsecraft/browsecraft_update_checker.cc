// Copyright 2026 The BrowseCraft Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "chrome/browser/browsecraft/browsecraft_update_checker.h"

#include <utility>

#include "base/command_line.h"
#include "base/files/file_enumerator.h"
#include "base/files/file_path.h"
#include "base/files/file_util.h"
#include "base/functional/bind.h"
#include "base/json/json_reader.h"
#include "base/logging.h"
#include "base/no_destructor.h"
#include "base/strings/string_number_conversions.h"
#include "base/strings/stringprintf.h"
#include "base/strings/utf_string_conversions.h"
#include "base/task/thread_pool.h"
#include "base/time/time.h"
#include "build/build_config.h"
#include "chrome/browser/browser_process.h"
#include "chrome/browser/ui/browser_window/public/browser_window_interface_iterator.h"
#include "chrome/browser/ui/singleton_tabs.h"
#include "chrome/browser/ui/webui/settings/browsecraft_version.h"
#include "components/prefs/pref_registry_simple.h"
#include "components/prefs/pref_service.h"
#include "content/public/browser/web_ui_data_source.h"
#include "net/base/load_flags.h"
#include "net/http/http_response_headers.h"
#include "net/traffic_annotation/network_traffic_annotation.h"
#include "services/network/public/cpp/resource_request.h"
#include "services/network/public/cpp/shared_url_loader_factory.h"
#include "services/network/public/cpp/simple_url_loader.h"
#include "services/network/public/mojom/url_response_head.mojom.h"

#if BUILDFLAG(IS_WIN)
#include <windows.h>

#include "base/win/registry.h"
#endif

namespace browsecraft::update {

namespace {

constexpr char kDefaultUrl[] =
    "https://altcraft.perkin.me.uk/browsecraft/latest.json";
constexpr char kSwitchDisable[] = "disable-browsecraft-update-check";
constexpr char kSwitchUrl[] = "browsecraft-update-url";
// For tests: seconds before the first check and between checks, and "today".
constexpr char kSwitchDelay[] = "browsecraft-update-check-delay";
constexpr char kSwitchInterval[] = "browsecraft-update-check-interval";
constexpr char kSwitchToday[] = "browsecraft-update-today";

constexpr net::NetworkTrafficAnnotationTag kTrafficAnnotation =
    net::DefineNetworkTrafficAnnotation("browsecraft_update_check", R"(
        semantics {
          sender: "BrowseCraft update check"
          description:
            "Fetches a small public file naming the newest BrowseCraft "
            "version, so the user can be told about updates, above all "
            "security updates."
          trigger: "Shortly after start-up, then once a day."
          data: "None. A plain GET; no cookies, no identifiers."
          destination: OTHER
          destination_other: "The BrowseCraft download site."
        }
        policy {
          cookies_allowed: NO
          setting: "chrome://browsecraft: Check for a new BrowseCraft."
          policy_exception_justification: "BrowseCraft policy UpdateCheckEnabled."
        })");

base::CommandLine& Cmd() {
  return *base::CommandLine::ForCurrentProcess();
}

int SwitchSeconds(const char* name, int fallback) {
  int v = 0;
  if (Cmd().HasSwitch(name) &&
      base::StringToInt(Cmd().GetSwitchValueASCII(name), &v) && v >= 0) {
    return v;
  }
  return fallback;
}

PrefService* Local() {
  return g_browser_process ? g_browser_process->local_state() : nullptr;
}

std::string Today() {
  if (Cmd().HasSwitch(kSwitchToday)) {
    return Cmd().GetSwitchValueASCII(kSwitchToday);
  }
  base::Time::Exploded e;
  base::Time::Now().LocalExplode(&e);
  return base::StringPrintf("%04d-%02d-%02d", e.year, e.month, e.day_of_month);
}

// Policy UpdateCheckEnabled (0/false = off) and UpdateCheckUrl, read off the
// UI thread. Windows: HKLM wins over HKCU. Linux: the managed JSON files.
std::pair<std::optional<bool>, std::string> ReadPolicy() {
  std::optional<bool> enabled;
  std::string url;
#if BUILDFLAG(IS_WIN)
  for (HKEY root : {HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE}) {  // HKLM last: wins
    base::win::RegKey key(root, L"SOFTWARE\\Policies\\BrowseCraft", KEY_READ);
    if (!key.Valid()) {
      continue;
    }
    DWORD d = 0;
    if (key.ReadValueDW(L"UpdateCheckEnabled", &d) == ERROR_SUCCESS) {
      enabled = d != 0;
    }
    std::wstring w;
    if (key.ReadValue(L"UpdateCheckUrl", &w) == ERROR_SUCCESS && !w.empty()) {
      url = base::WideToUTF8(w);
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
    std::optional<base::DictValue> d =
        base::JSONReader::ReadDict(text, base::JSON_PARSE_RFC);
    if (!d) {
      continue;
    }
    if (const base::Value* v = d->Find("UpdateCheckEnabled")) {
      if (v->is_bool()) {
        enabled = v->GetBool();
      } else if (v->is_int()) {
        enabled = v->GetInt() != 0;
      }
    }
    if (const std::string* s = d->FindString("UpdateCheckUrl")) {
      url = *s;
    }
  }
#endif
  return {enabled, url};
}

// Opens the update tab for a security update: at the check after a browser
// start, once a day at most, never for a skipped version (13 Nov 2026).
class UpdateNotifier : public Observer {
 public:
  void OnUpdateFound(const UpdateInfo& info, bool at_startup) override {
    PrefService* local = Local();
    if (!local) {
      return;
    }
    const std::string today = Today();
    if (!ShouldOpenTab(info, UpdateChecker::Get()->Wanted(), at_startup, today,
                       local->GetString(kPrefLastTabDay))) {
      return;
    }
    BrowserWindowInterface* browser =
        GetLastActiveBrowserWindowInterfaceWithAnyProfile();
    if (!browser) {
      return;  // no window yet: the next start will do
    }
    VLOG(1) << "BrowseCraft update: opening the update tab for v"
            << info.version;
    LOG(WARNING) << "BrowseCraft update: security update v" << info.version
                 << " - opening " << kUpdatePageUrl;
    ShowSingletonTab(browser, GURL(kUpdatePageUrl));
    local->SetString(kPrefLastTabDay, today);
  }
};

}  // namespace

// static
UpdateChecker* UpdateChecker::Get() {
  static base::NoDestructor<UpdateChecker> instance;
  return instance.get();
}

// static
void UpdateChecker::RegisterLocalStatePrefs(PrefRegistrySimple* registry) {
  registry->RegisterBooleanPref(kPrefEnabled, true);
  registry->RegisterStringPref(kPrefManifest, std::string());
  registry->RegisterStringPref(kPrefManifestUrl, std::string());
  registry->RegisterStringPref(kPrefLastCheck, std::string());
  registry->RegisterStringPref(kPrefSkipped, std::string());
  registry->RegisterStringPref(kPrefLastTabDay, std::string());
}

UpdateChecker::UpdateChecker() : notifier_(std::make_unique<UpdateNotifier>()) {
  observers_.AddObserver(notifier_.get());
}

UpdateChecker::~UpdateChecker() = default;

void UpdateChecker::Start() {
  if (started_) {
    return;
  }
  started_ = true;
  base::ThreadPool::PostTaskAndReplyWithResult(
      FROM_HERE, {base::MayBlock(), base::TaskPriority::BEST_EFFORT},
      base::BindOnce(&ReadPolicy),
      base::BindOnce(&UpdateChecker::OnPolicyRead,
                     weak_factory_.GetWeakPtr()));
}

void UpdateChecker::OnPolicyRead(
    std::pair<std::optional<bool>, std::string> policy) {
  policy_read_ = true;
  policy_off_ = policy.first.has_value() && !*policy.first;
  url_ = GURL(Cmd().HasSwitch(kSwitchUrl) ? Cmd().GetSwitchValueASCII(kSwitchUrl)
              : !policy.second.empty()   ? policy.second
                                         : std::string(kDefaultUrl));
  if (!url_.is_valid() || !url_.SchemeIsHTTPOrHTTPS()) {
    url_ = GURL(kDefaultUrl);
  }
  Schedule(base::Seconds(SwitchSeconds(kSwitchDelay, 30)));
}

bool UpdateChecker::DisabledByAdmin() const {
  return policy_off_ || Cmd().HasSwitch(kSwitchDisable);
}

bool UpdateChecker::Enabled() const {
  PrefService* local = Local();
  return local && local->GetBoolean(kPrefEnabled) && !DisabledByAdmin();
}

void UpdateChecker::SetEnabled(bool enabled) {
  if (PrefService* local = Local()) {
    local->SetBoolean(kPrefEnabled, enabled);
    local->CommitPendingWrite();  // the user's choice survives a crash
  }
  if (enabled && policy_read_ && !timer_.IsRunning() && !loader_) {
    at_startup_ = false;  // turned on mid-session: no tab until the next start
    Schedule(base::Seconds(5));
  }
}

void UpdateChecker::Schedule(base::TimeDelta delay) {
  timer_.Start(FROM_HERE, delay,
               base::BindOnce(&UpdateChecker::Check, base::Unretained(this)));
}

void UpdateChecker::Check() {
  if (!Enabled()) {
    return;  // checked again only if it is turned back on
  }
  auto request = std::make_unique<network::ResourceRequest>();
  request->url = url_;
  request->method = "GET";
  request->credentials_mode = network::mojom::CredentialsMode::kOmit;
  request->load_flags = net::LOAD_BYPASS_CACHE | net::LOAD_DISABLE_CACHE;
  loader_ = network::SimpleURLLoader::Create(std::move(request),
                                             kTrafficAnnotation);
  loader_->SetTimeoutDuration(base::Seconds(30));
  loader_->DownloadToString(
      g_browser_process->shared_url_loader_factory().get(),
      base::BindOnce(&UpdateChecker::OnFetched, base::Unretained(this)),
      kMaxManifestBytes);
}

void UpdateChecker::OnFetched(std::optional<std::string> body) {
  const int code = loader_->ResponseInfo() && loader_->ResponseInfo()->headers
                       ? loader_->ResponseInfo()->headers->response_code()
                       : 0;
  loader_.reset();
  PrefService* local = Local();
  if (local && body && code == 200) {
    if (ParseManifest(*body, url_)) {  // a bad file is never kept or shown
      local->SetString(kPrefManifest, *body);
      local->SetString(kPrefManifestUrl, url_.spec());
      base::Time::Exploded e;
      base::Time::Now().LocalExplode(&e);
      local->SetString(kPrefLastCheck,
                       base::StringPrintf("%04d-%02d-%02d %02d:%02d", e.year,
                                          e.month, e.day_of_month, e.hour,
                                          e.minute));
    } else {
      LOG(WARNING) << "BrowseCraft update: " << url_.spec()
                   << " is not a valid latest.json: ignored";
    }
  } else {
    VLOG(1) << "BrowseCraft update: no answer from " << url_.spec()
            << " (HTTP " << code << ")";
  }
  // A failed check still tells about what the last good one found.
  Tell();
  at_startup_ = false;
  Schedule(base::Seconds(SwitchSeconds(kSwitchInterval, 24 * 3600)));
}

void UpdateChecker::Tell() {
  std::optional<UpdateInfo> info = Latest();
  if (!info || !Wanted()) {
    return;
  }
  LOG(WARNING) << "BrowseCraft update: v" << info->version << " is available"
               << (info->security.update ? " (security update)" : "")
               << ", running v" << RunningVersion();
  for (Observer& o : observers_) {
    o.OnUpdateFound(*info, at_startup_);
  }
}

std::string UpdateChecker::RunningVersion() const {
  return base::UTF16ToUTF8(kBrowseCraftVersion);
}

std::optional<UpdateInfo> UpdateChecker::Latest() const {
  PrefService* local = Local();
  if (!local || local->GetString(kPrefManifest).empty()) {
    return std::nullopt;
  }
  return ParseManifest(local->GetString(kPrefManifest),
                       GURL(local->GetString(kPrefManifestUrl)));
}

bool UpdateChecker::Newer() const {
  std::optional<UpdateInfo> info = Latest();
  return info && CompareVersions(info->version, RunningVersion()) > 0;
}

bool UpdateChecker::Wanted() const {
  std::optional<UpdateInfo> info = Latest();
  PrefService* local = Local();
  return Enabled() && info && local &&
         IsWanted(RunningVersion(), info->version,
                  local->GetString(kPrefSkipped));
}

bool UpdateChecker::Skipped() const {
  std::optional<UpdateInfo> info = Latest();
  PrefService* local = Local();
  return info && local && !local->GetString(kPrefSkipped).empty() &&
         CompareVersions(info->version, local->GetString(kPrefSkipped)) <= 0;
}

void UpdateChecker::SetSkipped(bool skip) {
  std::optional<UpdateInfo> info = Latest();
  PrefService* local = Local();
  if (!local) {
    return;
  }
  if (skip && info) {
    local->SetString(kPrefSkipped, info->version);
  } else if (!skip) {
    local->ClearPref(kPrefSkipped);
  }
  local->CommitPendingWrite();  // the user's choice survives a crash
}

base::DictValue UpdateChecker::State() const {
  base::DictValue d;
  d.Set("enabled", Enabled());
  d.Set("setting", Local() && Local()->GetBoolean(kPrefEnabled));
  d.Set("managedOff", DisabledByAdmin());
  d.Set("running", RunningVersion());
  d.Set("lastCheck", Local() ? Local()->GetString(kPrefLastCheck) : "");
  d.Set("newer", Newer());
  d.Set("wanted", Wanted());
  d.Set("skipped", Skipped());
  if (std::optional<UpdateInfo> info = Latest()) {
    base::DictValue l;
    l.Set("version", info->version);
    l.Set("chromium", info->chromium);
    l.Set("released", info->released);
    l.Set("page", info->page);
    base::DictValue s;
    s.Set("update", info->security.update);
    s.Set("fixes", info->security.fixes);
    s.Set("highest", info->security.highest);
    s.Set("inTheWild", info->security.in_the_wild);
    s.Set("summary", info->security.summary);
    s.Set("source", info->security.source);
    base::ListValue refs;
    for (const std::string& r : info->security.references) {
      refs.Append(r);
    }
    s.Set("references", std::move(refs));
    l.Set("security", std::move(s));
    d.Set("latest", std::move(l));
  }
  return d;
}

std::u16string UpdateChecker::MenuLabel() const {
  std::optional<UpdateInfo> info = Latest();
  if (!info) {
    return std::u16string();
  }
  return info->security.update
             ? u"Security update: BrowseCraft v" +
                   base::UTF8ToUTF16(info->version)
             : u"Update available: BrowseCraft v" +
                   base::UTF8ToUTF16(info->version);
}

void UpdateChecker::AddAboutStrings(content::WebUIDataSource* source) const {
  std::optional<UpdateInfo> info = Latest();
  const bool show = info && Wanted();
  std::u16string text;
  if (show) {
    text = info->security.update
               ? u"Security update available: BrowseCraft v" +
                     base::UTF8ToUTF16(info->version) + u" (Chromium " +
                     base::UTF8ToUTF16(info->chromium) + u")."
               : u"Update available: BrowseCraft v" +
                     base::UTF8ToUTF16(info->version) + u" (Chromium " +
                     base::UTF8ToUTF16(info->chromium) + u").";
  }
  source->AddString("browsecraftUpdateText", text);
  // The About page hides the line with a class (a $i18n value can only sit in
  // text or a quoted attribute there).
  source->AddString("browsecraftUpdateClass",
                    !show                    ? "bc-hidden"
                    : info->security.update ? "bc-security"
                                            : "");
  source->AddString("browsecraftUpdatePage", show ? info->page : "");
}

}  // namespace browsecraft::update
