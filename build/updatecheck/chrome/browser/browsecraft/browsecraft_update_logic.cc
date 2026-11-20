// Copyright 2026 The BrowseCraft Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "chrome/browser/browsecraft/browsecraft_update_logic.h"

#include <algorithm>
#include <tuple>

#include "base/json/json_reader.h"
#include "base/strings/string_number_conversions.h"
#include "base/strings/string_split.h"
#include "base/values.h"
#include "url/gurl.h"
#include "url/origin.h"

namespace browsecraft::update {

namespace {

bool AllDigits(std::string_view s) {
  if (s.empty() || s.size() > 9) {
    return false;
  }
  for (char c : s) {
    if (c < '0' || c > '9') {
      return false;
    }
  }
  return true;
}

int ToInt(std::string_view s) {
  int v = 0;
  base::StringToInt(s, &v);
  return v;
}

// Printable text only: a file with control characters is not ours.
bool PlainText(const std::string& s, size_t max) {
  if (s.size() > max) {
    return false;
  }
  for (unsigned char c : s) {
    if (c < 0x20 || c == 0x7f) {
      return false;
    }
  }
  return true;
}

// https anywhere, or the manifest's own origin (a test server, an
// organisation's own copy on its LAN).
bool AcceptableUrl(const std::string& text, const GURL& manifest_url) {
  if (text.size() > 2048) {
    return false;
  }
  GURL url(text);
  if (!url.is_valid()) {
    return false;
  }
  if (url.SchemeIs("https")) {
    return true;
  }
  return url.SchemeIsHTTPOrHTTPS() &&
         url::Origin::Create(url).IsSameOriginWith(manifest_url);
}

bool ChromiumVersion(const std::string& s) {
  std::vector<std::string_view> p =
      base::SplitStringPiece(s, ".", base::KEEP_WHITESPACE, base::SPLIT_WANT_ALL);
  if (p.size() != 4) {
    return false;
  }
  for (std::string_view x : p) {
    if (!AllDigits(x)) {
      return false;
    }
  }
  return true;
}

}  // namespace

std::optional<Version> ParseVersion(std::string_view text) {
  if (!text.empty() && (text[0] == 'v' || text[0] == 'V')) {
    text.remove_prefix(1);
  }
  std::vector<std::string_view> p = base::SplitStringPiece(
      text, ".", base::KEEP_WHITESPACE, base::SPLIT_WANT_ALL);
  Version v;
  if (p.size() == 4) {  // 0.8.261113.2
    for (std::string_view x : p) {
      if (!AllDigits(x)) {
        return std::nullopt;
      }
    }
    if (p[2].size() != 6) {
      return std::nullopt;
    }
    v.major = ToInt(p[0]);
    v.minor = ToInt(p[1]);
    v.day = ToInt(p[2]);
    v.build = ToInt(p[3]);
    return v;
  }
  if (p.size() == 3) {  // 1.0.261120 or 1.0.261120a
    std::string_view day = p[2];
    if (day.size() == 7 && day[6] >= 'a' && day[6] <= 'z') {
      v.letter = day[6] - 'a' + 1;
      day.remove_suffix(1);
    }
    if (!AllDigits(p[0]) || !AllDigits(p[1]) || day.size() != 6 ||
        !AllDigits(day)) {
      return std::nullopt;
    }
    v.major = ToInt(p[0]);
    v.minor = ToInt(p[1]);
    v.day = ToInt(day);
    if (v.major < 1) {  // the new scheme starts at 1 (Chromium 158)
      return std::nullopt;
    }
    return v;
  }
  return std::nullopt;
}

int CompareVersions(std::string_view a, std::string_view b) {
  std::optional<Version> x = ParseVersion(a);
  std::optional<Version> y = ParseVersion(b);
  if (!x || !y) {
    return (x ? 1 : 0) - (y ? 1 : 0);
  }
  auto key = [](const Version& v) {
    return std::tie(v.major, v.minor, v.day, v.build, v.letter);
  };
  if (key(*x) < key(*y)) {
    return -1;
  }
  return key(*y) < key(*x) ? 1 : 0;
}

std::optional<UpdateInfo> ParseManifest(std::string_view json,
                                        const GURL& manifest_url) {
  if (json.size() > kMaxManifestBytes) {
    return std::nullopt;
  }
  std::optional<base::DictValue> root =
      base::JSONReader::ReadDict(json, base::JSON_PARSE_RFC);
  if (!root) {
    return std::nullopt;
  }
  if (root->FindInt("schema") != kSchema) {
    return std::nullopt;
  }
  // "BrowseCraft" is the product id every installed copy checks for (renamed
  // AltBrowse on 20 Nov 2026; the id stays so older copies still see updates).
  // "AltBrowse" is taken too, so the file can switch once no BrowseCraft-era
  // copy is left.
  const std::string* product = root->FindString("product");
  if (!product || (*product != "BrowseCraft" && *product != "AltBrowse")) {
    return std::nullopt;
  }
  UpdateInfo info;
  const std::string* version = root->FindString("version");
  const std::string* chromium = root->FindString("chromium");
  const std::string* page = root->FindString("page");
  if (!version || !ParseVersion(*version) || !chromium ||
      !ChromiumVersion(*chromium) || !page ||
      !AcceptableUrl(*page, manifest_url)) {
    return std::nullopt;
  }
  info.version = (*version)[0] == 'v' ? version->substr(1) : *version;
  info.chromium = *chromium;
  info.page = *page;
  if (const std::string* released = root->FindString("released")) {
    if (!PlainText(*released, 10)) {
      return std::nullopt;
    }
    info.released = *released;
  }

  const base::DictValue* sec = root->FindDict("security");
  if (!sec) {
    return std::nullopt;
  }
  std::optional<bool> update = sec->FindBool("update");
  if (!update) {
    return std::nullopt;
  }
  info.security.update = *update;
  if (const base::Value* fixes = sec->Find("fixes")) {
    if (!fixes->is_int() || fixes->GetInt() < 0 || fixes->GetInt() > 10000) {
      return std::nullopt;
    }
    info.security.fixes = fixes->GetInt();
  }
  info.security.highest = "unknown";
  if (const base::Value* highest = sec->Find("highest")) {
    const std::string* h = highest->GetIfString();
    if (!h || (*h != "critical" && *h != "high" && *h != "medium" &&
               *h != "low" && *h != "unknown")) {
      return std::nullopt;
    }
    info.security.highest = *h;
  }
  if (const base::Value* wild = sec->Find("in_the_wild")) {
    if (!wild->is_bool()) {
      return std::nullopt;
    }
    info.security.in_the_wild = wild->GetBool();
  }
  for (const char* key : {"summary", "source"}) {
    if (const base::Value* v = sec->Find(key)) {
      const std::string* s = v->GetIfString();
      if (!s || !PlainText(*s, key[1] == 'u' ? 400 : 40)) {
        return std::nullopt;
      }
      (key[1] == 'u' ? info.security.summary : info.security.source) = *s;
    }
  }
  if (const base::Value* refs = sec->Find("references")) {
    if (!refs->is_list() || refs->GetList().size() > 20) {
      return std::nullopt;
    }
    for (const base::Value& r : refs->GetList()) {
      if (!r.is_string() || !GURL(r.GetString()).SchemeIs("https") ||
          r.GetString().size() > 2048) {
        return std::nullopt;
      }
      info.security.references.push_back(r.GetString());
    }
  }

  if (const base::Value* downloads = root->Find("downloads")) {
    if (!downloads->is_dict() || downloads->GetDict().size() > 8) {
      return std::nullopt;
    }
    for (const auto [platform, value] : downloads->GetDict()) {
      const base::DictValue* d = value.GetIfDict();
      if (!d || !PlainText(platform, 32)) {
        return std::nullopt;
      }
      const std::string* url = d->FindString("url");
      const std::string* sha = d->FindString("sha256");
      std::optional<double> size = d->FindDouble("size");
      std::vector<uint8_t> bytes;
      if (!url || !AcceptableUrl(*url, manifest_url) || !sha ||
          sha->size() != 64 || !base::HexStringToBytes(*sha, &bytes) ||
          !size || *size < 0 || *size > 1e11) {
        return std::nullopt;
      }
      info.downloads[platform] = {*url, *sha, static_cast<int64_t>(*size)};
    }
  }
  return info;
}

bool IsWanted(std::string_view running,
              std::string_view latest,
              std::string_view skipped) {
  if (!ParseVersion(running) || !ParseVersion(latest) ||
      CompareVersions(latest, running) <= 0) {
    return false;
  }
  return skipped.empty() || !ParseVersion(skipped) ||
         CompareVersions(latest, skipped) > 0;
}

bool ShouldOpenTab(const UpdateInfo& info,
                   bool wanted,
                   bool at_startup,
                   std::string_view today,
                   std::string_view last_tab_day) {
  return info.security.update && wanted && at_startup && today != last_tab_day;
}

WhatsNew DecideWhatsNew(std::string_view last_run,
                        std::string_view running,
                        bool new_profile,
                        bool enabled) {
  WhatsNew w;
  if (!enabled || !ParseVersion(running)) {
    return w;
  }
  if (last_run.empty() || !ParseVersion(last_run)) {
    w.show = !new_profile;
    return w;
  }
  if (CompareVersions(running, last_run) > 0) {
    w.show = true;
    w.from = std::string(last_run);
  }
  return w;
}

std::vector<std::string> NotesToShow(const std::vector<std::string>& versions,
                                     std::string_view from,
                                     std::string_view running) {
  std::vector<std::string> out;
  for (const std::string& v : versions) {
    if (!ParseVersion(v) || CompareVersions(v, running) > 0) {
      continue;
    }
    if (!from.empty() && CompareVersions(v, from) <= 0) {
      continue;
    }
    out.push_back(v);
  }
  std::sort(out.begin(), out.end(), [](const std::string& a, const std::string& b) {
    return CompareVersions(a, b) > 0;
  });
  return out;
}

}  // namespace browsecraft::update
