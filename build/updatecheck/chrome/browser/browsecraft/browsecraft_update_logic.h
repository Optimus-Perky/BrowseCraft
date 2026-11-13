// Copyright 2026 The BrowseCraft Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// BrowseCraft's update check, the parts with no browser in them: versions,
// the published latest.json, and the rules for what to show. Unit-tested in
// browsecraft_update_logic_unittest.cc.

#ifndef CHROME_BROWSER_BROWSECRAFT_BROWSECRAFT_UPDATE_LOGIC_H_
#define CHROME_BROWSER_BROWSECRAFT_BROWSECRAFT_UPDATE_LOGIC_H_

#include <cstdint>
#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "url/gurl.h"

namespace browsecraft::update {

// A BrowseCraft version, either scheme:
//   up to Chromium 157: 0.8.<yymmdd>.<n>        e.g. 0.8.261113.2
//   from Chromium 158:  <major>.<minor>.<yymmdd>[letter]  e.g. 1.0.261120a
// A leading "v" is allowed. Every 0.x version sorts below every 1.x; a letter
// sorts above the same number without one (a fix of ours on that build).
struct Version {
  int major = 0;
  int minor = 0;
  int day = 0;     // yymmdd
  int build = 0;   // the old scheme's n; 0 in the new one
  int letter = 0;  // 0 = none, 1 = a ... 26 = z
};
std::optional<Version> ParseVersion(std::string_view text);
// <0, 0, >0 like strcmp. Both must parse; an unparsable one sorts lowest.
int CompareVersions(std::string_view a, std::string_view b);

struct Download {
  std::string url;
  std::string sha256;
  int64_t size = 0;
};

struct Security {
  bool update = false;      // a security update
  int fixes = 0;            // security fixes in it (0 = not known)
  std::string highest;      // critical | high | medium | low | unknown
  bool in_the_wild = false; // one of them is being exploited
  std::string summary;      // one or two sentences
  std::string source;       // chrome-releases | browsecraft | unavailable
  std::vector<std::string> references;
};

// latest.json, schema 1 (written by autobuild/publish/latest_json.py).
struct UpdateInfo {
  std::string version;   // BrowseCraft, no "v"
  std::string chromium;  // e.g. 157.0.8103.50
  std::string released;  // yyyy-mm-dd
  std::string page;      // the download page
  Security security;
  // Platform ("windows-x64", "linux-x64") -> the installer itself. Not used
  // yet: kept for installing updates automatically later.
  std::map<std::string, Download> downloads;
};

inline constexpr int kSchema = 1;
inline constexpr size_t kMaxManifestBytes = 64 * 1024;

// Parses and checks latest.json. Anything wrong (not JSON, wrong schema or
// product, a bad version, a URL that is not https or the manifest's own
// origin, out-of-range values) gives nullopt: a file that does not check out
// is never shown.
std::optional<UpdateInfo> ParseManifest(std::string_view json,
                                        const GURL& manifest_url);

// The version is newer than the one running, and newer than one the user
// chose to skip (so the next version, a letter release included, shows again).
bool IsWanted(std::string_view running,
              std::string_view latest,
              std::string_view skipped);

// Whether to open the update tab: a wanted security update, found by the check
// that follows a browser start, and the tab not opened yet today.
bool ShouldOpenTab(const UpdateInfo& info,
                   bool wanted,
                   bool at_startup,
                   std::string_view today,
                   std::string_view last_tab_day);

// What's new after an update (13 Nov 2026): at the first start after
// the running version changes. |last_run| is the version recorded at the last
// start ("" if none). No last-run version: a new profile is a fresh install
// (no tab); an existing one ran a BrowseCraft from before this feature, so all
// bundled notes are shown (from = ""). Never on a downgrade or the same version.
struct WhatsNew {
  bool show = false;
  std::string from;  // show releases newer than this; "" = all bundled
};
WhatsNew DecideWhatsNew(std::string_view last_run,
                        std::string_view running,
                        bool new_profile,
                        bool enabled);

// The release-note versions to show: newer than |from| ("" = no lower bound),
// not newer than |running|, newest first.
std::vector<std::string> NotesToShow(const std::vector<std::string>& versions,
                                     std::string_view from,
                                     std::string_view running);

}  // namespace browsecraft::update

#endif  // CHROME_BROWSER_BROWSECRAFT_BROWSECRAFT_UPDATE_LOGIC_H_
