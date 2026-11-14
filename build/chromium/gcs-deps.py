#!/usr/bin/env python3
"""Build-time packages that Chromium's DEPS fetches from Google Cloud Storage and the lite tarball
may leave out, fetched for a Linux host from the tree's own DEPS (object, sha256), into each tree.

Chromium 157.0.8103.80's lite tarball has no third_party/llvm-libclang (157.0.8103.50's had it),
but v8 generates gen/v8/metagen/instance-types.h with libclang's Python bindings: the Windows
build stopped at once with "cindex.py ... missing and no known rule to make it" (14 Nov 2026).

    gcs-deps.py --dl <cache dir> <src tree>...

A path already in a tree with the same object (its .gcs-object stamp, or files from the
tarball itself) is left alone. Build-time tools only: nothing here ends up in BrowseCraft.
"""
import argparse, hashlib, os, re, shutil, sys, tarfile, urllib.request

# DEPS path -> what must be there afterwards
WANTED = {
    "src/third_party/llvm-libclang": "bindings/python/clang/cindex.py",
}
HOST = "Linux_x64/"   # the build host; the Windows build is a cross-build from Linux too


def deps_objects(deps_text, path):
    """The objects of one gcs dep: [(bucket, object_name, sha256)] for this host."""
    m = re.search(r"'%s':\s*\{(.*?)\n  \}," % re.escape(path), deps_text, re.S)
    if not m:
        return None
    block = m.group(1)
    if "'dep_type': 'gcs'" not in block:
        sys.exit(f"{path}: not a gcs dep in DEPS")
    bucket = re.search(r"'bucket':\s*'([^']+)'", block).group(1)
    out = []
    for o in re.finditer(r"\{\s*'object_name':\s*'([^']+)',\s*'sha256sum':\s*'([0-9a-f]{64})'(.*?)\}", block, re.S):
        name, sha, rest = o.groups()
        if name.startswith(HOST) and 'host_os == "linux"' in rest:
            out.append((bucket, name, sha))
    return out


def fetch(bucket, name, sha, dl):
    path = os.path.join(dl, name.replace("/", "_"))
    if not os.path.exists(path):
        url = f"https://storage.googleapis.com/{bucket}/{name}"
        urllib.request.urlretrieve(url, path + ".part")
        os.rename(path + ".part", path)
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    if h.hexdigest() != sha:
        os.remove(path)
        sys.exit(f"{name}: sha256 mismatch (expected {sha}, got {h.hexdigest()})")
    return path


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--dl", required=True)
    p.add_argument("trees", nargs="+")
    a = p.parse_args()
    os.makedirs(a.dl, exist_ok=True)
    for tree in a.trees:
        deps = open(os.path.join(tree, "DEPS")).read()
        for dep, must in WANTED.items():
            out = os.path.join(tree, dep[len("src/"):])
            objs = deps_objects(deps, dep)
            if objs is None:   # no longer in DEPS: nothing to do
                print(f"{tree}: {dep} not in DEPS")
                continue
            if len(objs) != 1:
                sys.exit(f"{dep}: expected one {HOST} object in DEPS, found {len(objs)}")
            bucket, name, sha = objs[0]
            stamp = os.path.join(out, ".gcs-object")
            if os.path.exists(os.path.join(out, must)):
                have = open(stamp).read().strip() if os.path.exists(stamp) else "from the tarball"
                if have in (name, "from the tarball"):
                    print(f"{tree}: {dep} there ({have})")
                    continue
            tar = fetch(bucket, name, sha, a.dl)
            if os.path.isdir(out):
                shutil.rmtree(out)
            os.makedirs(out)
            with tarfile.open(tar) as t:
                t.extractall(out, filter="data")
            if not os.path.exists(os.path.join(out, must)):
                sys.exit(f"{dep}: {must} not in {name}")
            open(stamp, "w").write(name + "\n")
            print(f"{tree}: {dep} <- {name}")


if __name__ == "__main__":
    main()
