import random, sys
print("python", sys.version.split()[0])
def f17(v): return "%.17g" % v
r = random.Random(42); print("random()", [f17(r.random()) for _ in range(10)])
r = random.Random(42); print("getrandbits(32)", [r.getrandbits(32) for _ in range(10)])
r = random.Random(42); print("randrange(100)", [r.randrange(100) for _ in range(10)])
r = random.Random(42); print("expovariate(1.0)", [f17(r.expovariate(1.0)) for _ in range(10)])
r = random.Random(42); x=list(range(10)); r.shuffle(x); print("shuffle", x)
print("RR99", [f17(v) for v in (lambda r:[r.random() for _ in range(5)])(random.Random(99))])
print("RI5", [f17(v) for v in (lambda r:[r.expovariate(1.0/5.0) for _ in range(3)])(random.Random(5))])
print("RT11", [f17(v) for v in (lambda r:[r.expovariate(1.0/3.0) for _ in range(3)])(random.Random(11))])
