# Puts back third_party/win_build_output (Chromium's checked-in outputs of Windows-only tools,
# MIDL .tlb and MC .bin), which ungoogled-chromium's binary pruning removes and a Linux
# cross-build cannot make. Run in the Windows source tree: python3 fetch-win-build-output.py
# <pruning.list> <chromium version>
import base64, concurrent.futures, os, sys, urllib.request
pruning, version = sys.argv[1], sys.argv[2]
paths = [l.strip() for l in open(pruning) if l.startswith("third_party/win_build_output/")]
def get(p):
    url = f"https://chromium.googlesource.com/chromium/src/+/{version}/{p}?format=TEXT"
    data = base64.b64decode(urllib.request.urlopen(url, timeout=60).read())
    os.makedirs(os.path.dirname(p), exist_ok=True)
    open(p, "wb").write(data)
    return p
with concurrent.futures.ThreadPoolExecutor(6) as ex:
    for p in ex.map(get, paths):
        print(p)
