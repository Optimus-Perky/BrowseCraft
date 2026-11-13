// Copyright 2026 The BrowseCraft Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// chrome://browsecraft's update parts: the Updates section of the main page,
// chrome://browsecraft/update (a newer version, security details, Download,
// "Skip this version") and chrome://browsecraft/whatsnew[/<from>].

#ifndef CHROME_BROWSER_UI_WEBUI_BROWSECRAFT_BROWSECRAFT_UPDATE_PAGE_H_
#define CHROME_BROWSER_UI_WEBUI_BROWSECRAFT_BROWSECRAFT_UPDATE_PAGE_H_

#include <memory>
#include <string>

namespace content {
class WebUIMessageHandler;
}

namespace browsecraft::update_page {

// The Updates section, for the main page's HTML, and its extra CSS.
extern const char kSection[];
extern const char kStyle[];

bool Handles(const std::string& path);
std::string Serve(const std::string& path);
std::unique_ptr<content::WebUIMessageHandler> CreateHandler();

}  // namespace browsecraft::update_page

#endif  // CHROME_BROWSER_UI_WEBUI_BROWSECRAFT_BROWSECRAFT_UPDATE_PAGE_H_
