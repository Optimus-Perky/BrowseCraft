// Copyright 2026 The BrowseCraft Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#ifndef CHROME_BROWSER_UI_WEBUI_BROWSECRAFT_BROWSECRAFT_UI_H_
#define CHROME_BROWSER_UI_WEBUI_BROWSECRAFT_BROWSECRAFT_UI_H_

#include "content/public/browser/web_ui_controller.h"
#include "content/public/browser/webui_config.h"
#include "content/public/common/url_constants.h"

class BrowseCraftUI;

inline constexpr char kChromeUIBrowseCraftHost[] = "browsecraft";

class BrowseCraftUIConfig : public content::DefaultWebUIConfig<BrowseCraftUI> {
 public:
  BrowseCraftUIConfig()
      : DefaultWebUIConfig(content::kChromeUIScheme, kChromeUIBrowseCraftHost) {}
};

// chrome://browsecraft: signing in to your own BrowseCraft sync server.
class BrowseCraftUI : public content::WebUIController {
 public:
  explicit BrowseCraftUI(content::WebUI* web_ui);
  BrowseCraftUI(const BrowseCraftUI&) = delete;
  BrowseCraftUI& operator=(const BrowseCraftUI&) = delete;
  ~BrowseCraftUI() override;

  WEB_UI_CONTROLLER_TYPE_DECL();
};

#endif  // CHROME_BROWSER_UI_WEBUI_BROWSECRAFT_BROWSECRAFT_UI_H_
