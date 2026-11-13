#!/usr/bin/env python3
"""Tests for chrome_releases.py, release_notes.py and the autobuild's latest_json.py, on real
Chrome Releases posts (testdata/, saved 13 Nov 2026) and made-up entries. Each test has a control.
  python3 build/tests/test_release_notes.py"""
import json
import os
import shutil
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
BUILD = os.path.dirname(HERE)
sys.path.insert(0, BUILD)
sys.path.insert(0, os.path.join(os.path.dirname(BUILD), "autobuild", "publish"))
import chrome_releases  # noqa: E402

DATA = os.path.join(HERE, "testdata")


def post(name):
    return open(os.path.join(DATA, name)).read()


class ChromeReleases(unittest.TestCase):
    def test_reads_a_post_with_an_exploit(self):
        p = chrome_releases.parse_post(post("chrome-releases-157.0.8103.50.html"))
        self.assertIn("157.0.8103.50", p["versions"])
        self.assertIn("157.0.8103.51", p["versions"])   # "157.0.8103.50/.51 for Windows and Mac"
        self.assertEqual(p["stated_fixes"], 9)
        self.assertEqual(p["fixes"], 9)
        self.assertEqual(p["in_the_wild"], ["CVE-2026-125350"])
        wild = [i for i in p["items"] if i["in_the_wild"]]
        self.assertEqual([(i["cve"], i["severity"], i["title"]) for i in wild],
                         [("CVE-2026-125350", "high", "Use after free in XML")])
        # Control: the other fixes are not flagged.
        self.assertTrue(all(not i["in_the_wild"] for i in p["items"] if i["cve"] != "CVE-2026-125350"))

    def test_count_split_by_markup(self):
        # The post's HTML splits "212" in two ("21 2" as text): the count reads it whole.
        p = chrome_releases.parse_post(post("chrome-releases-157.0.8103.38-head.html"))
        self.assertEqual(p["stated_fixes"], 212)
        self.assertEqual(p["versions"][0], "157.0.8103.38")

    def test_range_and_fallbacks(self):
        posts = []
        for f, url in (("chrome-releases-157.0.8103.50.html", "https://x/50"),
                       ("chrome-releases-157.0.8103.38-head.html", "https://x/38")):
            p = chrome_releases.parse_post(post(f))
            p.update(url=url, date="2026-11-0x", title="Stable Channel Update for Desktop")
            posts.append(p)
        r = chrome_releases.between("157.0.8103.38", "157.0.8103.50", posts)
        self.assertTrue(r["update"])
        self.assertEqual(r["fixes"], 9)                 # only the .50 post is in (.38, .50]
        self.assertTrue(r["in_the_wild"])
        self.assertEqual(r["source"], "chrome-releases")
        r = chrome_releases.between("156.0.8078.79", "157.0.8103.50", posts)
        self.assertEqual(r["fixes"], 9 + 212)
        # A respin no post names: the posts in the range count (.50's fixes, up to .51).
        r = chrome_releases.between("157.0.8103.38", "157.0.8103.51", posts)
        self.assertEqual((r["source"], r["fixes"]), ("chrome-releases", 9))
        self.assertEqual(r["note"], "")                  # .51 is named in the .50 post
        r = chrome_releases.between("157.0.8103.38", "157.0.8103.60", posts)
        self.assertEqual((r["source"], r["fixes"]), ("chrome-releases", 9))
        self.assertIn("no post names 157.0.8103.60", r["note"])
        # Controls: nothing in the range, and no posts at all, are security updates anyway.
        r = chrome_releases.between("157.0.8103.50", "157.0.8103.80", posts)
        self.assertTrue(r["update"])
        self.assertEqual(r["source"], "unavailable")
        r = chrome_releases.between("157.0.8103.38", "157.0.8103.50", [])
        self.assertEqual(r["source"], "unavailable")
        self.assertTrue(r["update"])


class ReleaseNotes(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()
        os.environ["BROWSECRAFT_RELEASE_NOTES"] = self.dir
        for m in ("release_notes", "latest_json"):
            sys.modules.pop(m, None)
        import release_notes
        self.rn = release_notes
        sec = {"source": "chrome-releases", "fixes": 2, "highest": "high", "in_the_wild": True,
               "scope": "range", "releases": [{"chromium": "157.0.8103.50", "date": "2026-11-09",
                    "notes": "https://chromereleases.googleblog.com/x.html", "fixes": 2, "items": [
                        {"cve": "CVE-2026-1", "severity": "high", "title": "Use after <free>", "in_the_wild": True},
                        {"cve": "CVE-2026-2", "severity": "low", "title": "Thing", "in_the_wild": False}]}]}
        for e in [
            {"version": "0.8.261021.1", "date": "2026-10-21", "chromium": "155.0.8059.39", "new": [], "fixed": ["a"]},
            {"version": "0.8.261113.1", "date": "2026-11-13", "chromium": "157.0.8103.50",
             "chromium_from": "155.0.8059.39", "new": ["Chromium 157 & <more>"], "fixed": [], "security": sec},
            {"version": "0.8.261113.2", "date": "2026-11-13", "chromium": "157.0.8103.50",
             "chromium_from": "157.0.8103.50", "new": ["update check"], "fixed": [], "security": None},
            {"version": "1.0.261120", "date": "2026-11-20", "chromium": "158.0.8127.37",
             "chromium_from": "157.0.8103.50", "new": ["158"], "fixed": [], "draft": True}]:
            with open(os.path.join(self.dir, e["version"] + ".json"), "w") as f:
                json.dump(e, f)

    def tearDown(self):
        shutil.rmtree(self.dir)
        os.environ.pop("BROWSECRAFT_RELEASE_NOTES", None)

    def test_order_both_schemes(self):
        k = self.rn.vkey
        self.assertLess(k("0.8.291231.9"), k("1.0.261120"))
        self.assertLess(k("1.0.261120"), k("1.0.261120a"))
        self.assertLess(k("0.8.261113.1"), k("0.8.261113.2"))
        with self.assertRaises(ValueError):   # control
            k("157.0.8103.50")

    def test_render_escapes_and_flags(self):
        html = self.rn.render(self.rn.load("0.8.261113.1"))
        self.assertIn('id="v0.8.261113.1"', html)
        self.assertIn("Chromium 157 &amp; &lt;more&gt;", html)
        self.assertIn("Use after &lt;free&gt;", html)
        self.assertNotIn("<free>", html)                       # control: never raw
        self.assertIn("exploited in the wild", html)
        self.assertIn("updated from 155.0.8059.39", html)
        self.assertNotIn("Security fixes applied", self.rn.render(self.rn.load("0.8.261113.2")))

    def test_bundle_and_site_never_show_drafts_or_newer(self):
        self.assertEqual([e["version"] for e in self.rn.bundled("0.8.261113.2")],
                         ["0.8.261113.2", "0.8.261113.1", "0.8.261021.1"])
        self.assertEqual([e["version"] for e in self.rn.bundled("0.8.261113.1")],
                         ["0.8.261113.1", "0.8.261021.1"])     # nothing newer than the build
        self.assertEqual([e["version"] for e in self.rn.bundled("1.0.261120")],
                         ["0.8.261113.2", "0.8.261113.1", "0.8.261021.1"])   # the draft is left out
        t = os.path.join(self.dir, "t.html")
        with open(t, "w") as f:
            f.write('<html><head><title>x</title></head><body><main class="wrap">old</main>f</body></html>')
        out = os.path.join(self.dir, "site")
        self.rn.site(out, "0.8.261113.1", t)
        page = open(os.path.join(out, "index.html")).read()
        self.assertIn('id="v0.8.261113.1"', page)
        self.assertNotIn('id="v0.8.261113.2"', page)           # not published yet: not on the page
        with self.assertRaises(SystemExit):                     # a draft blocks the page
            self.rn.site(out, "1.0.261120", t)

    def test_latest_json_security(self):
        import latest_json
        s = latest_json.security("0.8.261113.1", "0.8.261021.1")
        self.assertTrue(s["update"])
        self.assertEqual((s["fixes"], s["highest"], s["in_the_wild"]), (2, "high", True))
        # Control: our own build on the same Chromium is not a security update...
        self.assertFalse(latest_json.security("0.8.261113.2", "0.8.261113.1")["update"])
        # ...unless we say it is.
        s = latest_json.security("0.8.261113.2", "0.8.261113.1", own="Blocks a bad certificate.")
        self.assertTrue(s["update"])
        self.assertEqual(s["source"], "browsecraft")
        # Skipped versions count: from 261021.1 straight to 261113.2 still carries 157's fixes.
        self.assertTrue(latest_json.security("0.8.261113.2", "0.8.261021.1")["update"])


if __name__ == "__main__":
    unittest.main(verbosity=1)
