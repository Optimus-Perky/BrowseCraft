#!/usr/bin/env python3
"""BrowseCraft: extensions can sign in to their own services (chrome.identity.launchWebAuthFlow).

An extension signs in by opening its provider's login page with a return address of
https://<extension id>.chromiumapp.org/; the browser catches that address and hands the result
to the extension. ungoogled-chromium's domain substitution turns chromiumapp.org into
ch40m1umapp.qjz9zk, so every provider refuses the return address and the extension never keeps
its login (found 2026-10-17: the Claude extension asked the user to sign in every time). No request
ever goes to chromiumapp.org; the browser only matches the address, so putting it back contacts
nobody.

Run in the src tree AFTER domain substitution."""
import sys

EDITS = {
    "chrome/browser/extensions/api/identity/identity_launch_web_auth_flow_function.cc": [
        ('"https://%s.ch40m1umapp.qjz9zk/";', '"https://%s.chromiumapp.org/";'),
    ],
    "chrome/renderer/resources/extensions/identity_custom_bindings.js": [
        ("'.ch40m1umapp.qjz9zk'", "'.chromiumapp.org'"),
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
