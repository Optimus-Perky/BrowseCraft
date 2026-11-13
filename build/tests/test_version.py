"""build/version.py: the v<major>.<minor>.<yymmdd>[letter] scheme from Chromium 158 (13 Nov
2026), and the old one up to 157. Run: python3 -I build/tests/test_version.py"""
import datetime, importlib.util, os, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("version", os.path.join(HERE, "..", "version.py"))
version = importlib.util.module_from_spec(spec)
spec.loader.exec_module(version)

d = tempfile.mkdtemp()
version.LOG = os.path.join(d, "builds.log")
with open(version.LOG, "w") as f:
    f.write("# test\n0.8.261021.1 2026-10-21 08:27 155.0.8059.39\n0.8.261113.1 2026-11-13 12:32 157.0.8103.50\n")
day = lambda s: datetime.date(2000 + int(s[:2]), int(s[2:4]), int(s[4:]))
bad = 0


def want(got, exp, what):
    global bad
    if got != exp:
        bad += 1
        print(f"FAIL {what}: {got}, want {exp}")


def build(chromium, today):
    v = version.next_version(chromium, day(today))
    version.record(v, chromium)
    return v


want(build("157.0.8103.80", "261114"), version.release() + ".261114.1", "157 point release keeps the old scheme")
want(build("158.0.8127.37", "261120"), "1.0.261120", "first 158 build")
want(build("158.0.8127.37", "261121"), "1.0.261120a", "a fix on the same Chromium: a letter, same number")
want(build("158.0.8127.37", "261125"), "1.0.261120b", "another fix")
want(build("158.0.8127.52", "261201"), "1.1.261201", "158 point release: minor up")
want(build("158.0.8127.52", "261202"), "1.1.261201a", "a fix on it")
want(build("158.0.8127.70", "261210"), "1.2.261210", "next point release")
want(build("159.0.8200.20", "270105"), "2.0.270105", "159: major up, minor 0")
want(build("159.0.8200.31", "270112"), "2.1.270112", "159 point release")
for ok in ["1.0.261120", "1.0.261120a", "0.8.261013.1"]:
    version.check(ok)
for no in ["1.0.26112", "1.0.261120A", "1.0.261120ab", "v1.0.261120"]:
    try:
        version.check(no); bad += 1; print("FAIL accepted", no)
    except SystemExit:
        pass
print(open(version.LOG).read())
print("PASS" if not bad else f"{bad} FAILED")
sys.exit(1 if bad else 0)
