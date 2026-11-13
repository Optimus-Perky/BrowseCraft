// Copyright 2026 The BrowseCraft Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// "What's new" after an update (13 Nov 2026): at the first start after
// the running BrowseCraft version changes, a tab with what is new, what was
// fixed and the security fixes applied, for every release between the last-run
// version and this one. The notes are bundled into the build
// (browsecraft_release_notes.h, from release-notes/ by build/release_notes.py);
// the tab fetches nothing. Never on a fresh install or a downgrade. Off with
// Local State browsecraft.whats_new.enabled (chrome://browsecraft).

#ifndef CHROME_BROWSER_BROWSECRAFT_BROWSECRAFT_WHATS_NEW_H_
#define CHROME_BROWSER_BROWSECRAFT_BROWSECRAFT_WHATS_NEW_H_

#include <string>

class PrefRegistrySimple;
class Profile;

namespace browsecraft::whats_new {

inline constexpr char kPrefEnabled[] = "browsecraft.whats_new.enabled";
inline constexpr char kPrefLastRun[] = "browsecraft.whats_new.last_run_version";

void RegisterLocalStatePrefs(PrefRegistrySimple* registry);

// From ChromeBrowserMainParts::PostProfileInit for the initial profile.
void OnInitialProfile(Profile* profile);

bool Enabled();
void SetEnabled(bool enabled);

// The page for chrome://browsecraft/whatsnew[/<from version>]: releases newer
// than |from| ("" = every bundled one) up to the running version.
std::string Page(const std::string& from);

}  // namespace browsecraft::whats_new

#endif  // CHROME_BROWSER_BROWSECRAFT_BROWSECRAFT_WHATS_NEW_H_
