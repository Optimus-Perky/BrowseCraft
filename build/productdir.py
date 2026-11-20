#!/usr/bin/env python3
"""BrowseCraft: its own profile folder, registry key and Windows app identity, so it never
shares a profile (or a running process) with an installed Chromium. Run in the src tree."""
import sys
EDITS = {
    "chrome/install_static/chromium_install_modes.h": [
        ('kProductPathName[] = L"Chromium";', 'kProductPathName[] = L"BrowseCraft";'),
        ('.base_app_name = L"Chromium",', '.base_app_name = L"BrowseCraft",'),
        ('.base_app_id = L"Chromium",', '.base_app_id = L"BrowseCraft",'),
        ('.browser_prog_id_prefix = L"ChromiumHTM",', '.browser_prog_id_prefix = L"BCraftHTM",'),
        ('L"Chromium HTML Document",', 'L"AltBrowse HTML Document",'),
        ('.direct_launch_url_scheme = "chromium",', '.direct_launch_url_scheme = "browsecraft",'),
        ('.pdf_prog_id_prefix = L"ChromiumPDF",', '.pdf_prog_id_prefix = L"BCraftPDF",'),
        ('L"Chromium PDF Document",', 'L"AltBrowse PDF Document",'),
    ],
    "chrome/common/chrome_paths_linux.cc": [
        ('  std::string data_dir_basename = "chromium";', '  std::string data_dir_basename = "browsecraft";'),
    ],
}
for path, pairs in EDITS.items():
    s = open(path).read()
    for old, new in pairs:
        if new in s:
            continue
        if s.count(old) != 1:
            sys.exit(f"{path}: expected one '{old}', found {s.count(old)}")
        s = s.replace(old, new)
    open(path, "w").write(s)
print("ok")
