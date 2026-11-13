#!/usr/bin/env python3
"""BrowseCraft: installing and updating extensions from the Chrome Web Store, as in Chrome.

ungoogled-chromium points every Web Store address at an unreachable domain (domain
substitution) and its disable-webstore-urls.patch stops extension update checks. This puts back
the store's addresses, its install API, update checks and the store's own process. Its other
changes stay (no Google account credentials or "daystart" pings on update checks).

Run in the src tree AFTER domain substitution (the addresses are written out in full here)."""
import sys

STORE = "https://chromewebstore.google.com/"
EDITS = {
    "extensions/common/extension_urls.cc": [
        ('kChromeWebstoreBaseURL[] = "https://chrome.9oo91e.qjz9zk/webstore";',
         'kChromeWebstoreBaseURL[] = "https://chrome.google.com/webstore";'),
        ('kNewChromeWebstoreBaseURL[] = "https://chromewebstore.9oo91e.qjz9zk/";',
         f'kNewChromeWebstoreBaseURL[] = "{STORE}";'),
        ('"https://clients2.9oo91e.qjz9zk/service/update2/crx";',
         '"https://clients2.google.com/service/update2/crx";'),
        ('kChromeWebstoreApiURL[] = "https://chromewebstore.9oo91eapis.qjz9zk/";',
         'kChromeWebstoreApiURL[] = "https://chromewebstore.googleapis.com/";'),
    ],
    # The store's page may use chrome.webstorePrivate and chrome.management (to install).
    "extensions/common/api/_api_features.json": [
        ('"matches": ["https://chromewebstore.9oo91e.qjz9zk/*"]', f'"matches": ["{STORE}*"]', 2),
    ],
    "chrome/browser/resources/webstore_app/manifest.json": [
        ('"https://chrome.9oo91e.qjz9zk/webstore"', '"https://chrome.google.com/webstore"', 2),
    ],
    # Undo disable-webstore-urls.patch where it matters.
    "chrome/browser/extensions/chrome_content_browser_client_extensions_part.cc": [
        ("""    GetOriginsRequiringDedicatedProcess() {
  std::vector<url::Origin> list;

  return list;""",
         """    GetOriginsRequiringDedicatedProcess() {
  std::vector<url::Origin> list;

  // Require a dedicated process for the webstore origin.  See
  // https://crbug.com/40094229.
  list.push_back(url::Origin::Create(extension_urls::GetWebstoreLaunchURL()));
  list.push_back(
      url::Origin::Create(extension_urls::GetNewWebstoreLaunchURL()));

  return list;"""),
    ],
    "chrome/browser/extensions/chrome_content_verifier_delegate.cc": [
        ("""  if (!InstallVerifier::IsFromStore(extension, context_)) {
    return false;
  }""",
         """  if (!InstallVerifier::IsFromStore(extension, context_)) {
    // It's possible that the webstore update url was overridden for testing
    // so also consider extensions with the default (production) update url
    // to be from the store as well.
    ExtensionManagement* extension_management =
        ExtensionManagementFactory::GetForBrowserContext(context_);
    if (extension_management->GetEffectiveUpdateURL(extension) !=
        extension_urls::GetDefaultWebstoreUpdateUrl()) {
      return false;
    }
  }"""),
    ],
    "extensions/browser/updater/extension_downloader.cc": [
        ("""  if (update_url.is_empty()) {
    // Fill in default update URL.
    return std::nullopt;
  }""",
         """  if (update_url.is_empty()) {
    // Fill in default update URL.
    return extension_urls::GetWebstoreUpdateUrl();
  }"""),
        ("""    return std::nullopt;
  }


  return update_url;""",
         """    return std::nullopt;
  }

  // Make sure we use SSL for store-hosted extensions.
  if (extension_urls::IsWebstoreUpdateUrl(update_url) &&
      !update_url.SchemeIsCryptographic()) {
    return extension_urls::GetWebstoreUpdateUrl();
  }

  return update_url;"""),
        ("""    AddToFetches(fetches_preparing, std::move(task));
  pending_tasks_.clear();

}""",
         """    AddToFetches(fetches_preparing, std::move(task));
  pending_tasks_.clear();

  for (auto& fetch_list : fetches_preparing) {
    for (auto& fetch : fetch_list.second)
      StartUpdateCheck(std::move(fetch));
  }
}"""),
    ],
    # ungoogled's inox 0005-disable-default-extensions.patch empties these two, so the store's
    # "complete install" never downloads or installs anything.
    "extensions/browser/webstore_installer.cc": [
        # From 157 the same patch also deletes the download folder's name, which DownloadCrx uses.
        ("""constexpr base::TimeDelta kTimeRemainingThreshold = base::Seconds(1);

base::FilePath* g_download_directory_for_tests = nullptr;""",
         """constexpr base::TimeDelta kTimeRemainingThreshold = base::Seconds(1);

// Folder for downloading crx files from the webstore. This is used so that the
// crx files don't go via the usual downloads folder.
const base::FilePath::CharType kWebstoreDownloadFolder[] =
    FILE_PATH_LITERAL("Webstore Downloads");

base::FilePath* g_download_directory_for_tests = nullptr;"""),
        ("""void WebstoreInstaller::DownloadCrx(const ExtensionId& extension_id,
                                    InstallSource source) {
}""",
         """void WebstoreInstaller::DownloadCrx(const ExtensionId& extension_id,
                                    InstallSource source) {
  download_url_ = GetWebstoreInstallURL(extension_id, source);
  MaybeAppendAuthUserParameter(approval_->authuser, download_url_);

  base::FilePath user_data_dir =
      ExtensionsBrowserClient::Get()->GetUserDataDir();
  base::FilePath download_path = user_data_dir.Append(kWebstoreDownloadFolder);

  base::FilePath download_directory(g_download_directory_for_tests
                                        ? *g_download_directory_for_tests
                                        : download_path);

  GetExtensionFileTaskRunner()->PostTaskAndReplyWithResult(
      FROM_HERE,
      base::BindOnce(&GetDownloadFilePath, download_directory, extension_id),
      base::BindOnce(&WebstoreInstaller::StartDownload, this, extension_id));
}"""),
        ("""  DCHECK_CURRENTLY_ON(BrowserThread::UI);
  DCHECK(!crx_installer_.get());
}""",
         """  DCHECK_CURRENTLY_ON(BrowserThread::UI);
  DCHECK(!crx_installer_.get());

  const InstallApproval* approval = GetAssociatedApproval(download);
  DCHECK(approval);

  crx_installer_ =
      ExtensionsBrowserClient::Get()->CreateCrxInstallerFromDownloadItem(
          browser_context_, download);

  crx_installer_->set_expected_id(approval->extension_id);
  crx_installer_->set_is_gallery_install(true);
  crx_installer_->set_allow_silent_install(true);
  crx_installer_->AddInstallerCallback(base::BindOnce(
      &WebstoreInstaller::OnInstallerDone, weak_ptr_factory_.GetWeakPtr()));
  if (approval->withhold_permissions) {
    crx_installer_->set_withhold_permissions();
  }

  crx_installer_->InstallCrx(download.GetFullPath());
}"""),
    ],
}


for path, edits in EDITS.items():
    s = open(path).read()
    for e in edits:
        old, new, n = (e + (1,))[:3]
        if old not in s and new in s:
            continue
        if s.count(old) != n:
            sys.exit(f"{path}: expected {n} of:\n{old}\nfound {s.count(old)}")
        s = s.replace(old, new)
    open(path, "w").write(s)
print("ok")
