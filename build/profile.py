#!/usr/bin/env python3
"""BrowseCraft: signing in with your Windows account, and the profile's name and photo.

- Single sign-on: at start-up, with a BrowseCraft server set by policy (SyncServerUrl, else
  ExtensionStoreUrl) and nobody signed in, the profile signs in with the Windows (Kerberos)
  account of whoever is logged on; the server must have single sign-on set up. Signing out
  stops that for the profile until its user signs in again.
- The profile menu has "Sign in" (the chrome://browsecraft page, which tries the Windows account
  first, then offers a user name and password) and, signed in, who and where.
- Sync starts and stops as you sign in and out: no restart. (BrowseCraft always runs Chromium's
  "local sync" mode, pointed at the user's server; with no sign-in it does not run at all.)
- The profile's name is the full name from the directory (AD displayName), whatever the user had
  called it, refreshed from the server at every start-up.
- Settings > Customise profile has "Upload a photo": the picture is cropped square, decoded out of
  process, and kept as the profile's picture (Chromium's slot for a Google account photo, which
  BrowseCraft never uses otherwise). Signed in, the photo is kept on the server too (PUT
  /api/photo) and every browser the user signs in to shows it (checked at sign-in and start-up via
  /api/me's photo hash); the server starts from the AD thumbnailPhoto.

New files are in profile/ next to this script. Run in the src tree after the BrowseCraft patches
(sync-to-own-server.patch and restore-sync-service.patch) and extstore.py."""
import os
import shutil
import sys

HERE = os.path.dirname(os.path.abspath(__file__))

EDITS = {
    "components/sync/base/pref_names.h": [
        ('inline constexpr char kBrowseCraftSyncUser[] = "browsecraft.sync.user";\n',
         'inline constexpr char kBrowseCraftSyncUser[] = "browsecraft.sync.user";\n'
         '// BrowseCraft: the signed-in user\'s full name, the last server signed in to,\n'
         '// "signed out on purpose" (no automatic sign-in), and the profile name last\n'
         '// taken from the directory (so it follows the directory until renamed).\n'
         'inline constexpr char kBrowseCraftSyncDisplayName[] =\n    "browsecraft.sync.display_name";\n'
         'inline constexpr char kBrowseCraftSyncLastServer[] =\n    "browsecraft.sync.last_server";\n'
         'inline constexpr char kBrowseCraftAutoSignInOff[] =\n    "browsecraft.sync.auto_sign_in_off";\n'
         'inline constexpr char kBrowseCraftNameFromDirectory[] =\n    "browsecraft.profile.name_from_directory";\n'),
        # ... and, added later, the photo's hash (its own edit, so patched trees take it too).
        ('inline constexpr char kBrowseCraftNameFromDirectory[] =\n    "browsecraft.profile.name_from_directory";\n',
         'inline constexpr char kBrowseCraftNameFromDirectory[] =\n    "browsecraft.profile.name_from_directory";\n'
         '// BrowseCraft: the hash of the profile photo the server has (and this\n// profile shows), so a changed one is fetched.\n'
         'inline constexpr char kBrowseCraftPhotoSha[] = "browsecraft.profile.photo_sha";\n'),
    ],
    "components/sync/service/sync_prefs.cc": [
        ("  registry->RegisterStringPref(prefs::kBrowseCraftSyncUser, std::string());\n",
         "  registry->RegisterStringPref(prefs::kBrowseCraftSyncUser, std::string());\n"
         "  registry->RegisterStringPref(prefs::kBrowseCraftSyncDisplayName,\n                               std::string());\n"
         "  registry->RegisterStringPref(prefs::kBrowseCraftSyncLastServer,\n                               std::string());\n"
         "  registry->RegisterBooleanPref(prefs::kBrowseCraftAutoSignInOff, false);\n"
         "  registry->RegisterStringPref(prefs::kBrowseCraftNameFromDirectory,\n                               std::string());\n"),
        ("  registry->RegisterStringPref(prefs::kBrowseCraftNameFromDirectory,\n                               std::string());\n",
         "  registry->RegisterStringPref(prefs::kBrowseCraftNameFromDirectory,\n                               std::string());\n"
         "  registry->RegisterStringPref(prefs::kBrowseCraftPhotoSha, std::string());\n"),
        ("""      local_sync_enabled_(
          pref_service_->GetBoolean(prefs::kEnableLocalSyncBackend)) {""",
         """      // BrowseCraft: always "local sync", which goes to the user's own
      // server (and does not run while nobody is signed in to one).
      local_sync_enabled_(true) {"""),
    ],
    "components/sync/service/sync_service_impl.h": [
        ('#include "base/timer/timer.h"\n',
         '#include "base/timer/timer.h"\n#include "components/prefs/pref_change_registrar.h"\n'),
        ("""  void TryStart();
""",
         """  void TryStart();

  // BrowseCraft: signed in to or out of the user's server: start or stop.
  void OnBrowseCraftSignInChanged();
"""),
        ("""  std::unique_ptr<SyncPrefsPolicyHandler> sync_prefs_policy_handler_;
""",
         """  std::unique_ptr<SyncPrefsPolicyHandler> sync_prefs_policy_handler_;

  // BrowseCraft: watches the sign-in token.
  PrefChangeRegistrar browsecraft_registrar_;
"""),
    ],
    "components/sync/service/sync_service_impl.cc": [
        ('#include "components/sync/service/sync_service_impl.h"\n',
         '#include "components/sync/service/sync_service_impl.h"\n\n#include "components/sync/base/pref_names.h"\n'),
        ("""  sync_prefs_policy_handler_ = std::make_unique<SyncPrefsPolicyHandler>(this);
""",
         """  sync_prefs_policy_handler_ = std::make_unique<SyncPrefsPolicyHandler>(this);

  // BrowseCraft: sync follows signing in and out, without a restart.
  browsecraft_registrar_.Init(sync_client_->GetPrefService());
  browsecraft_registrar_.Add(
      prefs::kBrowseCraftSyncToken,
      base::BindRepeating(&SyncServiceImpl::OnBrowseCraftSignInChanged,
                          base::Unretained(this)));
"""),
        ("""  DisableReasonSet result;

  // If local sync is enabled, most disable reasons don't apply.
""",
         """  DisableReasonSet result;

  // BrowseCraft: local sync goes to the user's own server; with nobody signed
  // in to one, it does not run.
  if (IsLocalSyncEnabled() &&
      (sync_prefs_.GetBrowseCraftSyncToken().empty() ||
       !GURL(sync_prefs_.GetBrowseCraftSyncServer()).is_valid())) {
    result.Put(DISABLE_REASON_NOT_SIGNED_IN);
  }

  // If local sync is enabled, most disable reasons don't apply.
"""),
        ("""void SyncServiceImpl::TryStartImpl(
""",
         """void SyncServiceImpl::OnBrowseCraftSignInChanged() {
  DCHECK_CALLED_ON_VALID_SEQUENCE(sequence_checker_);
  if (!IsLocalSyncEnabled() || !data_type_manager_) {
    return;
  }
  if (HasDisableReason(DISABLE_REASON_NOT_SIGNED_IN)) {
    // Signed out: stop, and forget the sync state (not the data).
    StopAndClear(ResetEngineReason::kNotSignedIn);
  } else if (engine_) {
    // A new sign-in over a running engine: start over (ResetEngine restarts).
    ResetEngine(ResetEngineReason::kResetLocalData);
  } else {
    TryStart();
  }
  NotifyObservers();
}

void SyncServiceImpl::TryStartImpl(
"""),
    ],
    "chrome/browser/sync/sync_service_factory.cc": [
        ("""    // BrowseCraft: signed in to a BrowseCraft server, sync goes there and
    // needs no folder.
    if (local_sync_backend_folder.empty() &&
        prefs.GetBrowseCraftSyncToken().empty()) {
      return nullptr;
    }
""",
         """    // BrowseCraft: sync only ever goes to the user's BrowseCraft server, so
    // no folder is needed (and the service must exist to sign in later).
"""),
    ],
    "chrome/browser/BUILD.gn": [
        ('    "chrome_browser_main.cc",\n',
         '    "browsecraft/browsecraft_account.cc",\n    "browsecraft/browsecraft_account.h",\n    "chrome_browser_main.cc",\n'),
    ],
    "chrome/browser/chrome_browser_main.cc": [
        ("""  for (auto& chrome_extra_part : chrome_extra_parts_) {
    chrome_extra_part->PostProfileInit(profile, is_initial_profile);
  }
""",
         """  for (auto& chrome_extra_part : chrome_extra_parts_) {
    chrome_extra_part->PostProfileInit(profile, is_initial_profile);
  }

  // BrowseCraft: sign in with the Windows account, where policy says where.
  browsecraft::MaybeSignInAutomatically(profile);
"""),
        ('#include "chrome/browser/chrome_browser_main.h"\n',
         '#include "chrome/browser/chrome_browser_main.h"\n\n#include "chrome/browser/browsecraft/browsecraft_account.h"\n'),
    ],
    "chrome/browser/profiles/gaia_info_update_service.cc": [
        ("""  entry->SetGAIAGivenName(std::u16string());
  entry->SetGAIAPicture(std::string(), gfx::Image());
""",
         """  entry->SetGAIAGivenName(std::u16string());
  // BrowseCraft: there is no Google account; this is the user's own photo.
  if (!browsecraft::IsOwnPhotoUrl(
          entry->GetLastDownloadedGAIAPictureUrlWithSize())) {
    entry->SetGAIAPicture(std::string(), gfx::Image());
  }
"""),
        ('#include "chrome/browser/profiles/gaia_info_update_service.h"\n',
         '#include "chrome/browser/profiles/gaia_info_update_service.h"\n\n#include "chrome/browser/browsecraft/browsecraft_account.h"\n'),
    ],
    "chrome/browser/profiles/profile_avatar_icon_util.cc": [
        ("""    avatars.Insert(avatars.begin(),
                   base::Value(std::move(generic_avatar_info)));
    return avatars;
  }
""",
         """    avatars.Insert(avatars.begin(),
                   base::Value(std::move(generic_avatar_info)));
    // BrowseCraft: the user's own photo (no Google account), first.
    if (const gfx::Image* own_photo = entry->GetGAIAPicture()) {
      avatars.Insert(
          avatars.begin(),
          base::Value(GetAvatarIconAndLabelDict(
              webui::GetBitmapDataUrl(
                  GetAvatarIconForWebUI(*own_photo).AsBitmap()),
              u"Your photo", /*index=*/0, using_gaia,
              /*is_gaia_avatar=*/true)));
    }
    return avatars;
  }
"""),
    ],
    "chrome/browser/ui/views/profiles/profile_menu_view.h": [
        ("""  void OnEditProfileButtonClicked();
""",
         """  void OnEditProfileButtonClicked();
  // BrowseCraft: sign in to the user's BrowseCraft server.
  void OnBrowseCraftSignInButtonClicked();
"""),
    ],
    "chrome/browser/ui/views/profiles/profile_menu_view.cc": [
        ("""  ActionableItem button_type = ActionableItem::kSigninAccountButton;
""",
         """  // BrowseCraft: no Google sign-in; the user's own BrowseCraft server.
  if (browsecraft::IsSignedIn(&profile())) {
    params.subtitle = base::UTF8ToUTF16(
        "Syncing to " +
        std::string(GURL(browsecraft::SignedInServer(&profile())).host()));
  } else {
    params.subtitle = u"Sign in to sync your bookmarks, passwords and more";
    params.button_text = u"Sign in";
    params.button_action = base::BindRepeating(
        &ProfileMenuView::OnBrowseCraftSignInButtonClicked,
        base::Unretained(this));
  }
  if (true) {
    return params;
  }

  ActionableItem button_type = ActionableItem::kSigninAccountButton;
"""),
        ("""void ProfileMenuView::OnYourSavedInfoSettingsButtonClicked() {
""",
         """void ProfileMenuView::OnBrowseCraftSignInButtonClicked() {
  OnActionableItemClicked(ActionableItem::kSigninButton);
  if (!perform_menu_actions()) {
    return;
  }
  browsecraft::ShowSignIn(&profile());
}

void ProfileMenuView::OnYourSavedInfoSettingsButtonClicked() {
"""),
        ('#include "chrome/browser/ui/views/profiles/profile_menu_view.h"\n',
         '#include "chrome/browser/ui/views/profiles/profile_menu_view.h"\n\n#include "chrome/browser/browsecraft/browsecraft_account.h"\n'),
    ],
    "chrome/browser/ui/webui/settings/settings_manage_profile_handler.cc": [
        ("""  web_ui()->RegisterMessageCallback(
      "setProfileName",
""",
         """  // BrowseCraft: the user's own photo (base64 of an image, cropped square by
  // the page; decoded out of process).
  web_ui()->RegisterMessageCallback(
      "setProfilePhoto",
      base::BindRepeating(
          [](Profile* profile, const base::ListValue& args) {
            if (args.empty() || !args[0].is_string()) {
              return;
            }
            std::optional<std::vector<uint8_t>> bytes =
                base::Base64Decode(args[0].GetString());
            if (!bytes || bytes->empty()) {
              return;
            }
            browsecraft::SetProfilePhoto(
                profile, std::move(*bytes), base::BindOnce([](bool ok) {
                  LOG_IF(WARNING, !ok)
                      << "BrowseCraft: that photo could not be used";
                }));
          },
          base::Unretained(profile_)));
  web_ui()->RegisterMessageCallback(
      "setProfileName",
"""),
        ('#include "chrome/browser/ui/webui/settings/settings_manage_profile_handler.h"\n',
         '#include "chrome/browser/ui/webui/settings/settings_manage_profile_handler.h"\n\n'
         '#include "base/base64.h"\n#include "base/logging.h"\n#include "chrome/browser/browsecraft/browsecraft_account.h"\n'),
    ],
    "chrome/browser/resources/settings/people_page/manage_profile_browser_proxy.ts": [
        ("""  setProfileIconToDefaultAvatar(index: number): void;
""",
         """  setProfileIconToDefaultAvatar(index: number): void;

  /**
   * BrowseCraft: makes a photo (base64 of an image file) the profile's icon.
   */
  setProfilePhoto(base64: string): void;
"""),
        ("""  setProfileName(name: string) {
    chrome.send('setProfileName', [name]);
  }
""",
         """  setProfileName(name: string) {
    chrome.send('setProfileName', [name]);
  }

  setProfilePhoto(base64: string) {
    chrome.send('setProfilePhoto', [base64]);
  }
"""),
    ],
    "chrome/browser/resources/settings/people_page/manage_profile.html.ts": [
        ("""        @selected-avatar-changed="${this.onSelectedAvatarChanged_}"
        ignore-modified-key-events>
    </cr-profile-avatar-selector>
""",
         """        @selected-avatar-changed="${this.onSelectedAvatarChanged_}"
        ignore-modified-key-events>
    </cr-profile-avatar-selector>
    <div id="photoRow">
      <cr-button id="uploadPhoto" @click="${this.onUploadPhotoClick_}">
        Upload a photo&hellip;
      </cr-button>
      <input id="photoFile" type="file" accept="image/*" hidden
          @change="${this.onPhotoFileChange_}">
    </div>
"""),
    ],
    "chrome/browser/resources/settings/people_page/manage_profile.ts": [
        ("""import 'chrome://resources/cr_elements/cr_icon/cr_icon.js';
""",
         """import 'chrome://resources/cr_elements/cr_button/cr_button.js';
import 'chrome://resources/cr_elements/cr_icon/cr_icon.js';
"""),
        ("""  $: {
    nameInput: CrInputElement,
  };""",
         """  $: {
    nameInput: CrInputElement,
    photoFile: HTMLInputElement,
  };"""),
        ("""  protected onSelectedAvatarChanged_(e: CustomEvent<{value: AvatarIcon}>) {
""",
         """  // BrowseCraft: upload a photo for the profile.
  protected onUploadPhotoClick_() {
    this.$.photoFile.value = '';
    this.$.photoFile.click();
  }

  protected async onPhotoFileChange_() {
    const file = this.$.photoFile.files?.[0];
    if (!file) {
      return;
    }
    // The middle square at 256 pixels, as PNG (decoded again, out of
    // process, by the browser).
    const bitmap = await createImageBitmap(file);
    const side = Math.min(bitmap.width, bitmap.height);
    const canvas = document.createElement('canvas');
    canvas.width = canvas.height = 256;
    canvas.getContext('2d')!.drawImage(
        bitmap, (bitmap.width - side) / 2, (bitmap.height - side) / 2, side,
        side, 0, 0, 256, 256);
    bitmap.close();
    const url = canvas.toDataURL('image/png');
    this.browserProxy_.setProfilePhoto(url.substring(url.indexOf(',') + 1));
  }

  protected onSelectedAvatarChanged_(e: CustomEvent<{value: AvatarIcon}>) {
"""),
    ],
}

# manage_profile.css is plain CSS in recent Chromium; append a rule rather than match.
CSS_FILE = "chrome/browser/resources/settings/people_page/manage_profile.css"
CSS_RULE = "\n/* BrowseCraft: Upload a photo */\n#photoRow {\n  margin-top: 16px;\n}\n"

NEW_FILES = [
    "chrome/browser/browsecraft/browsecraft_account.h",
    "chrome/browser/browsecraft/browsecraft_account.cc",
    "chrome/browser/ui/webui/browsecraft/browsecraft_ui.cc",
]


def main():
    failed = False
    for rel in NEW_FILES:
        os.makedirs(os.path.dirname(rel), exist_ok=True)
        shutil.copyfile(os.path.join(HERE, "profile", rel), rel)
    for path, edits in EDITS.items():
        try:
            text = open(path, encoding="utf-8").read()
        except OSError as e:
            print(f"profile.py: {path}: {e}", file=sys.stderr)
            failed = True
            continue
        for old, new in edits:
            if new in text:
                continue  # already applied
            n = text.count(old)
            if n != 1:
                print(f"profile.py: {path}: expected one match, found {n}: {old[:70]!r}", file=sys.stderr)
                failed = True
                continue
            text = text.replace(old, new, 1)
        if not failed:
            open(path, "w", encoding="utf-8").write(text)
    css = open(CSS_FILE, encoding="utf-8").read()
    if CSS_RULE not in css:
        open(CSS_FILE, "a", encoding="utf-8").write(CSS_RULE)
    if failed:
        sys.exit(1)
    print("profile.py: Windows sign-in, profile name and photo applied")


if __name__ == "__main__":
    main()
