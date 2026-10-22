import sys, json, pathlib
sys.path.insert(0, "src")
from contingency.conformance.spec import build_schedule_from_spec
from contingency import ResponseEvent
base = pathlib.Path("conformance/atomic")
for name in ["vr_seeded_42","vi_seeded_7","vt_seeded_3","rr_seeded_99","ri_seeded_5","rt_seeded_11"]:
    d = json.load(open(base / f"{name}.json"))
    sch = build_schedule_from_spec(d["schedule"])
    mism = 0; n_rein = 0
    for i, st in enumerate(d["steps"]):
        ev = st["event"]
        e = ResponseEvent(time=ev["time"], operandum=ev["operandum"]) if ev else None
        o = sch.step(st["now"], e)
        if o.reinforced != st["expect"]["reinforced"]: mism += 1
        if o.reinforced: n_rein += 1
    print(f"{name}: steps={len(d['steps'])} python_reinforced={n_rein} mismatches={mism} expect_total={sum(1 for s in d['steps'] if s['expect']['reinforced'])}")
