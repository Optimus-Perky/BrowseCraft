import os, shutil, subprocess
SRC = "/srv/browsecraft/portablelinux/build/src"
B = "/srv/browsecraft"
WORK = f"{B}/patchwork/skipsec"
OUT = f"{B}/patches/browsecraft/sync-without-security-events.patch"
files = ["chrome/browser/sync/chrome_sync_controller_builder.h", "chrome/browser/sync/chrome_sync_controller_builder.cc"]
if os.path.exists(WORK): shutil.rmtree(WORK)
for f in files:
    for side in ("a", "b"):
        os.makedirs(os.path.dirname(f"{WORK}/{side}/{f}"), exist_ok=True)
        shutil.copy(f"{SRC}/{f}", f"{WORK}/{side}/{f}")
h = f"{WORK}/b/{files[0]}"
s = open(h).read()
old = """    // Set() must have been called before.
    Ptr value() const {"""
new = """    // BrowseCraft: whether Set() was called (ungoogled-chromium never sets
    // the security event recorder).
    bool has_value() const { return ptr_.has_value(); }

    // Set() must have been called before.
    Ptr value() const {"""
assert s.count(old) == 1
open(h, "w").write(s.replace(old, new))
c = f"{WORK}/b/{files[1]}"
s = open(c).read()
start = s.index("    syncer::DataTypeControllerDelegate* security_events_delegate =")
end_marker = "            security_events_delegate)));\n"
end = s.index(end_marker, start) + len(end_marker)
block = s[start:end]
wrapped = "  // BrowseCraft: SECURITY_EVENTS only when there is a recorder.\n  if (security_event_recorder_.has_value()) {\n" + block + "  }\n"
s = s[:start] + wrapped + s[end:]
open(c, "w").write(s)
out = subprocess.run(["diff", "-ruN", "a", "b"], cwd=WORK, capture_output=True, text=True).stdout
open(OUT, "w").write(out)
print(out)
