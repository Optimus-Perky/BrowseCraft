// Copyright 2026 The BrowseCraft Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "chrome/browser/browsecraft/browsecraft_account.h"

#include <algorithm>
#include <memory>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include "base/base64.h"
#include "base/files/file_enumerator.h"
#include "base/files/file_path.h"
#include "base/files/file_util.h"
#include "base/functional/bind.h"
#include "base/functional/callback.h"
#include "base/json/json_reader.h"
#include "base/json/json_writer.h"
#include "base/logging.h"
#include "base/no_destructor.h"
#include "base/strings/string_number_conversions.h"
#include "base/strings/string_util.h"
#include "base/strings/utf_string_conversions.h"
#include "base/task/thread_pool.h"
#include "base/time/time.h"
#include "base/values.h"
#include "build/build_config.h"
#include "chrome/browser/browser_process.h"
#include "chrome/browser/profiles/profile.h"
#include "chrome/browser/profiles/profile_attributes_entry.h"
#include "chrome/browser/profiles/profile_attributes_storage.h"
#include "chrome/browser/profiles/profile_manager.h"
#include "chrome/browser/profiles/profiles_state.h"
#include "chrome/browser/ui/singleton_tabs.h"
#include "chrome/common/pref_names.h"
#include "components/prefs/pref_service.h"
#include "components/sync/base/pref_names.h"
#include "content/public/browser/storage_partition.h"
#include "net/base/net_errors.h"
#include "net/base/network_interfaces.h"
#include "net/http/http_response_headers.h"
#include "net/traffic_annotation/network_traffic_annotation.h"
#include "services/data_decoder/public/cpp/decode_image.h"
#include "services/network/public/cpp/resource_request.h"
#include "services/network/public/cpp/shared_url_loader_factory.h"
#include "services/network/public/cpp/simple_url_loader.h"
#include "services/network/public/mojom/url_response_head.mojom.h"
#include "skia/ext/image_operations.h"
#include "third_party/skia/include/core/SkBitmap.h"
#include "ui/gfx/codec/png_codec.h"
#include "ui/gfx/image/image.h"
#include "url/gurl.h"

#if BUILDFLAG(IS_WIN)
#include <windows.h>

#define SECURITY_WIN32
#include <security.h>

#include "base/win/registry.h"
#pragma comment(lib, "secur32.lib")
#endif

namespace browsecraft {

namespace {

namespace sp = syncer::prefs;

constexpr char kOwnPhotoUrl[] = "browsecraft:photo";
constexpr int kPhotoSize = 256;

constexpr net::NetworkTrafficAnnotationTag kTrafficAnnotation =
    net::DefineNetworkTrafficAnnotation("browsecraft_account", R"(
        semantics {
          sender: "BrowseCraft sync"
          description:
            "Signs in to, or out of, the user's BrowseCraft sync server: with "
            "the Windows account (Kerberos) or a user name and password."
          trigger:
            "Start-up, when the organisation's policy names a server and "
            "nobody is signed in; or the user signing in or out."
          data:
            "A Kerberos ticket for the server, or the user name and password "
            "typed; the computer's name."
          destination: OTHER
          destination_other: "The user's own BrowseCraft sync server."
        }
        policy {
          cookies_allowed: NO
          setting: "Sign out on chrome://browsecraft to stop it."
          policy_exception_justification: "The organisation's own server."
        })");

std::string ReadPolicyServer() {
  constexpr const char* kNames[] = {"SyncServerUrl", "ExtensionStoreUrl"};
#if BUILDFLAG(IS_WIN)
  for (const char* name : kNames) {
    for (HKEY root : {HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER}) {
      base::win::RegKey key(root, L"SOFTWARE\\Policies\\BrowseCraft", KEY_READ);
      std::wstring value;
      if (key.Valid() &&
          key.ReadValue(base::ASCIIToWide(name).c_str(), &value) ==
              ERROR_SUCCESS &&
          !value.empty()) {
        return base::WideToUTF8(value);
      }
    }
  }
#elif BUILDFLAG(IS_LINUX)
  for (const char* name : kNames) {
    base::FileEnumerator files(
        base::FilePath(FILE_PATH_LITERAL("/etc/browsecraft/policies/managed")),
        false, base::FileEnumerator::FILES, FILE_PATH_LITERAL("*.json"));
    for (base::FilePath f = files.Next(); !f.empty(); f = files.Next()) {
      std::string text;
      if (!base::ReadFileToString(f, &text)) {
        continue;
      }
      std::optional<base::Value> v =
          base::JSONReader::Read(text, base::JSON_PARSE_RFC);
      if (v && v->is_dict()) {
        const std::string* s = v->GetDict().FindString(name);
        if (s && !s->empty()) {
          return *s;
        }
      }
    }
  }
#endif
  return std::string();
}

#if BUILDFLAG(IS_WIN)
// A Negotiate (Kerberos) token for HTTP/<host>, from the logged-on user's
// Windows sign-in. Blocking (it may ask a domain controller).
std::optional<std::string> WindowsNegotiateToken(const std::string& host,
                                                 std::string* error) {
  CredHandle cred;
  TimeStamp expiry;
  wchar_t package[] = L"Negotiate";
  SECURITY_STATUS status = AcquireCredentialsHandleW(
      nullptr, package, SECPKG_CRED_OUTBOUND, nullptr, nullptr, nullptr,
      nullptr, &cred, &expiry);
  if (status != SEC_E_OK) {
    *error = "Windows has no sign-in to offer (" +
             base::NumberToString(static_cast<uint32_t>(status)) + ")";
    return std::nullopt;
  }
  std::wstring spn = L"HTTP/" + base::UTF8ToWide(host);
  SecBuffer out_buffer = {0, SECBUFFER_TOKEN, nullptr};
  SecBufferDesc out = {SECBUFFER_VERSION, 1, &out_buffer};
  CtxtHandle context;
  ULONG attributes = 0;
  status = InitializeSecurityContextW(
      &cred, nullptr, spn.data(), ISC_REQ_ALLOCATE_MEMORY, 0,
      SECURITY_NATIVE_DREP, nullptr, 0, &context, &out, &attributes, &expiry);
  std::optional<std::string> token;
  if ((status == SEC_E_OK || status == SEC_I_CONTINUE_NEEDED) &&
      out_buffer.pvBuffer && out_buffer.cbBuffer) {
    token = base::Base64Encode(base::span(
        static_cast<const uint8_t*>(out_buffer.pvBuffer), out_buffer.cbBuffer));
  } else {
    *error = "Windows could not get a Kerberos ticket for HTTP/" + host +
             " (" + base::NumberToString(static_cast<uint32_t>(status)) + ")";
  }
  if (out_buffer.pvBuffer) {
    FreeContextBuffer(out_buffer.pvBuffer);
  }
  if (status == SEC_E_OK || status == SEC_I_CONTINUE_NEEDED) {
    DeleteSecurityContext(&context);
  }
  FreeCredentialsHandle(&cred);
  return token;
}
#endif

Profile* ProfileAt(const base::FilePath& path) {
  ProfileManager* manager = g_browser_process->profile_manager();
  return manager ? manager->GetProfileByPath(path) : nullptr;
}

std::string WithoutSync(std::string server) {
  if (base::EndsWith(server, "/sync")) {
    server.resize(server.size() - 5);
  }
  return server;
}

// The profile is named after the signed-in user's full name from the
// directory, whatever it was called before (2026-10-12).
void UpdateProfileName(Profile* profile, const std::string& display_name) {
  if (display_name.empty()) {
    return;
  }
  profiles::UpdateProfileName(profile, base::UTF8ToUTF16(display_name));
  profile->GetPrefs()->SetString(sp::kBrowseCraftNameFromDirectory,
                                 display_name);
}

// Brings the full name and photo from the server (defined below).
void RefreshFromServer(Profile* profile);

void ApplySignIn(Profile* profile,
                 const std::string& server,
                 const std::string& token,
                 const std::string& user,
                 const std::string& display_name) {
  PrefService* prefs = profile->GetPrefs();
  // Signed in already (another account or server): stop that first, so sync
  // starts afresh. Sync watches the token.
  if (!prefs->GetString(sp::kBrowseCraftSyncToken).empty()) {
    prefs->ClearPref(sp::kBrowseCraftSyncToken);
  }
  prefs->SetString(sp::kBrowseCraftSyncServer, server + "/sync");
  prefs->SetString(sp::kBrowseCraftSyncUser, user);
  prefs->SetString(sp::kBrowseCraftSyncDisplayName,
                   display_name.empty() ? user : display_name);
  prefs->SetString(sp::kBrowseCraftSyncLastServer, server);
  prefs->SetBoolean(sp::kBrowseCraftAutoSignInOff, false);
  prefs->SetBoolean(sp::kEnableLocalSyncBackend, true);
  prefs->SetString(sp::kBrowseCraftSyncToken, token);
  UpdateProfileName(profile, display_name);
  // The photo: the server's, or this profile's own to the server.
  RefreshFromServer(profile);
}

// One sign-in request to the server; deletes itself when done.
class SignInRequest {
 public:
  static void Start(Profile* profile,
                    std::string server,
                    std::string path,
                    base::DictValue body,
                    std::optional<std::string> negotiate,
                    SignInCallback callback) {
    auto* request = new SignInRequest(profile->GetPath(), std::move(server),
                                      std::move(callback));
    request->Send(profile, path, std::move(body), std::move(negotiate));
  }

 private:
  SignInRequest(base::FilePath profile_path,
                std::string server,
                SignInCallback callback)
      : profile_path_(std::move(profile_path)),
        server_(std::move(server)),
        callback_(std::move(callback)) {}

  void Send(Profile* profile,
            const std::string& path,
            base::DictValue body,
            std::optional<std::string> negotiate) {
    auto request = std::make_unique<network::ResourceRequest>();
    request->url = GURL(server_ + path);
    request->method = "POST";
    // No cookies or ambient authentication: a Windows ticket, when there is
    // one, is fetched by BrowseCraft and sent here.
    request->credentials_mode = network::mojom::CredentialsMode::kOmit;
    if (negotiate) {
      request->headers.SetHeader("Authorization", "Negotiate " + *negotiate);
    }
    body.Set("device", net::GetHostName());
    std::string json;
    base::JSONWriter::Write(body, &json);
    loader_ = network::SimpleURLLoader::Create(std::move(request),
                                               kTrafficAnnotation);
    loader_->AttachStringForUpload(json, "application/json");
    loader_->SetAllowHttpErrorResults(true);
    loader_->SetTimeoutDuration(base::Seconds(30));
    loader_->DownloadToString(
        profile->GetDefaultStoragePartition()
            ->GetURLLoaderFactoryForBrowserProcess()
            .get(),
        base::BindOnce(&SignInRequest::Done, base::Unretained(this)),
        1 << 20);
  }

  void Done(std::optional<std::string> body) {
    std::unique_ptr<SignInRequest> self(this);
    const int code = loader_->ResponseInfo() && loader_->ResponseInfo()->headers
                         ? loader_->ResponseInfo()->headers->response_code()
                         : 0;
    const int net_error = loader_->NetError();
    std::optional<base::DictValue> reply =
        body ? base::JSONReader::ReadDict(*body, base::JSON_PARSE_RFC)
             : std::nullopt;
    const std::string* token = reply ? reply->FindString("token") : nullptr;
    if (code == 200 && token && !token->empty()) {
      Profile* profile = ProfileAt(profile_path_);
      if (!profile) {
        std::move(callback_).Run(false, "The profile has closed.");
        return;
      }
      const std::string* user = reply->FindStringByDottedPath("user.name");
      const std::string* display_name = reply->FindString("displayName");
      ApplySignIn(profile, server_, *token, user ? *user : std::string(),
                  display_name ? *display_name : std::string());
      std::move(callback_).Run(true, std::string());
      return;
    }
    const std::string* error = reply ? reply->FindString("error") : nullptr;
    if (error) {
      std::move(callback_).Run(false, *error);
    } else if (code == 0) {
      std::move(callback_).Run(false, "Couldn't reach " + server_ + " (" +
                                          net::ErrorToShortString(net_error) +
                                          ").");
    } else if (code == 401) {
      std::move(callback_).Run(
          false, "This computer has no Windows sign-in the server accepts.");
    } else {
      std::move(callback_).Run(false, "That isn't an AltBrowse server (it "
                                      "answered " +
                                          base::NumberToString(code) + ").");
    }
  }

  const base::FilePath profile_path_;
  const std::string server_;
  SignInCallback callback_;
  std::unique_ptr<network::SimpleURLLoader> loader_;
};

// Tells the server a token is finished with (best effort); deletes itself.
class SignOutRequest {
 public:
  static void Start(Profile* profile,
                    const std::string& server,
                    const std::string& token) {
    auto* r = new SignOutRequest();
    auto request = std::make_unique<network::ResourceRequest>();
    request->url = GURL(server + "/api/sign-out");
    request->method = "POST";
    request->credentials_mode = network::mojom::CredentialsMode::kOmit;
    request->headers.SetHeader("Authorization", "Bearer " + token);
    r->loader_ = network::SimpleURLLoader::Create(std::move(request),
                                                  kTrafficAnnotation);
    r->loader_->SetTimeoutDuration(base::Seconds(15));
    r->loader_->DownloadHeadersOnly(
        profile->GetDefaultStoragePartition()
            ->GetURLLoaderFactoryForBrowserProcess()
            .get(),
        base::BindOnce(
            [](SignOutRequest* r, scoped_refptr<net::HttpResponseHeaders>) {
              delete r;
            },
            base::Unretained(r)));
  }

 private:
  std::unique_ptr<network::SimpleURLLoader> loader_;
};

// Sends the profile's photo (PNG) to the server, so every browser the user
// signs in to shows it; deletes itself.
class PhotoUpload {
 public:
  static void Start(Profile* profile, std::vector<uint8_t> png) {
    if (!IsSignedIn(profile) || png.empty()) {
      return;
    }
    auto* r = new PhotoUpload(profile->GetPath());
    auto request = std::make_unique<network::ResourceRequest>();
    request->url = GURL(SignedInServer(profile) + "/api/photo");
    request->method = "PUT";
    request->credentials_mode = network::mojom::CredentialsMode::kOmit;
    request->headers.SetHeader(
        "Authorization",
        "Bearer " + profile->GetPrefs()->GetString(sp::kBrowseCraftSyncToken));
    r->loader_ = network::SimpleURLLoader::Create(std::move(request),
                                                  kTrafficAnnotation);
    r->loader_->AttachStringForUpload(std::string(png.begin(), png.end()),
                                      "image/png");
    r->loader_->SetTimeoutDuration(base::Seconds(60));
    r->loader_->DownloadToString(
        profile->GetDefaultStoragePartition()
            ->GetURLLoaderFactoryForBrowserProcess()
            .get(),
        base::BindOnce(&PhotoUpload::Done, base::Unretained(r)), 1 << 16);
  }

 private:
  explicit PhotoUpload(base::FilePath path) : path_(std::move(path)) {}

  void Done(std::optional<std::string> body) {
    std::unique_ptr<PhotoUpload> self(this);
    Profile* profile = ProfileAt(path_);
    std::optional<base::DictValue> reply =
        body ? base::JSONReader::ReadDict(*body, base::JSON_PARSE_RFC)
             : std::nullopt;
    const std::string* sha = reply ? reply->FindString("photo") : nullptr;
    if (!profile || !sha) {
      LOG(WARNING) << "BrowseCraft: the photo did not reach the server";
      return;
    }
    // Ours already: no need to fetch it back.
    profile->GetPrefs()->SetString(sp::kBrowseCraftPhotoSha, *sha);
  }

  const base::FilePath path_;
  std::unique_ptr<network::SimpleURLLoader> loader_;
};

// The profile's own photo as PNG, if it has one.
std::vector<uint8_t> OwnPhotoPng(Profile* profile) {
  ProfileAttributesEntry* entry =
      g_browser_process->profile_manager()
          ->GetProfileAttributesStorage()
          .GetProfileAttributesWithPath(profile->GetPath());
  const gfx::Image* image = entry ? entry->GetGAIAPicture() : nullptr;
  if (!image || image->IsEmpty() ||
      !IsOwnPhotoUrl(entry->GetLastDownloadedGAIAPictureUrlWithSize())) {
    return {};
  }
  return gfx::PNGCodec::EncodeBGRASkBitmap(image->AsBitmap(),
                                          /*discard_transparency=*/false)
      .value_or(std::vector<uint8_t>());
}

// Decodes a photo (out of process), crops it square and makes it the
// profile's picture. `upload`: it was chosen here, so the server gets it too.
// `server_sha`: it came from the server, which has it under that hash.
void ApplyPhoto(Profile* profile,
                std::vector<uint8_t> encoded,
                bool upload,
                std::string server_sha,
                base::OnceCallback<void(bool)> done) {
  data_decoder::DecodeImageIsolated(
      encoded, data_decoder::mojom::ImageCodec::kDefault,
      /*shrink_to_fit=*/false, 16 << 20, gfx::Size(),
      base::BindOnce(
          [](base::FilePath path, bool upload, std::string server_sha,
             base::OnceCallback<void(bool)> done, const SkBitmap& bitmap) {
            Profile* profile = ProfileAt(path);
            ProfileAttributesEntry* entry =
                profile ? g_browser_process->profile_manager()
                              ->GetProfileAttributesStorage()
                              .GetProfileAttributesWithPath(path)
                        : nullptr;
            if (bitmap.isNull() || bitmap.width() < 1 || bitmap.height() < 1 ||
                !entry) {
              std::move(done).Run(false);
              return;
            }
            // The middle square, at the size Chromium keeps account photos.
            const int side = std::min(bitmap.width(), bitmap.height());
            SkBitmap square;
            if (!bitmap.extractSubset(
                    &square,
                    SkIRect::MakeXYWH((bitmap.width() - side) / 2,
                                      (bitmap.height() - side) / 2, side,
                                      side))) {
              std::move(done).Run(false);
              return;
            }
            SkBitmap photo = skia::ImageOperations::Resize(
                square, skia::ImageOperations::RESIZE_BEST, kPhotoSize,
                kPhotoSize);
            // A new address each time, or an unchanged one would be skipped.
            entry->SetGAIAPicture(
                std::string(kOwnPhotoUrl) + ":" +
                    base::NumberToString(
                        base::Time::Now().InMillisecondsSinceUnixEpoch()),
                gfx::Image::CreateFrom1xBitmap(photo));
            PrefService* prefs = profile->GetPrefs();
            prefs->SetBoolean(::prefs::kProfileUsingDefaultAvatar, false);
            prefs->SetBoolean(::prefs::kProfileUsingGAIAAvatar, true);
            entry->SetIsUsingGAIAPicture(true);
            if (!server_sha.empty()) {
              prefs->SetString(sp::kBrowseCraftPhotoSha, server_sha);
            }
            if (upload) {
              PhotoUpload::Start(
                  profile, gfx::PNGCodec::EncodeBGRASkBitmap(
                               photo, /*discard_transparency=*/false)
                               .value_or(std::vector<uint8_t>()));
            }
            std::move(done).Run(true);
          },
          profile->GetPath(), upload, std::move(server_sha), std::move(done)));
}

// Fetches the user's photo from the server and shows it; deletes itself.
class PhotoDownload {
 public:
  static void Start(Profile* profile, std::string sha) {
    auto* r = new PhotoDownload(profile->GetPath(), std::move(sha));
    auto request = std::make_unique<network::ResourceRequest>();
    request->url = GURL(SignedInServer(profile) + "/api/photo");
    request->credentials_mode = network::mojom::CredentialsMode::kOmit;
    request->headers.SetHeader(
        "Authorization",
        "Bearer " + profile->GetPrefs()->GetString(sp::kBrowseCraftSyncToken));
    r->loader_ = network::SimpleURLLoader::Create(std::move(request),
                                                  kTrafficAnnotation);
    r->loader_->SetTimeoutDuration(base::Seconds(60));
    r->loader_->DownloadToString(
        profile->GetDefaultStoragePartition()
            ->GetURLLoaderFactoryForBrowserProcess()
            .get(),
        base::BindOnce(&PhotoDownload::Done, base::Unretained(r)), 4 << 20);
  }

 private:
  PhotoDownload(base::FilePath path, std::string sha)
      : path_(std::move(path)), sha_(std::move(sha)) {}

  void Done(std::optional<std::string> body) {
    std::unique_ptr<PhotoDownload> self(this);
    Profile* profile = ProfileAt(path_);
    if (!profile || !body || body->empty()) {
      return;
    }
    ApplyPhoto(profile, std::vector<uint8_t>(body->begin(), body->end()),
               /*upload=*/false, sha_, base::BindOnce([](bool ok) {
                 LOG_IF(WARNING, !ok)
                     << "BrowseCraft: the server's photo could not be shown";
               }));
  }

  const base::FilePath path_;
  const std::string sha_;
  std::unique_ptr<network::SimpleURLLoader> loader_;
};

// Asks the server who the signed-in user is (GET /api/me): brings the full
// name (and so the profile's name) and the photo up to date, or gives the
// server this profile's photo when it has none; deletes itself.
class RefreshRequest {
 public:
  static void Start(Profile* profile) {
    auto* r = new RefreshRequest(profile->GetPath());
    auto request = std::make_unique<network::ResourceRequest>();
    request->url = GURL(SignedInServer(profile) + "/api/me");
    request->credentials_mode = network::mojom::CredentialsMode::kOmit;
    request->headers.SetHeader(
        "Authorization",
        "Bearer " + profile->GetPrefs()->GetString(sp::kBrowseCraftSyncToken));
    r->loader_ = network::SimpleURLLoader::Create(std::move(request),
                                                  kTrafficAnnotation);
    r->loader_->SetTimeoutDuration(base::Seconds(30));
    r->loader_->DownloadToString(
        profile->GetDefaultStoragePartition()
            ->GetURLLoaderFactoryForBrowserProcess()
            .get(),
        base::BindOnce(&RefreshRequest::Done, base::Unretained(r)), 1 << 16);
  }

 private:
  explicit RefreshRequest(base::FilePath path) : path_(std::move(path)) {}

  void Done(std::optional<std::string> body) {
    std::unique_ptr<RefreshRequest> self(this);
    Profile* profile = ProfileAt(path_);
    std::optional<base::DictValue> reply =
        body ? base::JSONReader::ReadDict(*body, base::JSON_PARSE_RFC)
             : std::nullopt;
    if (!profile || !reply || !IsSignedIn(profile)) {
      return;
    }
    if (const std::string* name = reply->FindString("displayName");
        name && !name->empty()) {
      profile->GetPrefs()->SetString(sp::kBrowseCraftSyncDisplayName, *name);
      UpdateProfileName(profile, *name);
    }
    const std::string* sha = reply->FindString("photo");
    if (sha && !sha->empty()) {
      if (*sha != profile->GetPrefs()->GetString(sp::kBrowseCraftPhotoSha)) {
        PhotoDownload::Start(profile, *sha);
      }
    } else {
      // The server has no photo for this user: give it this profile's.
      PhotoUpload::Start(profile, OwnPhotoPng(profile));
    }
  }

  const base::FilePath path_;
  std::unique_ptr<network::SimpleURLLoader> loader_;
};

void RefreshFromServer(Profile* profile) {
  if (IsSignedIn(profile)) {
    RefreshRequest::Start(profile);
  }
}

}  // namespace

std::string NormaliseServer(std::string server) {
  base::TrimWhitespaceASCII(server, base::TRIM_ALL, &server);
  while (!server.empty() && server.back() == '/') {
    server.pop_back();
  }
  if (!server.empty() && server.find("://") == std::string::npos) {
    server = "https://" + server;
  }
  return server;
}

const std::string& PolicyServerUrl() {
  static const base::NoDestructor<std::string> server(
      NormaliseServer(ReadPolicyServer()));
  return *server;
}

bool IsSignedIn(Profile* profile) {
  PrefService* prefs = profile->GetPrefs();
  return !prefs->GetString(sp::kBrowseCraftSyncServer).empty() &&
         !prefs->GetString(sp::kBrowseCraftSyncToken).empty();
}

std::string SignedInServer(Profile* profile) {
  return WithoutSync(profile->GetPrefs()->GetString(sp::kBrowseCraftSyncServer));
}

std::string SignedInUser(Profile* profile) {
  return profile->GetPrefs()->GetString(sp::kBrowseCraftSyncUser);
}

std::string SignedInDisplayName(Profile* profile) {
  std::string name =
      profile->GetPrefs()->GetString(sp::kBrowseCraftSyncDisplayName);
  return name.empty() ? SignedInUser(profile) : name;
}

std::string SuggestedServer(Profile* profile) {
  if (IsSignedIn(profile)) {
    return SignedInServer(profile);
  }
  if (!PolicyServerUrl().empty()) {
    return PolicyServerUrl();
  }
  return profile->GetPrefs()->GetString(sp::kBrowseCraftSyncLastServer);
}

void SignInWithWindowsAccount(Profile* profile,
                              const std::string& server_in,
                              SignInCallback callback) {
  const std::string server = NormaliseServer(server_in);
  const GURL url(server);
  if (!url.is_valid() || !url.SchemeIsHTTPOrHTTPS()) {
    std::move(callback).Run(false, "That server address doesn't look right.");
    return;
  }
#if BUILDFLAG(IS_WIN)
  base::ThreadPool::PostTaskAndReplyWithResult(
      FROM_HERE, {base::MayBlock(), base::TaskPriority::USER_VISIBLE},
      base::BindOnce(
          [](std::string host) {
            std::string error;
            std::optional<std::string> token =
                WindowsNegotiateToken(host, &error);
            return std::make_pair(token, error);
          },
          std::string(url.host())),
      base::BindOnce(
          [](base::FilePath path, std::string server, SignInCallback callback,
             std::pair<std::optional<std::string>, std::string> result) {
            Profile* profile = ProfileAt(path);
            if (!profile) {
              std::move(callback).Run(false, "The profile has closed.");
              return;
            }
            if (!result.first) {
              std::move(callback).Run(false, result.second);
              return;
            }
            SignInRequest::Start(profile, server, "/api/sign-in/negotiate",
                                 base::DictValue(), result.first,
                                 std::move(callback));
          },
          profile->GetPath(), server, std::move(callback)));
#else
  // ungoogled-chromium builds without Kerberos (use_kerberos=false) outside
  // Windows, where BrowseCraft asks Windows itself.
  std::move(callback).Run(
      false, "Signing in with the logon account works on Windows only.");
#endif
}

void SignInWithPassword(Profile* profile,
                        const std::string& server_in,
                        const std::string& user,
                        const std::string& password,
                        SignInCallback callback) {
  const std::string server = NormaliseServer(server_in);
  const GURL url(server);
  if (!url.is_valid() || !url.SchemeIsHTTPOrHTTPS()) {
    std::move(callback).Run(false, "That server address doesn't look right.");
    return;
  }
  base::DictValue body;
  body.Set("user", user);
  body.Set("password", password);
  SignInRequest::Start(profile, server, "/api/sign-in", std::move(body),
                       std::nullopt, std::move(callback));
}

void SignOut(Profile* profile) {
  PrefService* prefs = profile->GetPrefs();
  const std::string server = SignedInServer(profile);
  const std::string token = prefs->GetString(sp::kBrowseCraftSyncToken);
  if (GURL(server).is_valid() && !token.empty()) {
    SignOutRequest::Start(profile, server, token);
  }
  prefs->ClearPref(sp::kBrowseCraftSyncToken);
  prefs->ClearPref(sp::kBrowseCraftSyncServer);
  prefs->ClearPref(sp::kBrowseCraftSyncUser);
  prefs->ClearPref(sp::kBrowseCraftSyncDisplayName);
  prefs->ClearPref(sp::kBrowseCraftPhotoSha);
  prefs->SetBoolean(sp::kBrowseCraftAutoSignInOff, true);
}

void MaybeSignInAutomatically(Profile* profile) {
  if (!profile->IsRegularProfile() || profile->IsGuestSession()) {
    return;
  }
  if (IsSignedIn(profile)) {
    // Already signed in: the full name or photo may have changed elsewhere.
    RefreshFromServer(profile);
    return;
  }
  if (PolicyServerUrl().empty() ||
      profile->GetPrefs()->GetBoolean(sp::kBrowseCraftAutoSignInOff)) {
    return;
  }
  SignInWithWindowsAccount(
      profile, PolicyServerUrl(),
      base::BindOnce([](bool ok, const std::string& error) {
        if (ok) {
          VLOG(1) << "BrowseCraft: signed in with the Windows account";
        } else {
          LOG(WARNING) << "BrowseCraft: automatic sign-in to "
                       << PolicyServerUrl() << " failed: " << error;
        }
      }));
}

void ShowSignIn(Profile* profile) {
  ShowSingletonTab(profile,
                   GURL(std::string(kSignInPage) + "#" + kSignInRef));
}

bool IsOwnPhotoUrl(const std::string& picture_url) {
  return base::StartsWith(picture_url, kOwnPhotoUrl);
}

void SetProfilePhoto(Profile* profile,
                     std::vector<uint8_t> encoded,
                     base::OnceCallback<void(bool)> done) {
  ApplyPhoto(profile, std::move(encoded), /*upload=*/true, std::string(),
             std::move(done));
}

}  // namespace browsecraft
