import os, shutil, subprocess
SRC = "/srv/browsecraft/portablelinux/build/src"
WORK = "/srv/browsecraft/patchwork/fixlink"
OUT = "/srv/browsecraft/patches/browsecraft/fix-non-official-link.patch"
f = "chrome/browser/enterprise/signin/profile_management_disclaimer_service.cc"
if os.path.exists(WORK): shutil.rmtree(WORK)
for side in ("a", "b"):
    os.makedirs(os.path.dirname(f"{WORK}/{side}/{f}"), exist_ok=True)
    shutil.copy(f"{SRC}/{f}", f"{WORK}/{side}/{f}")
p = f"{WORK}/b/{f}"
s = open(p).read()
start = s.index("  if (profile_separation_policies_for_testing_.has_value() ||\n      user_choice_for_testing_.has_value()) {\n    CHECK_IS_TEST();")
end = s.index("    return;\n  }\n", start) + len("    return;\n  }\n")
s = s[:start] + "  // BrowseCraft: the tests-only path is gone (its controller is not built:\n  // ungoogled-chromium removes it), which non-LTO builds need to link.\n" + s[end:]
open(p, "w").write(s)
out = subprocess.run(["diff", "-ruN", "a", "b"], cwd=WORK, capture_output=True, text=True).stdout
open(OUT, "w").write(out)
print(out)
