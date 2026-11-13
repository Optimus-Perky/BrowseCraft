#!/usr/bin/env python3
"""BrowseCraft's own version (2026-10-13: "a version number under about to signify what build it
is", and in the installer's file name). build/builds.log lists every version an installer was made
from, with its Chromium, which is how the next one is worked out.

From Chromium 158 (13 Nov 2026): v<major>.<minor>.<yymmdd>[letter]
  - major goes up one with each Chromium major: 158 is v1, 159 v2, ...
  - minor is 0 for a major's first build and goes up one with each Chromium point release in it
  - yymmdd is the day that version was first built
  - a build of ours on a Chromium already built (a fix, no new Chromium) keeps the number and adds
    a letter: v1.0.261120, then v1.0.261120a, v1.0.261120b ...
Up to Chromium 157: v<release>.<yymmdd>.<build of the day>, e.g. v0.8.261013.1 (release in VERSION).

  version.py next [chromium]           the version the next build gets (no chromium: the old scheme)
  version.py stamp <src> <ver>         put <ver> in the source tree (Settings > About shows it)
  version.py record <ver> [chromium]   add <ver> to builds.log (build-installer.sh does this)

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


FIRST_MAJOR = 158   # Chromium 158 is BrowseCraft v1
OLD = re.compile(r"\d+\.\d+\.\d{6}\.\d+")          # 0.8.261013.1
NEW = re.compile(r"(\d+)\.(\d+)\.(\d{6})([a-z]?)")   # 1.0.261120, 1.0.261120a


def entries():
    """(version, chromium or "") for every line of builds.log."""
    if not os.path.exists(LOG):
        return []
    out = []
    with open(LOG) as f:
        for line in f:
            p = line.split()
            if p and not line.startswith("#"):
                out.append((p[0], p[3] if len(p) > 3 else ""))
    return out


def recorded():
    return [v for v, _ in entries()]


def next_version(chromium="", today=None):
    day = (today or datetime.date.today()).strftime("%y%m%d")
    major = int(chromium.split(".")[0]) if chromium else 0
    if major < FIRST_MAJOR:
        prefix = f"{release()}.{day}."
        built = [int(v[len(prefix):]) for v in recorded() if v.startswith(prefix)]
        return f"{prefix}{max(built, default=0) + 1}"
    mine = []   # (minor, yymmdd, letter, chromium) of this major's builds
    for v, c in entries():
        m = NEW.fullmatch(v)
        if m and c and int(c.split(".")[0]) == major and int(m.group(1)) == major - FIRST_MAJOR + 1:
            mine.append((int(m.group(2)), m.group(3), m.group(4), c))
    same = [e for e in mine if e[3] == chromium]
    if same:   # this Chromium again: a fix of ours, the next letter on its number
        minor, first_day = same[0][0], same[0][1]
        letters = [e[2] for e in same if e[0] == minor and e[1] == first_day]
        last = max(letters)
        if last == "z":
            sys.exit(f"Chromium {chromium} has been built 27 times: no letter left")
        nxt = "a" if last == "" else chr(ord(last) + 1)
        return f"{major - FIRST_MAJOR + 1}.{minor}.{first_day}{nxt}"
    minor = max((e[0] for e in mine), default=-1) + 1
    return f"{major - FIRST_MAJOR + 1}.{minor}.{day}"


def check(ver):
    if not (OLD.fullmatch(ver) or NEW.fullmatch(ver)):
        sys.exit(f"not a BrowseCraft version: {ver} (want e.g. 1.0.261120, 1.0.261120a or 0.8.261013.1, no v)")


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
    # The release notes this build carries (the "What's new" tab), up to this version.
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import release_notes
    if os.path.isdir(os.path.join(src, "chrome/browser/browsecraft")):
        release_notes.bundle(src, ver)
    print(f"stamped v{ver} in {src}")


def record(ver, chromium=""):
    check(ver)
    if ver in recorded():
        return
    with open(LOG, "a") as f:
        f.write(f"{ver} {datetime.datetime.now():%Y-%m-%d %H:%M}{' ' + chromium if chromium else ''}\n")


if __name__ == "__main__":
    a = sys.argv[1:]
    if a and a[0] == "next" and len(a) <= 2:
        print(next_version(a[1] if len(a) == 2 else ""))
    elif len(a) == 3 and a[0] == "stamp":
        stamp(a[1], a[2])
    elif a and a[0] == "record" and len(a) in (2, 3):
        record(a[1], a[2] if len(a) == 3 else "")
    else:
        sys.exit(__doc__)
