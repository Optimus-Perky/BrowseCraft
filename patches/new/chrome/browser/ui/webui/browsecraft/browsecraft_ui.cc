// Copyright 2026 The BrowseCraft Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "chrome/browser/ui/webui/browsecraft/browsecraft_ui.h"

#include <memory>
#include <string>
#include <utility>

#include "base/functional/bind.h"
#include "base/json/json_reader.h"
#include "base/json/json_writer.h"
#include "base/memory/raw_ptr.h"
#include "base/memory/ref_counted_memory.h"
#include "base/memory/weak_ptr.h"
#include "base/strings/string_number_conversions.h"
#include "base/strings/string_util.h"
#include "base/values.h"
#include "chrome/browser/lifetime/application_lifetime.h"
#include "chrome/browser/profiles/profile.h"
#include "components/prefs/pref_service.h"
#include "components/sync/base/pref_names.h"
#include "content/public/browser/storage_partition.h"
#include "content/public/browser/web_ui.h"
#include "content/public/browser/web_ui_data_source.h"
#include "content/public/browser/web_ui_message_handler.h"
#include "net/base/net_errors.h"
#include "net/base/network_interfaces.h"
#include "net/http/http_response_headers.h"
#include "net/traffic_annotation/network_traffic_annotation.h"
#include "services/network/public/cpp/resource_request.h"
#include "services/network/public/cpp/shared_url_loader_factory.h"
#include "services/network/public/cpp/simple_url_loader.h"
#include "services/network/public/mojom/url_response_head.mojom.h"
#include "url/gurl.h"

namespace {

constexpr char kPage[] = R"(<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>BrowseCraft sync</title>
<link rel="stylesheet" href="browsecraft.css">
<script src="browsecraft.js"></script>
</head>
<body>
<main>
  <h1>BrowseCraft sync</h1>
  <p class="lead">Sign in to your own BrowseCraft server and your bookmarks,
  passwords, history, open tabs, extensions and settings follow you to every
  PC you sign in on. No Google or Microsoft account involved.</p>
  <section id="signed-in" hidden>
    <p>Signed in to <b id="who-server"></b> as <b id="who-user"></b>.</p>
    <p id="restart-note" class="note" hidden>Restart BrowseCraft to start
    syncing.</p>
    <div class="buttons">
      <button id="restart" hidden>Restart now</button>
      <button id="sign-out" class="plain">Sign out</button>
    </div>
  </section>
  <form id="form" hidden>
    <label>Server <input id="server" type="text" autocomplete="url"
      placeholder="https://sync.example.home" required></label>
    <label>User name <input id="user" type="text" autocomplete="username"
      required></label>
    <label>Password <input id="password" type="password"
      autocomplete="current-password" required></label>
    <div class="buttons"><button id="sign-in" type="submit">Sign in</button></div>
  </form>
  <p id="error" class="error" hidden></p>
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
.lead { color: var(--dim); }
label { display: block; margin: 1em 0 0; font-weight: 600; }
input { display: block; width: 100%; box-sizing: border-box; margin-top: .3em;
  padding: .55em .7em; border: 1px solid var(--line); border-radius: 6px;
  background: var(--bg); color: var(--text); font: inherit; }
.buttons { margin-top: 1.4em; display: flex; gap: .7em; }
button { padding: .55em 1.3em; border: 0; border-radius: 6px; font: inherit;
  font-weight: 600; background: var(--accent); color: #fff; cursor: pointer; }
button.plain { background: transparent; color: var(--accent);
  border: 1px solid var(--line); }
button:disabled { opacity: .6; cursor: default; }
.error { color: #c0392b; }
.note { color: var(--dim); }
)";

constexpr char kScript[] = R"(
'use strict';
const $ = (id) => document.getElementById(id);
function show(status) {
  $('error').hidden = true;
  $('signed-in').hidden = !status.signedIn;
  $('form').hidden = status.signedIn;
  if (status.signedIn) {
    $('who-server').textContent = status.server;
    $('who-user').textContent = status.user;
  }
  $('restart-note').hidden = !status.needsRestart;
  $('restart').hidden = !status.needsRestart;
}
window.browsecraft = {
  onStatus(status) { show(status); },
  onSignIn(result) {
    $('sign-in').disabled = false;
    if (result.ok) {
      $('password').value = '';
      show(result.status);
    } else {
      $('error').textContent = result.error;
      $('error').hidden = false;
    }
  },
};
document.addEventListener('DOMContentLoaded', () => {
  $('form').addEventListener('submit', (e) => {
    e.preventDefault();
    $('sign-in').disabled = true;
    $('error').hidden = true;
    chrome.send('signIn', [$('server').value, $('user').value,
                           $('password').value]);
  });
  $('sign-out').addEventListener('click', () => chrome.send('signOut'));
  $('restart').addEventListener('click', () => chrome.send('restart'));
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

constexpr net::NetworkTrafficAnnotationTag kTrafficAnnotation =
    net::DefineNetworkTrafficAnnotation("browsecraft_sign_in", R"(
        semantics {
          sender: "BrowseCraft sync"
          description:
            "Signs in to the BrowseCraft sync server the user typed in."
          trigger: "The user signs in or out on chrome://browsecraft."
          data: "The user name and password the user typed."
          destination: OTHER
          destination_other: "The user's own BrowseCraft sync server."
        }
        policy {
          cookies_allowed: NO
          setting: "Only when the user signs in on chrome://browsecraft."
          policy_exception_justification: "User-initiated."
        })");

// "sync.example.home" -> "https://sync.example.home"; no trailing slash.
std::string NormaliseServer(std::string server) {
  base::TrimWhitespaceASCII(server, base::TRIM_ALL, &server);
  while (!server.empty() && server.back() == '/') {
    server.pop_back();
  }
  if (server.find("://") == std::string::npos) {
    server = "https://" + server;
  }
  return server;
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
        "signOut", base::BindRepeating(&BrowseCraftHandler::SignOut,
                                       base::Unretained(this)));
    web_ui()->RegisterMessageCallback(
        "restart", base::BindRepeating(&BrowseCraftHandler::Restart,
                                       base::Unretained(this)));
  }

 private:
  PrefService* prefs() { return profile_->GetPrefs(); }

  base::DictValue Status() {
    std::string server =
        prefs()->GetString(syncer::prefs::kBrowseCraftSyncServer);
    const std::string token =
        prefs()->GetString(syncer::prefs::kBrowseCraftSyncToken);
    const bool signed_in = !server.empty() && !token.empty();
    if (base::EndsWith(server, "/sync")) {
      server.resize(server.size() - 5);
    }
    base::DictValue d;
    d.Set("signedIn", signed_in);
    d.Set("server", server);
    d.Set("user", prefs()->GetString(syncer::prefs::kBrowseCraftSyncUser));
    // Sync picks up a change of server when BrowseCraft starts.
    d.Set("needsRestart", signed_in != started_signed_in_);
    return d;
  }

  void GetStatus(const base::ListValue& args) {
    AllowJavascript();
    if (!checked_start_) {
      checked_start_ = true;
      started_signed_in_ =
          !prefs()->GetString(syncer::prefs::kBrowseCraftSyncToken).empty();
    }
    CallJavascriptFunction("browsecraft.onStatus", base::Value(Status()));
  }

  void SignIn(const base::ListValue& args) {
    AllowJavascript();
    if (args.size() < 3 || !args[0].is_string() || !args[1].is_string() ||
        !args[2].is_string()) {
      return;
    }
    server_ = NormaliseServer(args[0].GetString());
    user_ = args[1].GetString();
    const GURL url(server_ + "/api/sign-in");
    if (!url.is_valid() || !url.SchemeIsHTTPOrHTTPS()) {
      Fail("That server address doesn't look right.");
      return;
    }
    base::DictValue body;
    body.Set("user", user_);
    body.Set("password", args[2].GetString());
    body.Set("device", net::GetHostName());
    std::string json;
    base::JSONWriter::Write(body, &json);

    auto request = std::make_unique<network::ResourceRequest>();
    request->url = url;
    request->method = "POST";
    request->credentials_mode = network::mojom::CredentialsMode::kOmit;
    loader_ = network::SimpleURLLoader::Create(std::move(request),
                                               kTrafficAnnotation);
    loader_->AttachStringForUpload(json, "application/json");
    loader_->SetAllowHttpErrorResults(true);
    loader_->DownloadToString(
        profile_->GetDefaultStoragePartition()
            ->GetURLLoaderFactoryForBrowserProcess()
            .get(),
        base::BindOnce(&BrowseCraftHandler::OnSignIn,
                       weak_factory_.GetWeakPtr()),
        1 << 20);
  }

  void OnSignIn(std::optional<std::string> body) {
    const int code = loader_ && loader_->ResponseInfo() &&
                             loader_->ResponseInfo()->headers
                         ? loader_->ResponseInfo()->headers->response_code()
                         : 0;
    const int net_error = loader_ ? loader_->NetError() : 0;
    loader_.reset();
    std::optional<base::DictValue> reply =
        body ? base::JSONReader::ReadDict(*body,
                                          base::JSON_PARSE_CHROMIUM_EXTENSIONS)
             : std::nullopt;
    if (code == 200 && reply) {
      const std::string* token = reply->FindString("token");
      if (token && !token->empty()) {
        prefs()->SetString(syncer::prefs::kBrowseCraftSyncServer,
                           server_ + "/sync");
        prefs()->SetString(syncer::prefs::kBrowseCraftSyncToken, *token);
        prefs()->SetString(syncer::prefs::kBrowseCraftSyncUser, user_);
        prefs()->SetBoolean(syncer::prefs::kEnableLocalSyncBackend, true);
        base::DictValue result;
        result.Set("ok", true);
        result.Set("status", Status());
        CallJavascriptFunction("browsecraft.onSignIn", base::Value(std::move(result)));
        return;
      }
    }
    if (reply && reply->FindString("error")) {
      Fail(*reply->FindString("error"));
    } else if (code == 0) {
      Fail("Couldn't reach that server (" + net::ErrorToShortString(net_error) +
           ").");
    } else {
      Fail("That isn't a BrowseCraft server (it answered " +
           base::NumberToString(code) + ").");
    }
  }

  void Fail(const std::string& error) {
    base::DictValue result;
    result.Set("ok", false);
    result.Set("error", error);
    CallJavascriptFunction("browsecraft.onSignIn", base::Value(std::move(result)));
  }

  void SignOut(const base::ListValue& args) {
    AllowJavascript();
    // Tell the server the token is finished with (best effort).
    std::string server =
        prefs()->GetString(syncer::prefs::kBrowseCraftSyncServer);
    const std::string token =
        prefs()->GetString(syncer::prefs::kBrowseCraftSyncToken);
    if (base::EndsWith(server, "/sync")) {
      server.resize(server.size() - 5);
    }
    const GURL url(server + "/api/sign-out");
    if (url.is_valid() && !token.empty()) {
      auto request = std::make_unique<network::ResourceRequest>();
      request->url = url;
      request->method = "POST";
      request->credentials_mode = network::mojom::CredentialsMode::kOmit;
      request->headers.SetHeader("Authorization", "Bearer " + token);
      sign_out_loader_ = network::SimpleURLLoader::Create(std::move(request),
                                                          kTrafficAnnotation);
      sign_out_loader_->DownloadHeadersOnly(
          profile_->GetDefaultStoragePartition()
              ->GetURLLoaderFactoryForBrowserProcess()
              .get(),
          base::BindOnce([](scoped_refptr<net::HttpResponseHeaders>) {}));
    }
    prefs()->ClearPref(syncer::prefs::kBrowseCraftSyncServer);
    prefs()->ClearPref(syncer::prefs::kBrowseCraftSyncToken);
    prefs()->ClearPref(syncer::prefs::kBrowseCraftSyncUser);
    prefs()->SetBoolean(syncer::prefs::kEnableLocalSyncBackend, false);
    CallJavascriptFunction("browsecraft.onStatus", base::Value(Status()));
  }

  void Restart(const base::ListValue& args) { chrome::AttemptRestart(); }

  raw_ptr<Profile> profile_;
  std::string server_;
  std::string user_;
  bool checked_start_ = false;
  bool started_signed_in_ = false;
  std::unique_ptr<network::SimpleURLLoader> loader_;
  std::unique_ptr<network::SimpleURLLoader> sign_out_loader_;
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
