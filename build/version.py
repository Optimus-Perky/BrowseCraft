#!/usr/bin/env python3
"""BrowseCraft's own version: v<release>.<yymmdd>.<build of the day>, e.g. v0.8.261013.1 (
2026-10-13: "a version number under about to signify what build it is", and in the installer's
file name). The release is in VERSION at the top of the repo; build/builds.log lists every
version an installer was made from, which is how the next build of the day is counted.

  version.py next                 the version the next build gets (today's date)
  version.py stamp <src> <ver>    put <ver> in the source tree (Settings > About shows it)
  version.py record <ver>         add <ver> to builds.log (build-installer.sh does this)

The stamp is a one-line header included only by the About page's strings, so a new version
recompiles one file and relinks; it is rewritten only when it changes. Run stamp in the src tree
after the other BrowseCraft edits (profile.py)."""
import datetime
import os
import re
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LOG = os.path.join(REPO, "build", "builds.log")
HEADER = "chrome/browser/ui/webui/settings/browsecraft_version.h"
STRINGS = "chrome/browser/ui/webui/settings/settings_localized_strings_provider.cc"

EDITS = [
    ('#include "base/strings/utf_string_conversions.h"\n',
     '#include "base/strings/utf_string_conversions.h"\n'
     '#include "base/strings/strcat.h"\n'
     '#include "chrome/browser/ui/webui/settings/browsecraft_version.h"\n'
     '#include "components/version_info/version_info.h"\n'),
    ("  std::u16string browser_version = VersionUI::GetAnnotatedVersionStringForUi();\n",
     "  // BrowseCraft: its own build, then the Chromium it is made from.\n"
     "  std::u16string browser_version = base::StrCat(\n"
     "      {u\"Version v\", kBrowseCraftVersion, u\" (Chromium \",\n"
     "       base::UTF8ToUTF16(version_info::GetVersionNumber()), u\")\"});\n"),
]


def release():
    with open(os.path.join(REPO, "VERSION")) as f:
        return f.read().strip()


def recorded():
    if not os.path.exists(LOG):
        return []
    with open(LOG) as f:
        return [line.split()[0] for line in f if line.strip() and not line.startswith("#")]


def next_version():
    day = datetime.date.today().strftime("%y%m%d")
    prefix = f"{release()}.{day}."
    built = [int(v[len(prefix):]) for v in recorded() if v.startswith(prefix)]
    return f"{prefix}{max(built, default=0) + 1}"


def check(ver):
    if not re.fullmatch(r"\d+\.\d+\.\d{6}\.\d+", ver):
        sys.exit(f"not a BrowseCraft version: {ver} (want e.g. 0.8.261013.1, no v)")


def stamp(src, ver):
    check(ver)
    path = os.path.join(src, STRINGS)
    with open(path) as f:
        text = f.read()
    if "kBrowseCraftVersion" not in text:
        for old, new in EDITS:
            if text.count(old) != 1:
                sys.exit(f"{STRINGS}: expected one {old.strip()!r}")
            text = text.replace(old, new)
        with open(path, "w") as f:
            f.write(text)
    # The first stamp (2026-10-13) converted an 8-bit string, which Chromium forbids for literals.
    if "base::ASCIIToUTF16(kBrowseCraftVersion)" in text:
        with open(path, "w") as f:
            f.write(text.replace("base::ASCIIToUTF16(kBrowseCraftVersion)", "kBrowseCraftVersion"))
    header = (f"// BrowseCraft's version, written by build/version.py.\n"
              f"#ifndef CHROME_BROWSER_UI_WEBUI_SETTINGS_BROWSECRAFT_VERSION_H_\n"
              f"#define CHROME_BROWSER_UI_WEBUI_SETTINGS_BROWSECRAFT_VERSION_H_\n"
              f"inline constexpr char16_t kBrowseCraftVersion[] = u\"{ver}\";\n"
              f"#endif  // CHROME_BROWSER_UI_WEBUI_SETTINGS_BROWSECRAFT_VERSION_H_\n")
    hpath = os.path.join(src, HEADER)
    if not os.path.exists(hpath) or open(hpath).read() != header:
        with open(hpath, "w") as f:
            f.write(header)
    print(f"stamped v{ver} in {src}")


def record(ver):
    check(ver)
    if ver in recorded():
        return
    with open(LOG, "a") as f:
        f.write(f"{ver} {datetime.datetime.now():%Y-%m-%d %H:%M}\n")


if __name__ == "__main__":
    a = sys.argv[1:]
    if a == ["next"]:
        print(next_version())
    elif len(a) == 3 and a[0] == "stamp":
        stamp(a[1], a[2])
    elif len(a) == 2 and a[0] == "record":
        record(a[1])
    else:
        sys.exit(__doc__)
