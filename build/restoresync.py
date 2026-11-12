import os, shutil, subprocess
SRC = "/srv/browsecraft/portablelinux/build/src"
B = "/srv/browsecraft"
WORK = f"{B}/patchwork/restoresync"
OUT = f"{B}/patches/browsecraft/restore-sync-service.patch"
f = "chrome/browser/sync/sync_service_factory.cc"
if os.path.exists(WORK): shutil.rmtree(WORK)
for side in ("a", "b"):
    os.makedirs(os.path.dirname(f"{WORK}/{side}/{f}"), exist_ok=True)
    shutil.copy(f"{SRC}/{f}", f"{WORK}/{side}/{f}")
old = open(f"{B}/syncfactory-old.txt").read()
new = open(f"{B}/syncfactory-new.txt").read().rstrip("\n")
p = f"{WORK}/b/{f}"
s = open(p).read()
assert s.count(old) == 1, "old body not found once"
s = s.replace(old, new)
open(p, "w").write(s)
out = subprocess.run(["diff", "-ruN", "a", "b"], cwd=WORK, capture_output=True, text=True).stdout
open(OUT, "w").write(out)
print(OUT, len(out.splitlines()), "lines")
