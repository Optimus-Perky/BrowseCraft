// Copyright 2026 The BrowseCraft Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "chrome/browser/browsecraft/browsecraft_update_logic.h"

#include <string>

#include "testing/gtest/include/gtest/gtest.h"
#include "url/gurl.h"

namespace browsecraft::update {
namespace {

const GURL kManifestUrl("https://altcraft.example/browsecraft/latest.json");

std::string Manifest(const std::string& version = "0.8.261113.2",
                     const std::string& security = R"({"update": true,
      "fixes": 9, "highest": "high", "in_the_wild": true,
      "summary": "9 security fixes, one exploited.",
      "source": "chrome-releases",
      "references": ["https://chromereleases.googleblog.com/x.html"]})",
                     const std::string& extra = "") {
  return R"({"schema": 1, "product": "BrowseCraft", "version": ")" + version +
         R"(", "chromium": "157.0.8103.50", "released": "2026-11-13",
      "page": "https://altcraft.example/browsecraft/",
      "downloads": {"windows-x64": {"url":
        "https://altcraft.example/downloads/BrowseCraft-0.8.261113.2-x64.exe",
        "sha256": "149893bb11f1a942a3421fb88b03f5023a747ca9d20c621f05b855677c68e697",
        "size": 159744527}},
      "security": )" +
         security + extra + "}";
}

TEST(BrowseCraftUpdateLogic, ParsesBothSchemes) {
  EXPECT_TRUE(ParseVersion("0.8.261113.2"));
  EXPECT_TRUE(ParseVersion("v0.8.261113.2"));
  EXPECT_TRUE(ParseVersion("1.0.261120"));
  EXPECT_TRUE(ParseVersion("1.0.261120a"));
  EXPECT_TRUE(ParseVersion("12.3.270101z"));
  // Controls: not versions.
  EXPECT_FALSE(ParseVersion(""));
  EXPECT_FALSE(ParseVersion("0.8.26111.2"));    // five-digit day
  EXPECT_FALSE(ParseVersion("1.0.261120A"));    // capital letter
  EXPECT_FALSE(ParseVersion("1.0.261120ab"));   // two letters
  EXPECT_FALSE(ParseVersion("0.0.261120"));     // new scheme starts at 1
  EXPECT_FALSE(ParseVersion("157.0.8103.50.1"));
  EXPECT_FALSE(ParseVersion("1.x.261120"));
}

TEST(BrowseCraftUpdateLogic, Orders) {
  EXPECT_LT(CompareVersions("0.8.261113.1", "0.8.261113.2"), 0);
  EXPECT_LT(CompareVersions("0.8.261113.9", "0.8.261114.1"), 0);
  // Every 0.8 version is older than every 1.x.
  EXPECT_LT(CompareVersions("0.8.291231.9", "1.0.261120"), 0);
  EXPECT_LT(CompareVersions("1.0.261120", "1.0.261120a"), 0);  // letter above
  EXPECT_LT(CompareVersions("1.0.261120a", "1.0.261120b"), 0);
  EXPECT_LT(CompareVersions("1.0.261120z", "1.1.261127"), 0);
  EXPECT_LT(CompareVersions("1.9.270101", "2.0.270102"), 0);
  EXPECT_EQ(CompareVersions("v1.0.261120a", "1.0.261120a"), 0);
  EXPECT_GT(CompareVersions("1.0.261120a", "1.0.261120"), 0);
  // An unparsable version sorts lowest; two unparsable ones are equal.
  EXPECT_LT(CompareVersions("rubbish", "0.8.261113.1"), 0);
  EXPECT_EQ(CompareVersions("rubbish", "junk"), 0);
}

TEST(BrowseCraftUpdateLogic, ReadsAGoodManifest) {
  std::optional<UpdateInfo> info = ParseManifest(Manifest(), kManifestUrl);
  ASSERT_TRUE(info);
  EXPECT_EQ(info->version, "0.8.261113.2");
  EXPECT_EQ(info->chromium, "157.0.8103.50");
  EXPECT_TRUE(info->security.update);
  EXPECT_EQ(info->security.fixes, 9);
  EXPECT_EQ(info->security.highest, "high");
  EXPECT_TRUE(info->security.in_the_wild);
  EXPECT_EQ(info->security.source, "chrome-releases");
  ASSERT_EQ(info->downloads.count("windows-x64"), 1u);
  EXPECT_EQ(info->downloads["windows-x64"].size, 159744527);
  // A normal (non-security) update is read too.
  info = ParseManifest(Manifest("1.0.261120", R"({"update": false})"),
                       kManifestUrl);
  ASSERT_TRUE(info);
  EXPECT_FALSE(info->security.update);
  EXPECT_EQ(info->security.highest, "unknown");
}

TEST(BrowseCraftUpdateLogic, RefusesBadManifests) {
  const GURL& u = kManifestUrl;
  EXPECT_FALSE(ParseManifest("", u));
  EXPECT_FALSE(ParseManifest("not json", u));
  EXPECT_FALSE(ParseManifest("[1, 2]", u));
  EXPECT_FALSE(ParseManifest(Manifest().substr(0, 80), u));  // cut short
  std::string m = Manifest();
  auto with = [&](const std::string& from, const std::string& to) {
    std::string s = m;
    size_t at = s.find(from);
    EXPECT_NE(at, std::string::npos) << from;
    return s.replace(at, from.size(), to);
  };
  EXPECT_FALSE(ParseManifest(with("\"schema\": 1", "\"schema\": 2"), u));
  EXPECT_FALSE(ParseManifest(with("\"BrowseCraft\"", "\"Chrome\""), u));
  EXPECT_FALSE(ParseManifest(Manifest("0.8.bad"), u));
  EXPECT_FALSE(ParseManifest(with("157.0.8103.50", "157.0"), u));
  EXPECT_FALSE(ParseManifest(
      with("https://altcraft.example/browsecraft/", "http://evil.example/"), u));
  EXPECT_FALSE(ParseManifest(with("\"fixes\": 9", "\"fixes\": \"9\""), u));
  EXPECT_FALSE(ParseManifest(with("\"fixes\": 9", "\"fixes\": -1"), u));
  EXPECT_FALSE(ParseManifest(with("\"high\"", "\"severe\""), u));
  EXPECT_FALSE(ParseManifest(with("\"in_the_wild\": true", "\"in_the_wild\": 1"), u));
  EXPECT_FALSE(ParseManifest(with("\"update\": true", "\"update\": \"yes\""), u));
  EXPECT_FALSE(ParseManifest(Manifest("0.8.261113.2", R"({"fixes": 9})"), u));
  EXPECT_FALSE(ParseManifest(with("one exploited.", "one\nexploited."), u));
  EXPECT_FALSE(ParseManifest(with("149893bb", "zz9893bb"), u));
  EXPECT_FALSE(ParseManifest(std::string(kMaxManifestBytes + 1, ' '), u));
  // Control: the same text untouched is fine.
  EXPECT_TRUE(ParseManifest(with("\"schema\": 1", "\"schema\": 1"), u));
}

TEST(BrowseCraftUpdateLogic, HttpOnlyFromTheManifestsOwnServer) {
  const GURL local("http://192.0.2.10:8299/latest.json");
  std::string m = Manifest();
  auto swap = [&](const std::string& from, const std::string& to) {
    std::string s = m;
    size_t at;
    while ((at = s.find(from)) != std::string::npos) {
      s.replace(at, from.size(), to);
    }
    return s;
  };
  EXPECT_TRUE(ParseManifest(
      swap("https://altcraft.example", "http://192.0.2.10:8299"), local));
  EXPECT_FALSE(ParseManifest(
      swap("https://altcraft.example", "http://192.0.2.11:8299"), local));
}

TEST(BrowseCraftUpdateLogic, WantedAndSkipped) {
  // Newer than running: wanted. Control: same or older is not.
  EXPECT_TRUE(IsWanted("0.8.261113.1", "0.8.261113.2", ""));
  EXPECT_FALSE(IsWanted("0.8.261113.2", "0.8.261113.2", ""));
  EXPECT_FALSE(IsWanted("1.0.261120", "0.8.261113.2", ""));
  // Skipped: that version is quiet...
  EXPECT_FALSE(IsWanted("0.8.261113.1", "0.8.261113.2", "0.8.261113.2"));
  // ...the next one shows again, a letter release on the same number too.
  EXPECT_TRUE(IsWanted("0.8.261113.1", "0.8.261114.1", "0.8.261113.2"));
  EXPECT_FALSE(IsWanted("0.8.261113.1", "1.0.261120", "1.0.261120"));
  EXPECT_TRUE(IsWanted("0.8.261113.1", "1.0.261120a", "1.0.261120"));
  // An older skip does not hide a newer version.
  EXPECT_TRUE(IsWanted("0.8.261113.1", "1.0.261120", "0.8.261113.2"));
  // Rubbish never makes anything wanted.
  EXPECT_FALSE(IsWanted("0.8.261113.1", "junk", ""));
  EXPECT_FALSE(IsWanted("junk", "1.0.261120", ""));
}

TEST(BrowseCraftUpdateLogic, TabOncePerDayAtStartup) {
  UpdateInfo security = *ParseManifest(Manifest(), kManifestUrl);
  UpdateInfo normal =
      *ParseManifest(Manifest("1.0.261120", R"({"update": false})"),
                     kManifestUrl);
  EXPECT_TRUE(ShouldOpenTab(security, true, true, "2026-11-13", ""));
  EXPECT_TRUE(ShouldOpenTab(security, true, true, "2026-11-14", "2026-11-13"));
  // Controls: already today; mid-session; skipped or not newer; not security.
  EXPECT_FALSE(ShouldOpenTab(security, true, true, "2026-11-13", "2026-11-13"));
  EXPECT_FALSE(ShouldOpenTab(security, true, false, "2026-11-14", "2026-11-13"));
  EXPECT_FALSE(ShouldOpenTab(security, false, true, "2026-11-14", "2026-11-13"));
  EXPECT_FALSE(ShouldOpenTab(normal, true, true, "2026-11-14", "2026-11-13"));
}

TEST(BrowseCraftUpdateLogic, WhatsNewAfterAnUpdateOnly) {
  // Upgrade with a recorded last-run version: shown, from that version.
  WhatsNew w = DecideWhatsNew("0.8.261113.1", "0.8.261113.2", false, true);
  EXPECT_TRUE(w.show);
  EXPECT_EQ(w.from, "0.8.261113.1");
  EXPECT_TRUE(DecideWhatsNew("0.8.261113.2", "1.0.261120", false, true).show);
  // Upgrade from a build without the feature (existing profile): all notes.
  w = DecideWhatsNew("", "0.8.261113.2", false, true);
  EXPECT_TRUE(w.show);
  EXPECT_EQ(w.from, "");
  // Controls: fresh install; same version (second start); downgrade; off.
  EXPECT_FALSE(DecideWhatsNew("", "0.8.261113.2", true, true).show);
  EXPECT_FALSE(DecideWhatsNew("0.8.261113.2", "0.8.261113.2", false, true).show);
  EXPECT_FALSE(DecideWhatsNew("1.0.261120", "0.8.261113.2", false, true).show);
  EXPECT_FALSE(DecideWhatsNew("0.8.261113.1", "0.8.261113.2", false, false).show);
}

TEST(BrowseCraftUpdateLogic, NotesBetweenNewestFirst) {
  const std::vector<std::string> all = {"0.8.261113.1", "0.8.261113.2",
                                        "1.0.261120", "1.0.261120a",
                                        "1.1.261201", "bad"};
  EXPECT_EQ(NotesToShow(all, "0.8.261113.1", "1.0.261120a"),
            (std::vector<std::string>{"1.0.261120a", "1.0.261120",
                                      "0.8.261113.2"}));
  EXPECT_EQ(NotesToShow(all, "", "0.8.261113.2"),
            (std::vector<std::string>{"0.8.261113.2", "0.8.261113.1"}));
  // Control: nothing between a version and itself.
  EXPECT_TRUE(NotesToShow(all, "1.1.261201", "1.1.261201").empty());
}

}  // namespace
}  // namespace browsecraft::update
