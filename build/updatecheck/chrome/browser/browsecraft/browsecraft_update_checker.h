// Copyright 2026 The BrowseCraft Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// BrowseCraft's light update check (2026-11-13): shortly after start-up and
// then once a day, a plain GET of latest.json (no cookies, no identifiers).
// Finding a newer version is kept apart from what is shown: observers get
// OnUpdateFound(), and UpdateNotifier is the one that opens the update tab for
// a security update. An "install updates automatically" step would be another
// observer, using UpdateInfo::downloads (installer URL and sha256).
//
// Off: the setting on chrome://browsecraft (Local State
// browsecraft.update.enabled), policy UpdateCheckEnabled = 0 (HKLM/HKCU
// SOFTWARE\Policies\BrowseCraft, or /etc/browsecraft/policies/managed/*.json),
// or --disable-browsecraft-update-check. Policy UpdateCheckUrl or
// --browsecraft-update-url point it at another latest.json.

#ifndef CHROME_BROWSER_BROWSECRAFT_BROWSECRAFT_UPDATE_CHECKER_H_
#define CHROME_BROWSER_BROWSECRAFT_BROWSECRAFT_UPDATE_CHECKER_H_

#include <memory>
#include <optional>
#include <string>

#include "base/memory/weak_ptr.h"
#include "base/no_destructor.h"
#include "base/observer_list.h"
#include "base/observer_list_types.h"
#include "base/timer/timer.h"
#include "base/values.h"
#include "chrome/browser/browsecraft/browsecraft_update_logic.h"
#include "url/gurl.h"

class PrefRegistrySimple;

namespace content {
class WebUIDataSource;
}

namespace network {
class SimpleURLLoader;
}

namespace browsecraft::update {

inline constexpr char kPrefEnabled[] = "browsecraft.update.enabled";
inline constexpr char kPrefManifest[] = "browsecraft.update.manifest";
inline constexpr char kPrefManifestUrl[] = "browsecraft.update.manifest_url";
inline constexpr char kPrefLastCheck[] = "browsecraft.update.last_check";
inline constexpr char kPrefSkipped[] = "browsecraft.update.skipped_version";
inline constexpr char kPrefLastTabDay[] = "browsecraft.update.last_tab_day";

inline constexpr char kUpdatePageUrl[] = "chrome://browsecraft/update";

class Observer : public base::CheckedObserver {
 public:
  // A newer BrowseCraft than the one running, not skipped by the user.
  // |at_startup|: found by the check that follows a browser start (or read
  // back from the last check, when that one fails).
  virtual void OnUpdateFound(const UpdateInfo& info, bool at_startup) = 0;
};

class UpdateChecker {
 public:
  static UpdateChecker* Get();
  static void RegisterLocalStatePrefs(PrefRegistrySimple* registry);

  UpdateChecker(const UpdateChecker&) = delete;
  UpdateChecker& operator=(const UpdateChecker&) = delete;

  // From ChromeBrowserMainParts::PostBrowserStart.
  void Start();

  bool Enabled() const;          // the setting, and not turned off by admin
  bool DisabledByAdmin() const;  // policy or command line
  void SetEnabled(bool enabled);

  std::string RunningVersion() const;
  std::optional<UpdateInfo> Latest() const;  // the last good latest.json
  bool Newer() const;   // Latest() is newer than the running version
  bool Wanted() const;  // ...and the user has not skipped it
  bool Skipped() const;
  void SetSkipped(bool skip);  // skip Latest()'s version, or not

  // For chrome://browsecraft, the About page and the app menu.
  base::DictValue State() const;
  std::u16string MenuLabel() const;
  void AddAboutStrings(content::WebUIDataSource* source) const;

  void AddObserver(Observer* o) { observers_.AddObserver(o); }
  void RemoveObserver(Observer* o) { observers_.RemoveObserver(o); }

 private:
  friend class base::NoDestructor<UpdateChecker>;
  UpdateChecker();
  ~UpdateChecker();

  void OnPolicyRead(std::pair<std::optional<bool>, std::string> policy);
  void Schedule(base::TimeDelta delay);
  void Check();
  void OnFetched(std::optional<std::string> body);
  void Tell();

  bool started_ = false;
  bool policy_read_ = false;
  bool policy_off_ = false;
  bool at_startup_ = true;  // until the first check is done
  GURL url_;
  base::OneShotTimer timer_;
  std::unique_ptr<network::SimpleURLLoader> loader_;
  std::unique_ptr<Observer> notifier_;
  base::ObserverList<Observer> observers_;
  base::WeakPtrFactory<UpdateChecker> weak_factory_{this};
};

}  // namespace browsecraft::update

#endif  // CHROME_BROWSER_BROWSECRAFT_BROWSECRAFT_UPDATE_CHECKER_H_
