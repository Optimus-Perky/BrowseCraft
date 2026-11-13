// Copyright 2026 The BrowseCraft Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "chrome/browser/ui/webui/browsecraft/browsecraft_update_page.h"

#include <utility>

#include "base/functional/bind.h"
#include "base/strings/string_util.h"
#include "base/values.h"
#include "chrome/browser/browsecraft/browsecraft_release_notes.h"
#include "chrome/browser/browsecraft/browsecraft_update_checker.h"
#include "chrome/browser/browsecraft/browsecraft_update_logic.h"
#include "chrome/browser/browsecraft/browsecraft_whats_new.h"
#include "content/public/browser/web_ui.h"
#include "content/public/browser/web_ui_message_handler.h"

namespace browsecraft::update_page {

const char kSection[] = R"(
  <section id="updates" class="updates">
    <h2>Updates</h2>
    <p id="update-line" class="note"></p>
    <label class="check"><input type="checkbox" id="update-enabled">
      Check for a new BrowseCraft once a day</label>
    <p class="hint">A plain request for a small public file: no cookies, nothing
    about you or this PC. Security updates open a tab; other updates only show
    in the menu and on the About page. Nothing is downloaded or installed.</p>
    <p id="update-managed" class="note" hidden>Turned off by your
    organisation.</p>
    <label class="check"><input type="checkbox" id="whatsnew-enabled">
      Show what's new after an update</label>
    <p class="foot"><a href="chrome://browsecraft/whatsnew">What's new in this
    version</a> · <a id="update-details" href="chrome://browsecraft/update">Update
    details</a></p>
  </section>
)";

const char kStyle[] = R"(
label.check { font-weight: 600; display: flex; align-items: center;
  gap: .6em; margin: 1.1em 0 0; }
label.check input { display: inline; width: auto; margin: 0; }
.hint { color: var(--dim); font-size: .9em; margin: .2em 0 0 1.9em; }
.updates { margin-top: 2em; padding-top: 1em; border-top: 1px solid var(--line); }
.updates h2 { margin: 0; font-size: 1.15em; }
main.wide { max-width: 50em; }
.banner { background: #fdecea; color: #8c1d18; border: 1px solid #f1b4ae;
  border-radius: 8px; padding: .9em 1.1em; font-weight: 600; margin: 0 0 1em; }
.banner.wild { background: #8c1d18; color: #fff; border-color: #8c1d18;
  font-size: 1.05em; }
.facts { margin: .6em 0; padding-left: 1.2em; }
a.button { display: inline-block; padding: .55em 1.3em; border-radius: 6px;
  font-weight: 600; background: var(--accent); color: #fff;
  text-decoration: none; }
.skip { margin-top: 1.6em; padding-top: 1em; border-top: 1px solid var(--line); }
.release { border-top: 1px solid var(--line); padding-top: .4em; margin-top: 1.4em; }
.release h2 { margin: .4em 0 .1em; font-size: 1.25em; }
.release h3 { font-size: 1em; margin: 1em 0 .2em; }
.release .meta { color: var(--dim); margin: 0; }
.release ul.cves { font-size: .92em; }
.release summary { cursor: pointer; }
.release .sev { display: inline-block; min-width: 4.6em; font-weight: 600;
  font-size: .85em; }
.release .sev.critical, .release .wild { color: #b3261e; }
.release .sev.high { color: #c4561a; }
.release .sev.medium { color: #8a6d00; }
.release .more { color: var(--dim); list-style: none; }
@media (prefers-color-scheme: dark) {
  .banner { background: #3b1513; color: #ffb4ab; border-color: #8c1d18; }
  .release .sev.critical, .release .wild { color: #ff8a80; }
  .release .sev.high { color: #ffab70; }
  .release .sev.medium { color: #e6c35c; }
}
)";

namespace {

constexpr char kUpdatePage[] = R"(<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>BrowseCraft update</title>
<link rel="stylesheet" href="/browsecraft.css">
<script src="/update.js"></script>
</head>
<body>
<main class="wide">
  <h1>BrowseCraft update</h1>
  <div id="update"><p class="note">Looking…</p></div>
</main>
</body>
</html>
)";

// Builds everything with textContent: nothing from the downloaded file is
// ever treated as HTML.
constexpr char kScript[] = R"(
'use strict';
(() => {
const $ = (id) => document.getElementById(id);
function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}
function link(text, href, external) {
  const a = el('a', '', text);
  a.href = href;
  if (external) { a.target = '_blank'; a.rel = 'noopener'; }
  return a;
}
const SEV = {critical: 'Critical', high: 'High', medium: 'Medium', low: 'Low'};
function settings(s) {
  if (!$('update-enabled')) return;
  const u = s.update;
  $('update-enabled').checked = u.setting;
  $('update-enabled').disabled = u.managedOff;
  $('update-managed').hidden = !u.managedOff;
  $('whatsnew-enabled').checked = s.whatsNew;
  const line = $('update-line');
  line.textContent = '';
  if (!u.enabled) {
    line.textContent = u.managedOff ?
        'Update checks are turned off by your organisation.' :
        'Update checks are off.';
  } else if (u.wanted) {
    line.append((u.latest.security.update ? 'Security update available: ' :
                 'Update available: ') + 'BrowseCraft v' + u.latest.version + '. ',
                link('Details', 'chrome://browsecraft/update'));
  } else {
    line.textContent = 'BrowseCraft v' + u.running + (u.lastCheck ?
        ': up to date when last checked (' + u.lastCheck + ').' :
        ': not checked yet.');
  }
}
function update(s) {
  const root = $('update');
  if (!root) return;
  const u = s.update;
  root.textContent = '';
  if (!u.latest || !u.newer) {
    root.append(el('p', 'lead', 'BrowseCraft v' + u.running + ' is the newest version' +
        (u.lastCheck ? ' (checked ' + u.lastCheck + ').' : '.')));
    root.append(el('p', 'foot'));
    root.lastChild.append(link('BrowseCraft settings', 'chrome://browsecraft/'));
    return;
  }
  const l = u.latest, sec = l.security;
  if (sec.update) {
    root.append(el('div', sec.inTheWild ? 'banner wild' : 'banner',
        sec.inTheWild ?
            'Security update. One of the flaws it fixes is already being used ' +
            'in attacks: update BrowseCraft now.' :
            'Security update. Please update BrowseCraft soon.'));
  }
  root.append(el('p', 'lead', 'BrowseCraft v' + l.version + ' (Chromium ' + l.chromium +
      ') is available' + (l.released ? ', released ' + l.released : '') +
      '. You have v' + u.running + '.'));
  if (sec.update) {
    const facts = el('ul', 'facts');
    if (sec.fixes > 0) {
      facts.append(el('li', '', sec.fixes + ' security fix' + (sec.fixes === 1 ? '' : 'es') +
          (SEV[sec.highest] ? ', the most serious rated ' + SEV[sec.highest] : '') + '.'));
    }
    if (sec.inTheWild) {
      facts.append(el('li', 'wild', 'At least one is being exploited in the wild.'));
    }
    if (sec.summary) facts.append(el('li', '', sec.summary));
    const src = el('li', 'note');
    if (sec.source === 'chrome-releases') {
      src.append('From Google\'s Chrome Releases notes');
      for (const r of sec.references) src.append(' ', link('[notes]', r, true));
      src.append('.');
    } else if (sec.source === 'browsecraft') {
      src.textContent = 'A security fix in BrowseCraft itself.';
    } else {
      src.textContent = 'Security details unavailable: treat it as a security update.';
    }
    facts.append(src);
    root.append(facts);
  } else {
    root.append(el('p', 'note', 'Not a security update.'));
  }
  const dl = el('p');
  dl.append(link('Download BrowseCraft v' + l.version, l.page, true));
  dl.firstChild.className = 'button';
  root.append(dl);
  root.append(el('p', 'note', 'BrowseCraft never downloads or installs updates by itself.'));
  const skip = el('div', 'skip');
  const label = el('label', 'check');
  const box = el('input');
  box.type = 'checkbox';
  box.id = 'skip';
  box.checked = u.skipped;
  box.addEventListener('change', () => chrome.send('setUpdateSkipped', [box.checked]));
  label.append(box, 'Skip this version');
  skip.append(label, el('p', 'hint', 'Ticked, BrowseCraft will not tell you about v' +
      l.version + ' again: no tab, no menu item, nothing on the About page. You will ' +
      'still be told about the next version.'));
  root.append(skip);
  const foot = el('p', 'foot');
  foot.append(link('Full change history', s.changes + '#v' + l.version, true), ' · ',
              link('BrowseCraft settings', 'chrome://browsecraft/'));
  root.append(foot);
}
window.browsecraftUpdate = {
  onState(s) { settings(s); update(s); },
};
document.addEventListener('DOMContentLoaded', () => {
  if ($('update-enabled')) {
    $('update-enabled').addEventListener('change',
        (e) => chrome.send('setUpdateEnabled', [e.target.checked]));
    $('whatsnew-enabled').addEventListener('change',
        (e) => chrome.send('setWhatsNewEnabled', [e.target.checked]));
  }
  chrome.send('getUpdateState');
});
})();
)";

class UpdateHandler : public content::WebUIMessageHandler {
 public:
  void RegisterMessages() override {
    auto add = [this](const char* name, void (UpdateHandler::*fn)(const base::ListValue&)) {
      web_ui()->RegisterMessageCallback(
          name, base::BindRepeating(fn, base::Unretained(this)));
    };
    add("getUpdateState", &UpdateHandler::GetState);
    add("setUpdateEnabled", &UpdateHandler::SetEnabled);
    add("setWhatsNewEnabled", &UpdateHandler::SetWhatsNew);
    add("setUpdateSkipped", &UpdateHandler::SetSkipped);
  }

 private:
  void Push() {
    AllowJavascript();
    base::DictValue s;
    s.Set("update", update::UpdateChecker::Get()->State());
    s.Set("whatsNew", whats_new::Enabled());
    s.Set("changes", notes::kChangesUrl);
    CallJavascriptFunction("browsecraftUpdate.onState", base::Value(std::move(s)));
  }
  void GetState(const base::ListValue&) { Push(); }
  void SetEnabled(const base::ListValue& args) {
    if (!args.empty() && args[0].is_bool()) {
      update::UpdateChecker::Get()->SetEnabled(args[0].GetBool());
    }
    Push();
  }
  void SetWhatsNew(const base::ListValue& args) {
    if (!args.empty() && args[0].is_bool()) {
      whats_new::SetEnabled(args[0].GetBool());
    }
    Push();
  }
  void SetSkipped(const base::ListValue& args) {
    if (!args.empty() && args[0].is_bool()) {
      update::UpdateChecker::Get()->SetSkipped(args[0].GetBool());
    }
    Push();
  }
};

}  // namespace

bool Handles(const std::string& path) {
  return path == "update" || path == "update.js" || path == "whatsnew" ||
         base::StartsWith(path, "whatsnew/");
}

std::string Serve(const std::string& path) {
  if (path == "update.js") {
    return kScript;
  }
  if (path == "update") {
    return kUpdatePage;
  }
  std::string from;
  if (base::StartsWith(path, "whatsnew/")) {
    from = path.substr(9);
    if (!update::ParseVersion(from)) {
      from.clear();
    }
  }
  return whats_new::Page(from);
}

std::unique_ptr<content::WebUIMessageHandler> CreateHandler() {
  return std::make_unique<UpdateHandler>();
}

}  // namespace browsecraft::update_page
