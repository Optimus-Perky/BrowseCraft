// Copyright 2026 The BrowseCraft Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "chrome/browser/ui/webui/browsecraft/browsecraft_ui.h"

#include <memory>
#include <string>
#include <utility>

#include "base/functional/bind.h"
#include "base/memory/raw_ptr.h"
#include "base/memory/ref_counted_memory.h"
#include "base/memory/weak_ptr.h"
#include "base/values.h"
#include "chrome/browser/browsecraft/browsecraft_account.h"
#include "chrome/browser/profiles/profile.h"
#include "components/prefs/pref_change_registrar.h"
#include "components/prefs/pref_service.h"
#include "components/sync/base/pref_names.h"
#include "content/public/browser/web_ui.h"
#include "content/public/browser/web_ui_data_source.h"
#include "content/public/browser/web_ui_message_handler.h"

namespace {

constexpr char kPage[] = R"(<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>AltBrowse</title>
<link rel="stylesheet" href="browsecraft.css">
<script src="browsecraft.js"></script>
</head>
<body>
<main>
  <h1>AltBrowse</h1>
  <h2>Sync</h2>
  <p class="lead">Sign in to your AltBrowse server and your bookmarks,
  passwords, history, open tabs, extensions and settings follow you to every
  PC you sign in on. No Google or Microsoft account involved.</p>
  <section id="signed-in" hidden>
    <p class="who"><b id="who-name"></b><span id="who-user"></span></p>
    <p>Signed in to <b id="who-server"></b>. Sync is on.</p>
    <div class="buttons">
      <button id="sign-out" class="plain">Sign out</button>
    </div>
  </section>
  <section id="signed-out" hidden>
    <label>Server <input id="server" type="text" autocomplete="url"
      placeholder="https://sync.example.home" required></label>
    <div class="buttons">
      <button id="sign-in-windows" type="button">Sign in with your Windows
      account</button>
    </div>
    <p id="busy" class="note" hidden></p>
    <form id="form">
      <p class="or">Or with a user name and password</p>
      <label>User name <input id="user" type="text" autocomplete="username"
        required></label>
      <label>Password <input id="password" type="password"
        autocomplete="current-password" required></label>
      <div class="buttons"><button id="sign-in" type="submit"
        class="plain">Sign in</button></div>
    </form>
  </section>
  <p id="error" class="error" hidden></p>
  <p class="foot"><a href="chrome://settings/manageProfile">Profile name
  and photo</a></p>
</main>
</body>
</html>
)";

constexpr char kStyle[] = R"(
:root { color-scheme: light dark; --accent: #2f6f5e; --text: #1f2328;
  --dim: #5b636b; --bg: #f6f7f8; --card: #ffffff; --line: #d9dde1; }
@media (prefers-color-scheme: dark) {
  :root { --accent: #5fb39b; --text: #e6e8ea; --dim: #a3aab1; --bg: #1c1f22;
    --card: #25292d; --line: #3a3f44; }
}
body { margin: 0; background: var(--bg); color: var(--text);
  font: 15px/1.5 system-ui, sans-serif; }
main { max-width: 34em; margin: 4em auto; background: var(--card);
  border: 1px solid var(--line); border-radius: 10px; padding: 2em 2.4em; }
h1 { margin: 0 0 .4em; font-size: 1.6em; }
h2 { margin: 1.2em 0 .2em; font-size: 1.15em; }
.lead { color: var(--dim); }
.who { font-size: 1.2em; margin-bottom: .2em; }
#who-user { color: var(--dim); font-size: .85em; margin-left: .5em; }
label { display: block; margin: 1em 0 0; font-weight: 600; }
input { display: block; width: 100%; box-sizing: border-box; margin-top: .3em;
  padding: .55em .7em; border: 1px solid var(--line); border-radius: 6px;
  background: var(--bg); color: var(--text); font: inherit; }
.buttons { margin-top: 1.2em; display: flex; gap: .7em; }
button { padding: .55em 1.3em; border: 0; border-radius: 6px; font: inherit;
  font-weight: 600; background: var(--accent); color: #fff; cursor: pointer; }
button.plain { background: transparent; color: var(--accent);
  border: 1px solid var(--line); }
button:disabled { opacity: .6; cursor: default; }
.or { margin: 2em 0 0; padding-top: 1.2em; border-top: 1px solid var(--line);
  color: var(--dim); }
.error { color: #c0392b; }
.note { color: var(--dim); }
.foot { margin: 2em 0 0; font-size: .9em; }
a { color: var(--accent); }
)";

constexpr char kScript[] = R"(
'use strict';
const $ = (id) => document.getElementById(id);
let first = true;
function busy(text) {
  $('busy').textContent = text || '';
  $('busy').hidden = !text;
  for (const b of ['sign-in', 'sign-in-windows']) $(b).disabled = !!text;
}
function fail(text) {
  $('error').textContent = text;
  $('error').hidden = !text;
}
function show(status) {
  $('signed-in').hidden = !status.signedIn;
  $('signed-out').hidden = status.signedIn;
  if (status.signedIn) {
    fail('');
    $('who-name').textContent = status.displayName;
    $('who-user').textContent =
        status.displayName === status.user ? '' : status.user;
    $('who-server').textContent = status.server;
  } else if (!$('server').value) {
    $('server').value = status.suggestedServer;
  }
  if (first) {
    first = false;
    if (!status.signedIn && location.hash === '#sign-in' &&
        $('server').value) {
      signInWindows();
    }
  }
}
function signInWindows() {
  fail('');
  busy('Signing in with your Windows account…');
  chrome.send('signInWindows', [$('server').value]);
}
window.browsecraft = {
  onStatus(status) { show(status); },
  onSignIn(result) {
    busy('');
    if (result.ok) {
      $('password').value = '';
      history.replaceState(null, '', location.pathname);
    } else {
      fail(result.error);
    }
    show(result.status);
  },
};
document.addEventListener('DOMContentLoaded', () => {
  $('form').addEventListener('submit', (e) => {
    e.preventDefault();
    fail('');
    busy('Signing in…');
    chrome.send('signIn', [$('server').value, $('user').value,
                           $('password').value]);
  });
  $('sign-in-windows').addEventListener('click', signInWindows);
  $('sign-out').addEventListener('click', () => chrome.send('signOut'));
  chrome.send('getStatus');
});
)";

bool ShouldHandle(const std::string& path) {
  return path.empty() || path == "index.html" || path == "browsecraft.js" ||
         path == "browsecraft.css";
}

void Handle(const std::string& path,
            content::WebUIDataSource::GotDataCallback callback) {
  std::string body = path == "browsecraft.js"    ? kScript
                     : path == "browsecraft.css" ? kStyle
                                                 : kPage;
  std::move(callback).Run(
      base::MakeRefCounted<base::RefCountedString>(std::move(body)));
}

class BrowseCraftHandler : public content::WebUIMessageHandler {
 public:
  explicit BrowseCraftHandler(Profile* profile) : profile_(profile) {}
  BrowseCraftHandler(const BrowseCraftHandler&) = delete;
  BrowseCraftHandler& operator=(const BrowseCraftHandler&) = delete;
  ~BrowseCraftHandler() override = default;

  void RegisterMessages() override {
    web_ui()->RegisterMessageCallback(
        "getStatus", base::BindRepeating(&BrowseCraftHandler::GetStatus,
                                         base::Unretained(this)));
    web_ui()->RegisterMessageCallback(
        "signIn", base::BindRepeating(&BrowseCraftHandler::SignIn,
                                      base::Unretained(this)));
    web_ui()->RegisterMessageCallback(
        "signInWindows",
        base::BindRepeating(&BrowseCraftHandler::SignInWindows,
                            base::Unretained(this)));
    web_ui()->RegisterMessageCallback(
        "signOut", base::BindRepeating(&BrowseCraftHandler::SignOut,
                                       base::Unretained(this)));
  }

  void OnJavascriptAllowed() override {
    // Signed in or out elsewhere (start-up sign-in, another tab): show it.
    registrar_.Init(profile_->GetPrefs());
    registrar_.Add(syncer::prefs::kBrowseCraftSyncToken,
                   base::BindRepeating(&BrowseCraftHandler::PushStatus,
                                       base::Unretained(this)));
  }

  void OnJavascriptDisallowed() override { registrar_.RemoveAll(); }

 private:
  base::DictValue Status() {
    base::DictValue d;
    d.Set("signedIn", browsecraft::IsSignedIn(profile_));
    d.Set("server", browsecraft::SignedInServer(profile_));
    d.Set("user", browsecraft::SignedInUser(profile_));
    d.Set("displayName", browsecraft::SignedInDisplayName(profile_));
    d.Set("suggestedServer", browsecraft::SuggestedServer(profile_));
    return d;
  }

  void PushStatus() {
    if (IsJavascriptAllowed()) {
      CallJavascriptFunction("browsecraft.onStatus", base::Value(Status()));
    }
  }

  void GetStatus(const base::ListValue& args) {
    AllowJavascript();
    PushStatus();
  }

  void Reply(bool ok, const std::string& error) {
    base::DictValue result;
    result.Set("ok", ok);
    result.Set("error", error);
    result.Set("status", Status());
    CallJavascriptFunction("browsecraft.onSignIn",
                           base::Value(std::move(result)));
  }

  void SignIn(const base::ListValue& args) {
    AllowJavascript();
    if (args.size() < 3 || !args[0].is_string() || !args[1].is_string() ||
        !args[2].is_string()) {
      return;
    }
    browsecraft::SignInWithPassword(
        profile_, args[0].GetString(), args[1].GetString(),
        args[2].GetString(),
        base::BindOnce(&BrowseCraftHandler::Reply,
                       weak_factory_.GetWeakPtr()));
  }

  void SignInWindows(const base::ListValue& args) {
    AllowJavascript();
    if (args.empty() || !args[0].is_string()) {
      return;
    }
    browsecraft::SignInWithWindowsAccount(
        profile_, args[0].GetString(),
        base::BindOnce(&BrowseCraftHandler::Reply,
                       weak_factory_.GetWeakPtr()));
  }

  void SignOut(const base::ListValue& args) {
    AllowJavascript();
    browsecraft::SignOut(profile_);
    PushStatus();
  }

  raw_ptr<Profile> profile_;
  PrefChangeRegistrar registrar_;
  base::WeakPtrFactory<BrowseCraftHandler> weak_factory_{this};
};

}  // namespace

BrowseCraftUI::BrowseCraftUI(content::WebUI* web_ui)
    : content::WebUIController(web_ui) {
  Profile* profile = Profile::FromWebUI(web_ui);
  content::WebUIDataSource* source =
      content::WebUIDataSource::CreateAndAdd(profile, kChromeUIBrowseCraftHost);
  source->SetRequestFilter(base::BindRepeating(&ShouldHandle),
                           base::BindRepeating(&Handle));
  web_ui->AddMessageHandler(std::make_unique<BrowseCraftHandler>(profile));
}

BrowseCraftUI::~BrowseCraftUI() = default;

WEB_UI_CONTROLLER_TYPE_IMPL(BrowseCraftUI)
