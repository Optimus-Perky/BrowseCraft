// Copyright 2026 The BrowseCraft Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#ifndef CHROME_BROWSER_BROWSECRAFT_BROWSECRAFT_ACCOUNT_H_
#define CHROME_BROWSER_BROWSECRAFT_BROWSECRAFT_ACCOUNT_H_

#include <cstdint>
#include <string>
#include <vector>

#include "base/functional/callback_forward.h"

class Profile;

// BrowseCraft: signing in to your own BrowseCraft server (no Google account),
// with your Windows sign-in where the server has single sign-on, and the
// profile's name and photo.
namespace browsecraft {

// The address of the page that signs in (chrome://browsecraft); with
// kSignInRef, it starts a Windows sign-in straight away.
inline constexpr char kSignInPage[] = "chrome://browsecraft/";
inline constexpr char kSignInRef[] = "sign-in";

// "sync.example.home" -> "https://sync.example.home"; no trailing slash.
std::string NormaliseServer(std::string server);

// The organisation's BrowseCraft server from policy (SyncServerUrl, else
// ExtensionStoreUrl), normalised; empty when there is none.
const std::string& PolicyServerUrl();

// Signed in to a BrowseCraft server: its address (no "/sync"), the account
// name and the full name to show (the domain's, else the account name).
bool IsSignedIn(Profile* profile);
std::string SignedInServer(Profile* profile);
std::string SignedInUser(Profile* profile);
std::string SignedInDisplayName(Profile* profile);

// The server to offer for signing in: the signed-in one, else policy's, else
// the last one used.
std::string SuggestedServer(Profile* profile);

// Sign-in result, for the page and the automatic sign-in.
using SignInCallback =
    base::OnceCallback<void(bool ok, const std::string& error)>;

// Signs in with the Windows (Kerberos) account of whoever is logged on, if the
// server has single sign-on. Nothing is asked of the user.
void SignInWithWindowsAccount(Profile* profile,
                              const std::string& server,
                              SignInCallback callback);

// Signs in with a user name and password.
void SignInWithPassword(Profile* profile,
                        const std::string& server,
                        const std::string& user,
                        const std::string& password,
                        SignInCallback callback);

// Signs out (and tells the server). The profile then stays signed out: no
// automatic sign-in until the user signs in again themselves.
void SignOut(Profile* profile);

// At start-up: with a server set by policy and nobody signed in (and not
// signed out on purpose), signs in with the Windows account, quietly. Signed
// in already, it refreshes the user's full name (the profile's name) instead.
void MaybeSignInAutomatically(Profile* profile);

// The profile menu's Sign in button: the sign-in page, which tries the
// Windows account first.
void ShowSignIn(Profile* profile);

// Makes `encoded` (an image file's bytes, decoded safely out of process) the
// profile's picture. `done` gets whether it worked.
void SetProfilePhoto(Profile* profile,
                     std::vector<uint8_t> encoded,
                     base::OnceCallback<void(bool)> done);

// Whether the profile's "GAIA picture" is really the user's own photo.
bool IsOwnPhotoUrl(const std::string& picture_url);

}  // namespace browsecraft

#endif  // CHROME_BROWSER_BROWSECRAFT_BROWSECRAFT_ACCOUNT_H_
