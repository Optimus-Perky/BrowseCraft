#!/usr/bin/env python3
"""Security fixes between two Chromium versions, from Google's Chrome Releases notes
(https://chromereleases.googleblog.com, the "Stable updates" feed). Used for latest.json (is
this update a security update?) and for the release notes the browser shows after an update.

  chrome_releases.py <from chromium> <to chromium>      JSON for releases in (from, to]
  chrome_releases.py --post <file.html|json>             parse one saved post (tests)

Each desktop "Stable Channel Update" post names its versions ("157.0.8103.78/.79 for Windows and
Mac and 157.0.8103.78 to Linux"), says "This update includes N security fixes", lists them as
"[$reward][bug] Severity CVE-yyyy-n: Title. Reported by ...", and, when one is being exploited,
"Google is aware that an exploit for CVE-yyyy-n exists in the wild". A post belongs to the range
when its lowest version is in (from, to]. Anything that cannot be found or read makes the result
"unavailable", and an unavailable answer counts as a security update (err towards warning).
"""
import html
import json
import re
import sys
import urllib.request

FEED = ("https://chromereleases.googleblog.com/feeds/posts/default/-/Stable%20updates"
        "?alt=json&max-results={n}")
UA = {"User-Agent": "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko)"}
SEVERITIES = ["critical", "high", "medium", "low"]
VERSION = re.compile(r"\b(\d{2,4}\.\d+\.\d{3,5}\.\d+)((?:/\.\d+)*)")
ITEM = re.compile(r"\b(Critical|High|Medium|Low)\s+(CVE-\d{4}-\d{3,7})\s*:\s*(.+?)"
                  r"(?=\.\s*Reported by|\s*\[(?:\$|N/A|TBD)|\s*Google is aware|\s*We would also|$)",
                  re.S)
COUNT = re.compile(r"includes\s*([\d,]+)\s*security\s*fix", re.I)
WILD = re.compile(r"[Ee]xploits?\s+for\s+((?:CVE-\d{4}-\d+(?:,\s*|\s+and\s+)?)+)\s+exists?\s+in\s+the\s+wild")


def vkey(v):
    return tuple(int(x) for x in v.split("."))


def text_of(content):
    """Visible text, tags as spaces (for sentences) and tags removed (for split numbers)."""
    spaced = re.sub(r"\s+", " ", html.unescape(re.sub(r"<[^>]+>", " ", content)))
    joined = re.sub(r"\s+", " ", html.unescape(re.sub(r"<[^>]+>", "", content)))
    return spaced, joined


def parse_post(content):
    """One post's versions and security fixes."""
    spaced, joined = text_of(content)
    head = spaced[:700]
    versions = set()
    for base, suffixes in VERSION.findall(head):
        versions.add(base)
        stem = base.rsplit(".", 1)[0]
        for s in re.findall(r"/\.(\d+)", suffixes):
            versions.add(f"{stem}.{s}")
    items, seen = [], set()
    for sev, cve, title in ITEM.findall(spaced):
        if cve in seen:
            continue
        seen.add(cve)
        items.append({"cve": cve, "severity": sev.lower(), "title": title.strip()[:160]})
    wild = set()
    for group in WILD.findall(spaced):
        wild.update(re.findall(r"CVE-\d{4}-\d+", group))
    for it in items:
        it["in_the_wild"] = it["cve"] in wild
    m = COUNT.search(joined)
    stated = int(m.group(1).replace(",", "")) if m else 0
    return {"versions": sorted(versions, key=vkey), "fixes": max(stated, len(items)),
            "stated_fixes": stated, "items": items, "in_the_wild": sorted(wild)}


def fetch_posts(n=100):
    data = json.load(urllib.request.urlopen(urllib.request.Request(FEED.format(n=n), headers=UA),
                                            timeout=30))
    posts = []
    for e in data.get("feed", {}).get("entry", []):
        title = e.get("title", {}).get("$t", "")
        if "Desktop" not in title:   # Android, iOS, ChromeOS posts are other builds
            continue
        url = next((l["href"] for l in e.get("link", []) if l.get("rel") == "alternate"), "")
        p = parse_post(e.get("content", {}).get("$t", ""))
        p.update(title=title, url=url, date=e.get("published", {}).get("$t", "")[:10])
        if p["versions"]:
            posts.append(p)
    return posts


def unavailable(why):
    return {"update": True, "fixes": 0, "highest": "unknown", "in_the_wild": False,
            "summary": "Security details unavailable: treat this as a security update.",
            "source": "unavailable", "references": [], "why": why, "releases": []}


def between(old, new, posts=None):
    """Security fixes in Chromium releases after |old| up to |new|."""
    try:
        posts = fetch_posts() if posts is None else posts
    except Exception as e:   # no network, feed changed: warn rather than stay quiet
        return unavailable(f"Chrome Releases feed unreadable: {e.__class__.__name__}")
    picked = [p for p in posts if vkey(old) < vkey(p["versions"][0]) <= vkey(new)]
    picked.sort(key=lambda p: vkey(p["versions"][0]), reverse=True)
    if not picked:
        return unavailable(f"no Chrome Releases post found for Chromium {old} -> {new}")
    # A platform respin (157.0.8103.80 after "157.0.8103.78/.79") often has no post of its own:
    # the posts in the range still say what it fixes; the result notes the gap.
    note = "" if any(new in p["versions"] for p in picked) else f"no post names {new} itself"
    fixes = sum(p["fixes"] for p in picked)
    sev = [it["severity"] for p in picked for it in p["items"]]
    highest = next((s for s in SEVERITIES if s in sev), "unknown")
    wild = sorted({c for p in picked for c in p["in_the_wild"]})
    if fixes == 0:
        return {"update": False, "fixes": 0, "highest": "unknown", "in_the_wild": False,
                "summary": f"Chromium {new}: no security fixes.", "source": "chrome-releases",
                "references": [p["url"] for p in picked if p["url"]], "releases": []}
    summary = f"Chromium {new}: {fixes} security fix{'es' if fixes != 1 else ''}, the most serious rated {highest.title()}."
    if wild:
        summary += (f" {len(wild)} {'is' if len(wild) == 1 else 'are'} already being exploited"
                    f" ({', '.join(wild)}).")
    releases = [{"chromium": p["versions"][0], "date": p["date"], "notes": p["url"],
                 "fixes": p["fixes"], "items": p["items"]} for p in picked]
    return {"update": True, "fixes": fixes, "highest": highest, "in_the_wild": bool(wild),
            "summary": summary[:400], "source": "chrome-releases", "note": note,
            "references": [p["url"] for p in picked if p["url"]][:20], "releases": releases}


if __name__ == "__main__":
    a = sys.argv[1:]
    if a and a[0] == "--post":
        raw = open(a[1]).read()
        print(json.dumps(parse_post(raw), indent=1))
    elif len(a) == 2:
        r = between(a[0], a[1])
        if "--brief" not in sys.argv:
            print(json.dumps(r, indent=1))
    else:
        sys.exit(__doc__)
