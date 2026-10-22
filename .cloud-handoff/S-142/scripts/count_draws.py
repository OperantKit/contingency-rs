import sys, json, pathlib, random, math
sys.path.insert(0, "src")
# instrument random.Random API calls
calls = []
class Spy(random.Random):
    def random(self): calls.append(("random",)); return super().random()
    def getrandbits(self, k): calls.append(("getrandbits", k)); return super().getrandbits(k)
    def getstate(self): calls.append(("getstate",)); return super().getstate()
    def setstate(self, s): calls.append(("setstate",)); return super().setstate(s)
random.Random = Spy  # schedules/helpers do `import random` and call random.Random(...)
from contingency.conformance.spec import build_schedule_from_spec
from contingency import ResponseEvent
base = pathlib.Path("conformance/atomic")
for name in ["vr_seeded_42","vi_seeded_7","vt_seeded_3","rr_seeded_99","ri_seeded_5","rt_seeded_11"]:
    calls.clear()
    d = json.load(open(base / f"{name}.json"))
    sch = build_schedule_from_spec(d["schedule"])
    for st in d["steps"]:
        ev = st["event"]
        sch.step(st["now"], ResponseEvent(time=ev["time"], operandum=ev["operandum"]) if ev else None)
    from collections import Counter
    c = Counter(x[0] if x[0] != "getrandbits" else f"getrandbits({x[1]})" for x in calls)
    print(name, dict(c))
# float identity checks
r = random.Random(0); bad=0; N=200000
for m in (5.0, 3.0, 10.0):
    lambd = 1.0/m; bad=0
    for _ in range(N):
        u = r.random(); a = -math.log(1.0-u)/lambd; b = -m*math.log(1.0-u)
        if a != b: bad += 1
    print(f"mean={m}: -log(1-u)/(1/m) != -m*log(1-u) in {bad}/{N} draws")
# fsum vs naive sum in FH progression correction
from contingency.helpers.fleshler_hoffman import _fh_progression
for v,n in ((10.0,12),(5.0,12),(3.0,12)):
    raw=_fh_progression(v,n); s1=math.fsum(raw); s2=0.0
    for x in raw: s2+=x
    print(f"FH v={v} n={n}: fsum={s1!r} naive={s2!r} same={s1==s2}; any .5 for round? {[x for x in raw if abs(x-math.floor(x)-0.5)<1e-12]}")
