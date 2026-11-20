// Copyright 2026 The BrowseCraft Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "chrome/browser/browsecraft/browsecraft_whats_new.h"

#include <vector>

#include "base/functional/bind.h"
#include "base/logging.h"
#include "base/strings/escape.h"
#include "base/strings/string_util.h"
#include "base/strings/utf_string_conversions.h"
#include "base/task/single_thread_task_runner.h"
#include "base/time/time.h"
#include "chrome/browser/browser_process.h"
#include "chrome/browser/browsecraft/browsecraft_release_notes.h"
#include "chrome/browser/browsecraft/browsecraft_update_logic.h"
#include "chrome/browser/profiles/profile.h"
#include "chrome/browser/ui/browser_window/public/browser_window_interface_iterator.h"
#include "chrome/browser/ui/singleton_tabs.h"
#include "chrome/browser/ui/webui/settings/browsecraft_version.h"
#include "components/prefs/pref_registry_simple.h"
#include "components/prefs/pref_service.h"
#include "components/version_info/version_info.h"
#include "url/gurl.h"

namespace browsecraft::whats_new {

namespace {

std::string Running() {
  return base::UTF16ToUTF8(kBrowseCraftVersion);
}

PrefService* Local() {
  return g_browser_process ? g_browser_process->local_state() : nullptr;
}

// Waits for a browser window (start-up is still making it), then opens the tab.
void OpenWhenReady(GURL url, int tries_left) {
  if (BrowserWindowInterface* browser =
          GetLastActiveBrowserWindowInterfaceWithAnyProfile()) {
    LOG(WARNING) << "BrowseCraft what's new: opening " << url.spec();
    ShowSingletonTab(browser, url);
    return;
  }
  if (tries_left > 0) {
    base::SingleThreadTaskRunner::GetCurrentDefault()->PostDelayedTask(
        FROM_HERE, base::BindOnce(&OpenWhenReady, std::move(url), tries_left - 1),
        base::Seconds(2));
  }
}

}  // namespace

void RegisterLocalStatePrefs(PrefRegistrySimple* registry) {
  registry->RegisterBooleanPref(kPrefEnabled, true);
  registry->RegisterStringPref(kPrefLastRun, std::string());
}

bool Enabled() {
  return Local() && Local()->GetBoolean(kPrefEnabled);
}

void SetEnabled(bool enabled) {
  if (Local()) {
    Local()->SetBoolean(kPrefEnabled, enabled);
    Local()->CommitPendingWrite();
  }
}

void OnInitialProfile(Profile* profile) {
  PrefService* local = Local();
  if (!local) {
    return;
  }
  const std::string running = Running();
  const std::string last = local->GetString(kPrefLastRun);
  update::WhatsNew w = update::DecideWhatsNew(last, running,
                                              profile->IsNewProfile(), Enabled());
  // Recorded whatever was decided: once per version change, on or off.
  if (update::ParseVersion(running)) {
    local->SetString(kPrefLastRun, running);
    local->CommitPendingWrite();  // shown once, even if this run crashes
  }
  LOG(WARNING) << "BrowseCraft what's new: last run v"
               << (last.empty() ? "(none)" : last) << ", now v" << running
               << (profile->IsNewProfile() ? ", new profile" : "")
               << (w.show ? ": showing" : ": not showing");
  if (!w.show) {
    return;
  }
  std::string url = "chrome://browsecraft/whatsnew";
  if (!w.from.empty()) {
    url += "/" + w.from;
  }
  base::SingleThreadTaskRunner::GetCurrentDefault()->PostDelayedTask(
      FROM_HERE, base::BindOnce(&OpenWhenReady, GURL(url), 30),
      base::Seconds(2));
}

std::string Page(const std::string& from) {
  const std::string running = Running();
  // No last-run version (an update from a BrowseCraft without this tab): the
  // notes of the running Chromium major, not every bundled one.
  const std::string major =
      std::string(version_info::GetVersionNumber()).substr(
          0, std::string(version_info::GetVersionNumber()).find('.'));
  std::vector<std::string> versions;
  for (const notes::Note& n : notes::kNotes) {
    if (*n.version &&
        (!from.empty() || base::StartsWith(n.chromium, major + "."))) {
      versions.push_back(n.version);
    }
  }
  std::vector<std::string> show = update::NotesToShow(versions, from, running);
  std::string body;
  for (const std::string& v : show) {
    for (const notes::Note& n : notes::kNotes) {
      if (v == n.version) {
        body += n.html;
        body += "\n";
      }
    }
  }
  const std::string anchor = show.empty() ? std::string() : "#v" + show.front();
  std::string intro =
      show.empty()
          ? "AltBrowse v" + running + "."
      : from.empty()
          ? "AltBrowse v" + running + " and the releases before it on Chromium " +
                major + ", newest first."
          : "AltBrowse was updated from v" + base::EscapeForHTML(from) +
                " to v" + running + "." +
                (show.size() > 1 ? " Every release since is listed, newest first."
                                 : "");
  return std::string(R"(<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>What's new in AltBrowse</title>
<link rel="stylesheet" href="/browsecraft.css">
</head>
<body>
<main class="wide">
  <h1>What's new in AltBrowse</h1>
  <p class="lead">)") +
         intro + R"(</p>
)" + (body.empty() ? "<p class=\"note\">No release notes are bundled with this build.</p>\n"
                    : body) +
         R"(  <p class="foot"><a href=")" + std::string(notes::kChangesUrl) + anchor +
         R"(" target="_blank" rel="noopener">Full change history</a> ·
  <a href="chrome://browsecraft/">Settings for updates and this page</a></p>
</main>
</body>
</html>
)";
}

}  // namespace browsecraft::whats_new
