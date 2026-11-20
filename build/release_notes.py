#!/usr/bin/env python3
"""AltBrowse's (BrowseCraft's until 0.8.261120.1) release notes: one JSON file per version in release-notes/, and the one renderer
for both places they are read (13 Nov 2026), so the two can never differ:
  - the "What's new" tab the browser opens after an update (bundled into the build, offline)
  - the change history page on the download site (built when a release is published)

  release_notes.py security <version>        fill the entry's security section from Google's
                                             Chrome Releases notes (chrome_releases.py)
  release_notes.py check                     every entry well-formed
  release_notes.py bundle <src> <version>    write the browser's bundle (version.py stamp does it)
  release_notes.py site <out dir> <version> <template page.html>
                                             the change history page: entries up to <version>
  release_notes.py draft <version> <chromium> <previous version> [<git range>]
                                             an entry for an automatic build: a Chromium update
                                             only, or (our own commits in the range) a draft
                                             listing them for review, never published as is

An entry (release-notes/<version>.json):
  {"version": "0.8.261113.1", "date": "2026-11-13", "chromium": "157.0.8103.50",
   "chromium_from": "155.0.8059.39",      (absent: first release, or Chromium unchanged)
   "new": ["..."], "fixed": ["..."],      (plain English, for users)
   "security": {...},                     (filled by "security"; null: Chromium unchanged)
   "draft": true}                         (only for an unreviewed draft: refused by site/bundle)
Security: {"source", "fixes", "highest", "in_the_wild", "scope": "range"|"exact",
           "releases": [{"chromium", "date", "notes", "fixes",
                         "items": [{"cve", "severity", "title", "in_the_wild"}]}]}
"""
import datetime
import glob
import html
import json
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
# BROWSECRAFT_RELEASE_NOTES: another checkout's notes (the autobuild publishes from its clone).
NOTES = os.environ.get("BROWSECRAFT_RELEASE_NOTES") or os.path.join(REPO, "release-notes")
CHANGES_URL = "https://altcraft.perkin.me.uk/browsecraft/changes/"
HEADER = "chrome/browser/browsecraft/browsecraft_release_notes.h"
BUNDLE_MAJORS = 1      # bundle every entry of the running Chromium major...
BUNDLE_LAST = 6        # ...and at least the last few versions before it
SEV_ORDER = ["critical", "high", "medium", "low"]

sys.path.insert(0, HERE)
from version import NEW, OLD  # noqa: E402


def vkey(v):
    """Sort key for both BrowseCraft schemes (0.8.yymmdd.n below 1.x; a letter above none)."""
    m = OLD.fullmatch(v)
    if m:
        a, b, d, n = (int(x) for x in v.split("."))
        return (a, b, d, n, 0)
    m = NEW.fullmatch(v)
    if m:
        return (int(m.group(1)), int(m.group(2)), int(m.group(3)), 0,
                ord(m.group(4)) - 96 if m.group(4) else 0)
    raise ValueError(f"not a BrowseCraft version: {v}")


RENAMED = "0.8.261120.1"  # the first version called AltBrowse (20 Nov 2026); earlier ones were BrowseCraft


def product(v):
    """The name a version shipped under."""
    return "AltBrowse" if vkey(v) >= vkey(RENAMED) else "BrowseCraft"


def load(version):
    with open(os.path.join(NOTES, f"{version}.json")) as f:
        return json.load(f)


def save(entry):
    path = os.path.join(NOTES, f"{entry['version']}.json")
    with open(path + ".tmp", "w") as f:
        json.dump(entry, f, indent=1, ensure_ascii=False)
        f.write("\n")
    os.replace(path + ".tmp", path)


def entries():
    out = [json.load(open(p)) for p in glob.glob(os.path.join(NOTES, "*.json"))]
    return sorted(out, key=lambda e: vkey(e["version"]), reverse=True)


def check_entry(e):
    errors = []
    for k in ("version", "date", "chromium"):
        if not isinstance(e.get(k), str) or not e[k]:
            errors.append(f"{k} missing")
    try:
        vkey(e.get("version", ""))
    except ValueError as x:
        errors.append(str(x))
    for k in ("new", "fixed"):
        if not isinstance(e.get(k, []), list) or not all(isinstance(s, str) for s in e.get(k, [])):
            errors.append(f"{k} must be a list of strings")
    if e.get("draft"):
        errors.append("still a draft (review it, then remove \"draft\")")
    return errors


# ---------------------------------------------------------------- security
def fill_security(entry):
    import chrome_releases
    if entry.get("chromium_from") and entry["chromium_from"] != entry["chromium"]:
        s = chrome_releases.between(entry["chromium_from"], entry["chromium"])
        scope = "range"
    elif entry.get("chromium_from") == entry["chromium"]:
        entry["security"] = None
        return entry
    else:   # a first release: the fixes Google listed for this Chromium itself
        posts = chrome_releases.fetch_posts(150)
        mine = [p for p in posts if entry["chromium"] in p["versions"]]
        s = chrome_releases.between("0.0.0.0", entry["chromium"], mine) if mine else \
            chrome_releases.unavailable(f"no post for Chromium {entry['chromium']}")
        scope = "exact"
    entry["security"] = {"source": s["source"], "fixes": s["fixes"], "highest": s["highest"],
                         "in_the_wild": s["in_the_wild"], "scope": scope,
                         "releases": s.get("releases", []),
                         "checked": datetime.date.today().isoformat()}
    return entry


# ---------------------------------------------------------------- rendering (both places)
def esc(s):
    return html.escape(str(s), quote=True)


def long_date(iso):
    d = datetime.date.fromisoformat(iso)
    return f"{d.day} {d.strftime('%B %Y')}"


def render(e):
    """One version as an HTML fragment: the tab and the site page both show exactly this."""
    v = e["version"]
    out = [f'<section class="release" id="v{esc(v)}">',
           f'<h2>{product(v)} v{esc(v)}</h2>']
    meta = f"{esc(long_date(e['date']))} · Chromium {esc(e['chromium'])}"
    if e.get("chromium_from") and e["chromium_from"] != e["chromium"]:
        meta += f" (updated from {esc(e['chromium_from'])})"
    elif e.get("chromium_from") == e["chromium"]:
        meta += " (unchanged)"
    out.append(f'<p class="meta">{meta}</p>')
    for key, title in (("new", "What's new"), ("fixed", "What's fixed")):
        items = e.get(key) or []
        if items:
            out.append(f"<h3>{title}</h3><ul>" + "".join(f"<li>{esc(i)}</li>" for i in items) + "</ul>")
    sec = e.get("security")
    if sec:
        out.append("<h3>Security fixes applied</h3>")
        if sec["source"] != "chrome-releases":
            out.append('<p class="wild">Chromium was updated, but Google\'s notes could not be read: '
                       "treat this as a security update.</p>")
        else:
            span = (f"in Chromium {esc(e['chromium'])}" if sec.get("scope") == "exact" else
                    f"in the Chromium releases from {esc(e['chromium_from'])} to {esc(e['chromium'])}")
            p = (f"{sec['fixes']} security fix{'es' if sec['fixes'] != 1 else ''} {span}, "
                 f"the most serious rated {esc(sec['highest'].title())}.")
            wild = sorted({i["cve"] for r in sec["releases"] for i in r["items"] if i.get("in_the_wild")})
            if wild:
                p += (f' <strong class="wild">{len(wild)} {"was" if len(wild) == 1 else "were"} already '
                      f'being exploited: {esc(", ".join(wild))}.</strong>')
            out.append(f"<p>{p}</p>")
            for r in sec["releases"]:
                n = r["fixes"]
                out.append(f'<details><summary>Chromium {esc(r["chromium"])}: {n} security '
                           f'fix{"es" if n != 1 else ""} · <a href="{esc(r["notes"])}" target="_blank" '
                           f'rel="noopener">Google\'s notes</a></summary><ul class="cves">')
                for i in sorted(r["items"], key=lambda i: (not i.get("in_the_wild"),
                                SEV_ORDER.index(i["severity"]) if i["severity"] in SEV_ORDER else 9)):
                    flag = ' <strong class="wild">exploited in the wild</strong>' if i.get("in_the_wild") else ""
                    out.append(f'<li><span class="sev {esc(i["severity"])}">{esc(i["severity"].title())}</span> '
                               f'{esc(i["cve"])}: {esc(i["title"])}{flag}</li>')
                if n > len(r["items"]):
                    out.append(f'<li class="more">and {n - len(r["items"])} more that Google does not list</li>')
                out.append("</ul></details>")
    out.append("</section>")
    return "\n".join(out)


# ---------------------------------------------------------------- the browser's bundle
def chromium_major(e):
    return int(e["chromium"].split(".")[0])


def bundled(version):
    """Entries the build carries: every one of the running Chromium major, at least the last
    BUNDLE_LAST, none newer than the build itself, no drafts."""
    upto = [e for e in entries() if vkey(e["version"]) <= vkey(version) and not e.get("draft")]
    if not upto:
        return []
    major = chromium_major(upto[0])
    return [e for i, e in enumerate(upto) if chromium_major(e) > major - BUNDLE_MAJORS or i < BUNDLE_LAST]


def bundle(src, version):
    notes = bundled(version)
    lines = ["// Generated by build/release_notes.py bundle: do not edit.",
             "#ifndef CHROME_BROWSER_BROWSECRAFT_BROWSECRAFT_RELEASE_NOTES_H_",
             "#define CHROME_BROWSER_BROWSECRAFT_BROWSECRAFT_RELEASE_NOTES_H_",
             "", "namespace browsecraft::notes {", "",
             "struct Note {", "  const char* version;", "  const char* chromium;", "  const char* html;", "};", "",
             f'inline constexpr char kChangesUrl[] = "{CHANGES_URL}";', "",
             "// Newest first.", "inline constexpr Note kNotes[] = {"]
    for e in notes:
        frag = render(e)
        assert ")BCRN" not in frag
        lines.append(f'    {{"{e["version"]}", "{e["chromium"]}", R"BCRN({frag})BCRN"}},')
    if not notes:
        lines.append('    {"", "", ""},')
    lines += ["};", "", "}  // namespace browsecraft::notes", "",
              "#endif  // CHROME_BROWSER_BROWSECRAFT_BROWSECRAFT_RELEASE_NOTES_H_", ""]
    text = "\n".join(lines)
    path = os.path.join(src, HEADER)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    if not os.path.exists(path) or open(path).read() != text:
        open(path, "w").write(text)
    print(f"release notes bundled: {', '.join(e['version'] for e in notes) or 'none'}")


# ---------------------------------------------------------------- the site page
SITE_CSS = """.release { background: var(--panel); border: 1px solid var(--line); border-radius: 12px;
  padding: 4px 20px 14px; margin: 18px 0; }
.release h2 { margin: 16px 0 2px; }
.release h3 { font-size: 1rem; margin: 16px 0 4px; }
.release .meta { color: var(--muted); margin: 0 0 6px; font-size: .95rem; }
.release details { margin: 6px 0; }
.release summary { cursor: pointer; }
.release ul.cves { font-size: .92rem; }
.release .sev { display: inline-block; min-width: 4.6em; font-weight: 600; font-size: .85em; }
.release .sev.critical { color: #b3261e; } .release .sev.high { color: #c4561a; }
.release .sev.medium { color: #8a6d00; } .release .sev.low { color: var(--muted); }
.release .wild { color: #b3261e; }
.release .more { color: var(--muted); list-style: none; }
@media (prefers-color-scheme: dark) {
  .release .sev.critical, .release .wild { color: #ff8a80; } .release .sev.high { color: #ffab70; }
  .release .sev.medium { color: #e6c35c; }
}
"""


def site(out_dir, version, template):
    t = open(template).read()
    head, rest = t.split('<main class="wrap">', 1)
    foot = rest.split("</main>", 1)[1]
    head = re.sub(r"<title>[^<]*</title>", "<title>AltBrowse change history - AltCraft</title>", head)
    head = re.sub(r'<meta name="description" content="[^"]*">',
                  '<meta name="description" content="Every AltBrowse (formerly BrowseCraft) version: what is new, what was '
                  'fixed and the security fixes applied.">', head)
    head = head.replace('<link rel="stylesheet" href="/style.css">',
                        '<link rel="stylesheet" href="/style.css">\n'
                        '<link rel="stylesheet" href="/browsecraft/changes/changes.css">')
    shown = [e for e in entries() if vkey(e["version"]) <= vkey(version)]
    drafts = [e["version"] for e in shown if e.get("draft")]
    if drafts:
        sys.exit(f"draft release notes not reviewed: {', '.join(drafts)}")
    body = ['<main class="wrap">', '<section class="page">',
            '<h1><img src="/img/browsecraft-logo.svg" alt="">AltBrowse change history</h1>',
            '<p class="lede">Every AltBrowse version (called BrowseCraft until v0.8.261120.1), newest first: '
            'what is new, what was fixed, and the Chromium security fixes it brought. '
            '<a href="/altbrowse/">Back to AltBrowse</a></p>']
    body += [render(e) for e in shown]
    body += ["</section>", "</main>"]
    os.makedirs(out_dir, exist_ok=True)
    open(os.path.join(out_dir, "index.html"), "w").write(head + "\n".join(body) + foot)
    open(os.path.join(out_dir, "changes.css"), "w").write(SITE_CSS)
    print(f"change history: {len(shown)} versions, newest v{shown[0]['version'] if shown else '-'}")


# ---------------------------------------------------------------- drafts for the autobuild
OWN_PATHS = ("build/", "patches/", "packaging/")
NOT_OURS = ("build/builds.log",)


def draft(version, chromium, previous, git_range=""):
    prev = load(previous) if os.path.exists(os.path.join(NOTES, f"{previous}.json")) else {}
    e = {"version": version, "date": datetime.date.today().isoformat(), "chromium": chromium,
         "chromium_from": prev.get("chromium", ""), "new": [], "fixed": []}
    if not e["chromium_from"]:
        del e["chromium_from"]
    subjects = []
    if git_range:
        log = subprocess.run(["git", "-C", REPO, "log", "--format=%s", git_range, "--",
                              *OWN_PATHS, *(f":!{p}" for p in NOT_OURS)],
                             capture_output=True, text=True).stdout.split("\n")
        subjects = [s for s in log if s and not s.startswith(("Merge ", "autobuild:", "builds.log"))]
    if e.get("chromium_from") and e["chromium_from"] != chromium:
        e["new"].append(f"Chromium updated to {chromium}.")
    if subjects:   # our own changes: a person writes these for users
        e["draft"] = True
        e["commits_for_review"] = subjects
    fill_security(e)
    save(e)
    return e


if __name__ == "__main__":
    a = sys.argv[1:]
    if a[:1] == ["security"] and len(a) == 2:
        save(fill_security(load(a[1])))
        print(f"{a[1]}: security filled")
    elif a[:1] == ["check"]:
        bad = 0
        for e in entries():
            for err in check_entry(e):
                print(f"{e.get('version')}: {err}")
                bad += 1
        sys.exit(1 if bad else 0)
    elif a[:1] == ["bundle"] and len(a) == 3:
        bundle(a[1], a[2])
    elif a[:1] == ["site"] and len(a) == 4:
        site(a[1], a[2], a[3])
    elif a[:1] == ["draft"] and len(a) in (4, 5):
        print(json.dumps(draft(*a[1:]), indent=1)[:2000])
    elif a[:1] == ["render"] and len(a) == 2:
        print(render(load(a[1])))
    else:
        sys.exit(__doc__)
