import random, sys, json, platform
out = {}
out["python"] = sys.version
out["implementation"] = platform.python_implementation()
r = random.Random(42); out["random_10"] = [repr(r.random()) for _ in range(10)]
r = random.Random(42); out["getrandbits32_10"] = [r.getrandbits(32) for _ in range(10)]
r = random.Random(42); out["randrange100_10"] = [r.randrange(100) for _ in range(10)]
r = random.Random(42); out["expovariate1_10"] = [repr(r.expovariate(1.0)) for _ in range(10)]
r = random.Random(42); x = list(range(10)); r.shuffle(x); out["shuffle_range10"] = x
# state after seeding: first 3 words + index, to pin init_by_array
r = random.Random(42); st = r.getstate()[1]; out["state42_first4_and_index"] = [st[0], st[1], st[2], st[3], st[624]]
# fixture-specific first draws
out["fixture"] = {
  "VR_seed42_randrange_0_2p31m1": random.Random(42).randrange(0, 2**31 - 1),
  "VI_seed7_getrandbits64": random.Random(7).getrandbits(64),
  "VT_seed3_randrange_0_2p31m1": random.Random(3).randrange(0, 2**31 - 1),
  "RR_seed99_random_first5": [repr(v) for v in (lambda r: [r.random() for _ in range(5)])(random.Random(99))],
  "RI_seed5_expovariate_0p2_first3": [repr(v) for v in (lambda r: [r.expovariate(1.0/5.0) for _ in range(3)])(random.Random(5))],
  "RT_seed11_expovariate_1div3_first3": [repr(v) for v in (lambda r: [r.expovariate(1.0/3.0) for _ in range(3)])(random.Random(11))],
}
# 64-bit getrandbits word-order check: compare with two 32-bit draws
r1 = random.Random(7); a = r1.getrandbits(32); b = r1.getrandbits(32)
out["getrandbits64_wordorder_check"] = {"seed7_two_u32": [a, b], "lo_plus_hi_shift32": a | (b << 32), "equals_getrandbits64": (a | (b << 32)) == out["fixture"]["VI_seed7_getrandbits64"]}
json.dump(out, sys.stdout, indent=1); print()
