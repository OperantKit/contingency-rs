# S-142 — (f) PRNG 等価: 使用 API と CPython 実装の逐語確認

調査のみ。ソースコードは変更していない。

## 0. 前提（版・行番号の基準）

| repo | commit | 備考 |
|---|---|---|
| contingency-rs | `d96582a` (`docs: add CITATION.cff`) | この session の clone。`git fetch` 後の `origin/main` と同一。したがって本書の行番号は **GitHub 上の main の版** のもの。 |
| contingency-py | `bc29fd8` (`docs: drop JOSS paper draft entry from CHANGELOG`) | 同上。GitHub 上の main と同一。 |

- 作業 TODO の注記にある「手元に未 push の COD・Fleshler-Hoffman・Gymnasium 関連の変更」は、この cloud session の clone には存在しない（clone は origin/main と一致）。手元の作業木で行番号を使うときはズレに注意。
- Python の版: この session の `python3.11` は CPython 3.11.17 (Linux x86_64, GCC 13.3.0)。固定列の生成と fixture の再生確認に使った。

### 0.1 stochastic fixture 6 本（contingency-rs `tests/conformance.rs`）

`#[ignore = "stochastic: PRNG differs between Python MT and Rust SmallRng"]` が付いているのは次の 6 テスト。

| 行（`#[ignore]` 属性） | テスト関数 | fixture |
|---|---|---|
| 552 | `atomic_vr_seeded_42` | `atomic/vr_seeded_42.json` |
| 558 | `atomic_vi_seeded_7` | `atomic/vi_seeded_7.json` |
| 564 | `atomic_vt_seeded_3` | `atomic/vt_seeded_3.json` |
| 570 | `atomic_rr_seeded_99` | `atomic/rr_seeded_99.json` |
| 576 | `atomic_ri_seeded_5` | `atomic/ri_seeded_5.json` |
| 582 | `atomic_rt_seeded_11` | `atomic/rt_seeded_11.json` |

関連箇所: `run_fixture` L358（bit 等価の再生）、`run_stochastic_fixture` L413（緩和版: 強化回数が Python 記録の 50% 帯に入るかだけ）、`STOCHASTIC_TYPES` L609、completeness guard の除外 L728、`replayed_fixtures()` L661（ソースを `run_fixture("` で grep する）。fixture の schedule 構築は L128-168（`VR`/`RR`/`VI`/`RI`/`VT`/`RT`、seed は `optional_u64(params, "seed")`）。

### 0.2 fixture が MT に完全に pin されていることの確認

`contingency.conformance.spec.build_schedule_from_spec` で 6 fixture を CPython 3.11.17 上で再生したところ、全 step で `reinforced` が一致（不一致 0）。

```
vr_seeded_42: steps=100 python_reinforced=17 mismatches=0 expect_total=17
vi_seeded_7:  steps=81  python_reinforced=5  mismatches=0 expect_total=5
vt_seeded_3:  steps=81  python_reinforced=8  mismatches=0 expect_total=8
rr_seeded_99: steps=60  python_reinforced=10 mismatches=0 expect_total=10
ri_seeded_5:  steps=61  python_reinforced=8  mismatches=0 expect_total=8
rt_seeded_11: steps=81  python_reinforced=14 mismatches=0 expect_total=14
```

（スクリプト: `.cloud-handoff/S-142/scripts/replay6.py`）

## (a) contingency-py が使う `random.Random` API と呼び出し箇所

### (a-1) 呼び出し箇所（`src/contingency/` 配下、bc29fd8）

grep 対象: `random|Random|randrange|expovariate|getrandbits|getstate|setstate|shuffle|.random()|.uniform|.choice|.gauss|.sample|.seed(`。`uniform`/`choice`/`gauss`/`sample`/`randint`/`seed()` の使用は **0 件**。

| ファイル | 行 | 呼び出し | 引数の形 | 用途 |
|---|---|---|---|---|
| `helpers/fleshler_hoffman.py` | 56 | `random.Random(seed)` | `seed: int \| None`（呼び出し元の sub-seed） | `_shuffled` の RNG |
| 同 | 58 | `rng.shuffle(out)` | `list[float]`（長さ n、既定 12） | FH 列のシャッフル（`generate_intervals` L91 / `generate_ratios` L125 から） |
| 同 | 143 | `random.Random(seed)` | | `generate_intervals_hantula1991` |
| 同 | 149 | `rng.randrange(0, n)` | 2 引数（start=0, stop=n, step 既定） | Hantula 変種の配置先 |
| `schedules/interval.py` | 197 | `random.Random(seed)` | `seed: int \| None` | `VI.__init__` master RNG |
| 同 | 206 | `self._rng.getrandbits(64)` | k=64 | `VI._generate_sequence` の sub-seed |
| 同 | 239 | `random.Random(self._seed)` | | `VI.reset` |
| 同 | 279 | `random.Random()` | 引数なし（seed None） | `RI.__init__` の rng 未指定時 |
| 同 | 280 | `self._rng.getstate()` | | `RI.__init__` 初期状態の snapshot |
| 同 | 285 | `self._rng.expovariate(1.0 / self._mean_interval)` | `lambd = 1.0 / mean` | `RI._draw_interval` |
| 同 | 305 | `self._rng.setstate(self._initial_state)` | | `RI.reset` |
| `schedules/ratio.py` | 198 | `random.Random(seed)` | | `VR.__init__` |
| 同 | 216 | `self._rng.randrange(0, 2**31 - 1)` | 2 引数（0, 2147483647） | `VR._next_sub_seed` |
| 同 | 247 | `random.Random(self._seed)` | | `VR.reset` |
| 同 | 289 | `random.Random()` | 引数なし | `RR.__init__` の rng 未指定時 |
| 同 | 292 | `self._rng.getstate()` | | `RR.__init__` snapshot |
| 同 | 305 | `self._rng.random() < self._p` | | `RR.step` Bernoulli |
| 同 | 310 | `self._rng.setstate(self._initial_state)` | | `RR.reset` |
| `schedules/time_based.py` | 237 | `random.Random(seed)` | | `VT.__init__` |
| 同 | 254 | `self._rng.randrange(0, 2**31 - 1)` | 2 引数 | `VT._next_sub_seed` |
| 同 | 285 | `random.Random(self._seed)` | | `VT.reset` |
| 同 | 349 | `random.Random()` | 引数なし | `RT.__init__` の rng 未指定時 |
| 同 | 350 | `self._rng.getstate()` | | `RT.__init__` snapshot |
| 同 | 351 | `self._rng.expovariate(1.0 / self._mean)` | | `RT.__init__` 初回要求 |
| 同 | 360 | `self._rng.expovariate(1.0 / self._mean)` | | `RT._draw_requirement` |
| 同 | 376 | `self._rng.setstate(self._initial_state)` | | `RT.reset` |
| 同 | 377 | `self._rng.expovariate(1.0 / self._mean)` | | `RT.reset` |
| `schedules/trial_based.py` | 194 | `random.Random(seed)` | | `MatchingToSample.__init__` |
| 同 | 227 | `self._rng.randrange(self._n_comparisons)` | 1 引数（stop のみ） | 正解 operandum の抽選 |
| 同 | 383 | `random.Random(self._seed)` | | `MatchingToSample.reset` |
| 同 | 494 | `random.Random(seed)` | | `GoNoGo.__init__` |
| 同 | 533 | `self._rng.random() < self._go_probability` | | go/no-go 試行型の抽選 |
| 同 | 678 | `random.Random(self._seed)` | | `GoNoGo.reset` |
| `conformance/spec.py` | 67, 78, 89 | `random.Random(params["seed"])` | fixture の `seed`（int） | RR / RI / RT に `rng=` で注入 |
| 同 | 64, 75, 86 | （`seed=params.get("seed")`） | | VR / VI / VT は seed を渡す |
| `bridge/__init__.py` | 320 | `random.Random(seed)` | | `_next_rng`（DSL 側） |

使用 API の集合（数え直し）: `Random(seed)` コンストラクタ、`random()`、`getrandbits(64)`、`randrange(0, n)` / `randrange(n)` / `randrange(0, 2**31 - 1)`、`expovariate(lambd)`、`shuffle(list)`、`getstate()` / `setstate()`。手元の集計（randrange・expovariate・getstate/setstate・random()・shuffle・getrandbits）と一致。それ以外の `random.Random` メソッドは使っていない。

### (a-2) 6 fixture がどの schedule・どの API に依存するか

Python 側を計装（`random.Random` を subclass で差し替えて呼び出しを数えた。`.cloud-handoff/S-142/scripts/count_draws.py`）。内部的に `randrange` と `shuffle` は `_randbelow_with_getrandbits` → `getrandbits(k)` に落ちるので、その内訳も併記する。

| fixture | schedule（params） | 依存する Python API（呼び出し順） | 実測の draw 数（fixture 全 step） |
|---|---|---|---|
| `vr_seeded_42` | `VR(mean=5, n_intervals=12, seed=42)` | `Random(42)` → `randrange(0, 2**31-1)`（sub-seed）→ `generate_ratios(5, 12, seed=sub)` 内で `Random(sub).shuffle(12 個の int)`。強化 17 回 > 12 なので 2 cycle 目の reload が走り、同じ経路がもう 1 回 | `getrandbits(31)` ×2（棄却なし）、shuffle 内 `getrandbits(4)` ×24・`(3)` ×11・`(2)` ×8 |
| `vi_seeded_7` | `VI(mean_interval=10.0, n_intervals=12, seed=7)` | `Random(7).getrandbits(64)`（sub-seed）→ `generate_intervals(10.0, 12, seed=sub)` 内で `Random(sub).shuffle(12 個の float)`。強化 5 回で 1 cycle | `getrandbits(64)` ×1、shuffle 内 `(4)` ×9・`(3)` ×5・`(2)` ×3 |
| `vt_seeded_3` | `VT(mean_interval=10.0, n_intervals=12, seed=3)` | `Random(3).randrange(0, 2**31-1)` → `generate_intervals` → `shuffle`。強化 8 回で 1 cycle | `getrandbits(31)` ×1、shuffle 内 `(4)` ×9・`(3)` ×4・`(2)` ×2 |
| `rr_seeded_99` | `RR(probability=0.2, rng=Random(99))` | `getstate()` ×1（構築時 snapshot）、以後 response step ごとに `random()` | `random()` ×60 |
| `ri_seeded_5` | `RI(mean_interval=5.0, rng=Random(5))` | `getstate()` ×1、`expovariate(1.0/5.0)`（構築時 1 回 + 強化ごと） | `random()` ×9（= expovariate 9 回） |
| `rt_seeded_11` | `RT(mean_interval=3.0, rng=Random(11))` | `getstate()` ×1、`expovariate(1.0/3.0)`（構築時 1 回 + 強化ごと） | `random()` ×15 |

補足:
- shuffle の `getrandbits(k)` の k は `_randbelow(i+1)` の `(i+1).bit_length()`。12 要素なら i+1 = 12..8 で k=4、7..4 で k=3、3..2 で k=2。回数が最小値（5, 4, 2）より多いのは棄却ループ（`r >= n` で引き直し）による。
- `setstate` は fixture 再生中に **0 回**（`reset()` でしか呼ばれず、fixture の step 列は reset を含まない）。
- 各 fixture の最初の draw 値（S-143 の単体テスト用）:
  - `Random(42).randrange(0, 2**31 - 1)` = `1373158606`（VR の 1 本目 sub-seed）
  - `Random(7).getrandbits(64)` = `17485029721327973432`（VI の sub-seed。2^32 以上なので `Random(sub)` の seed は 2 word の key になる）
  - `Random(3).randrange(0, 2**31 - 1)` = `511025150`（VT の sub-seed）
  - `Random(99).random()` 先頭 5 個 = `0.40397807494366633, 0.20007544457494542, 0.17880232058661227, 0.24843131850096878, 0.75987743650807793`
  - `Random(5).expovariate(1.0/5.0)` 先頭 3 個 = `4.8762468463219681, 6.7698520583964834, 7.9284498408703836`
  - `Random(11).expovariate(1.0/3.0)` 先頭 3 個 = `1.8065185436914499, 2.4613901443892892, 7.7393898810090835`

## (b) CPython 3.11 の一次ソース（逐語）

取得方法: `curl` で raw を実取得（2026-10-22）。`3.11` はブランチ名で先頭が動きうるので、行番号は取得時点のもの。sha256 も併記。

| ファイル | 取得 URL | sha256（取得物） | 閲覧用 |
|---|---|---|---|
| `Lib/random.py` | https://raw.githubusercontent.com/python/cpython/3.11/Lib/random.py | `b931a2d452db89ab9e1f79d59b66ce194dbd6748049cfe44699726da52b9c956` | https://github.com/python/cpython/blob/3.11/Lib/random.py |
| `Modules/_randommodule.c` | https://raw.githubusercontent.com/python/cpython/3.11/Modules/_randommodule.c | `4c56324b3e654c6fe4f7035bd850672b6e56ec8a11380bca62852e0ff5dce885` | https://github.com/python/cpython/blob/3.11/Modules/_randommodule.c |

注意: `Lib/random.py` L796-806 にも `random()` / `getrandbits()` の定義があるが、それは `class SystemRandom`（`os.urandom` ベース）のもので MT ではない。MT の `random()` / `getrandbits()` / `seed()` / `getstate()` / `setstate()` は C 側（`_random.Random`）で定義され、`random.Random` は `_random.Random` を継承して使う（random.py L103）。

### (b-1) `_randommodule.c`: 定数・`genrand_uint32`（L79-84, L122-155）

```c
/* Period parameters -- These are all magic.  Don't change. */
#define N 624
#define M 397
#define MATRIX_A 0x9908b0dfU    /* constant vector a */
#define UPPER_MASK 0x80000000U  /* most significant w-r bits */
#define LOWER_MASK 0x7fffffffU  /* least significant r bits */
```

```c
/* generates a random number on [0,0xffffffff]-interval */
static uint32_t
genrand_uint32(RandomObject *self)
{
    uint32_t y;
    static const uint32_t mag01[2] = {0x0U, MATRIX_A};
    /* mag01[x] = x * MATRIX_A  for x=0,1 */
    uint32_t *mt;

    mt = self->state;
    if (self->index >= N) { /* generate N words at one time */
        int kk;

        for (kk=0;kk<N-M;kk++) {
            y = (mt[kk]&UPPER_MASK)|(mt[kk+1]&LOWER_MASK);
            mt[kk] = mt[kk+M] ^ (y >> 1) ^ mag01[y & 0x1U];
        }
        for (;kk<N-1;kk++) {
            y = (mt[kk]&UPPER_MASK)|(mt[kk+1]&LOWER_MASK);
            mt[kk] = mt[kk+(M-N)] ^ (y >> 1) ^ mag01[y & 0x1U];
        }
        y = (mt[N-1]&UPPER_MASK)|(mt[0]&LOWER_MASK);
        mt[N-1] = mt[M-1] ^ (y >> 1) ^ mag01[y & 0x1U];

        self->index = 0;
    }

    y = mt[self->index++];
    y ^= (y >> 11);
    y ^= (y << 7) & 0x9d2c5680U;
    y ^= (y << 15) & 0xefc60000U;
    y ^= (y >> 18);
    return y;
}
```

### (b-2) `_randommodule.c`: `random()` = genrand_res53（L157-181）

```c
/* random_random is the function named genrand_res53 in the original code;
 * generates a random number on [0,1) with 53-bit resolution; note that
 * 9007199254740992 == 2**53; I assume they're spelling "/2**53" as
 * multiply-by-reciprocal in the (likely vain) hope that the compiler will
 * optimize the division away at compile-time.  67108864 is 2**26.  In
 * effect, a contains 27 random bits shifted left 26, and b fills in the
 * lower 26 bits of the 53-bit numerator.
 * The original code credited Isaku Wada for this algorithm, 2002/01/09.
 */

/*[clinic input]
_random.Random.random

  self: self(type="RandomObject *")

random() -> x in the interval [0, 1).
[clinic start generated code]*/

static PyObject *
_random_Random_random_impl(RandomObject *self)
/*[clinic end generated code: output=117ff99ee53d755c input=afb2a59cbbb00349]*/
{
    uint32_t a=genrand_uint32(self)>>5, b=genrand_uint32(self)>>6;
    return PyFloat_FromDouble((a*67108864.0+b)*(1.0/9007199254740992.0));
}
```

### (b-3) `_randommodule.c`: `init_genrand` / `init_by_array`（L183-232）

```c
/* initializes mt[N] with a seed */
static void
init_genrand(RandomObject *self, uint32_t s)
{
    int mti;
    uint32_t *mt;

    mt = self->state;
    mt[0]= s;
    for (mti=1; mti<N; mti++) {
        mt[mti] =
        (1812433253U * (mt[mti-1] ^ (mt[mti-1] >> 30)) + mti);
        /* See Knuth TAOCP Vol2. 3rd Ed. P.106 for multiplier. */
        /* In the previous versions, MSBs of the seed affect   */
        /* only MSBs of the array mt[].                                */
        /* 2002/01/09 modified by Makoto Matsumoto                     */
    }
    self->index = mti;
    return;
}

/* initialize by an array with array-length */
/* init_key is the array for initializing keys */
/* key_length is its length */
static void
init_by_array(RandomObject *self, uint32_t init_key[], size_t key_length)
{
    size_t i, j, k;       /* was signed in the original code. RDH 12/16/2002 */
    uint32_t *mt;

    mt = self->state;
    init_genrand(self, 19650218U);
    i=1; j=0;
    k = (N>key_length ? N : key_length);
    for (; k; k--) {
        mt[i] = (mt[i] ^ ((mt[i-1] ^ (mt[i-1] >> 30)) * 1664525U))
                 + init_key[j] + (uint32_t)j; /* non linear */
        i++; j++;
        if (i>=N) { mt[0] = mt[N-1]; i=1; }
        if (j>=key_length) j=0;
    }
    for (k=N-1; k; k--) {
        mt[i] = (mt[i] ^ ((mt[i-1] ^ (mt[i-1] >> 30)) * 1566083941U))
                 - (uint32_t)i; /* non linear */
        i++;
        if (i>=N) { mt[0] = mt[N-1]; i=1; }
    }

    mt[0] = 0x80000000U; /* MSB is 1; assuring non-zero initial array */
}
```

### (b-4) `_randommodule.c`: `seed(int)` が key 配列を作る部分（L274-356）

```c
static int
random_seed(RandomObject *self, PyObject *arg)
{
    int result = -1;  /* guilty until proved innocent */
    PyObject *n = NULL;
    uint32_t *key = NULL;
    size_t bits, keyused;
    int res;

    if (arg == NULL || arg == Py_None) {
       if (random_seed_urandom(self) < 0) {
            PyErr_Clear();

            /* Reading system entropy failed, fall back on the worst entropy:
               use the current time and process identifier. */
            random_seed_time_pid(self);
        }
        return 0;
    }

    /* This algorithm relies on the number being unsigned.
     * So: if the arg is a PyLong, use its absolute value.
     * Otherwise use its hash value, cast to unsigned.
     */
    if (PyLong_CheckExact(arg)) {
        n = PyNumber_Absolute(arg);
    } else if (PyLong_Check(arg)) {
        /* Calling int.__abs__() prevents calling arg.__abs__(), which might
           return an invalid value. See issue #31478. */
        _randomstate *state = _randomstate_type(Py_TYPE(self));
        n = PyObject_CallOneArg(state->Long___abs__, arg);
    }
    else {
        Py_hash_t hash = PyObject_Hash(arg);
        if (hash == -1)
            goto Done;
        n = PyLong_FromSize_t((size_t)hash);
    }
    if (n == NULL)
        goto Done;

    /* Now split n into 32-bit chunks, from the right. */
    bits = _PyLong_NumBits(n);
    if (bits == (size_t)-1 && PyErr_Occurred())
        goto Done;

    /* Figure out how many 32-bit chunks this gives us. */
    keyused = bits == 0 ? 1 : (bits - 1) / 32 + 1;

    /* Convert seed to byte sequence. */
    key = (uint32_t *)PyMem_Malloc((size_t)4 * keyused);
    if (key == NULL) {
        PyErr_NoMemory();
        goto Done;
    }
    res = _PyLong_AsByteArray((PyLongObject *)n,
                              (unsigned char *)key, keyused * 4,
                              PY_LITTLE_ENDIAN,
                              0); /* unsigned */
    if (res == -1) {
        goto Done;
    }

#if PY_BIG_ENDIAN
    {
        size_t i, j;
        /* Reverse an array. */
        for (i = 0, j = keyused - 1; i < j; i++, j--) {
            uint32_t tmp = key[i];
            key[i] = key[j];
            key[j] = tmp;
        }
    }
#endif
    init_by_array(self, key, keyused);

    result = 0;

Done:
    Py_XDECREF(n);
    PyMem_Free(key);
    return result;
}
```

読み解き（int seed の場合）: `n = abs(seed)`。`bits = n.bit_length()`。`keyused = 1`（n == 0）または `(bits-1)/32 + 1`。`key[0]` が下位 32 bit、`key[1]` が次の 32 bit…（little-endian の 32 bit word 列）。その key を `init_by_array(key, keyused)` に渡す。**`init_genrand(seed)` を直接呼ぶのではない**（seed=42 でも `init_by_array([42], 1)`）。
`Lib/random.py` 側の `Random.seed`（L128-164）は、`a` が int のときは変換せずに `super().seed(a)` を呼ぶ（str/bytes/float は別経路。OperantKit は int しか渡さない）。

### (b-5) `_randommodule.c`: `getrandbits(k)`（L480-526）

```c
static PyObject *
_random_Random_getrandbits_impl(RandomObject *self, int k)
/*[clinic end generated code: output=b402f82a2158887f input=8c0e6396dd176fc0]*/
{
    int i, words;
    uint32_t r;
    uint32_t *wordarray;
    PyObject *result;

    if (k < 0) {
        PyErr_SetString(PyExc_ValueError,
                        "number of bits must be non-negative");
        return NULL;
    }

    if (k == 0)
        return PyLong_FromLong(0);

    if (k <= 32)  /* Fast path */
        return PyLong_FromUnsignedLong(genrand_uint32(self) >> (32 - k));

    words = (k - 1) / 32 + 1;
    wordarray = (uint32_t *)PyMem_Malloc(words * 4);
    if (wordarray == NULL) {
        PyErr_NoMemory();
        return NULL;
    }

    /* Fill-out bits of long integer, by 32-bit words, from least significant
       to most significant. */
#if PY_LITTLE_ENDIAN
    for (i = 0; i < words; i++, k -= 32)
#else
    for (i = words - 1; i >= 0; i--, k -= 32)
#endif
    {
        r = genrand_uint32(self);
        if (k < 32)
            r >>= (32 - k);  /* Drop least significant bits */
        wordarray[i] = r;
    }

    result = _PyLong_FromByteArray((unsigned char *)wordarray, words * 4,
                                   PY_LITTLE_ENDIAN, 0 /* unsigned */);
    PyMem_Free(wordarray);
    return result;
}
```

読み解き: `getrandbits(64)` は `genrand_uint32` を 2 回呼び、1 回目が下位 word、2 回目が上位 word。実測で確認: `Random(7)` の `getrandbits(32)` 2 回 = `[1390851128, 4071050724]`、`1390851128 | (4071050724 << 32)` = `17485029721327973432` = `Random(7).getrandbits(64)`（一致）。

### (b-6) `_randommodule.c`: `getstate` / `setstate`（L389-415, L428-467）

```c
static PyObject *
_random_Random_getstate_impl(RandomObject *self)
/*[clinic end generated code: output=bf6cef0c092c7180 input=b937a487928c0e89]*/
{
    PyObject *state;
    PyObject *element;
    int i;

    state = PyTuple_New(N+1);
    if (state == NULL)
        return NULL;
    for (i=0; i<N ; i++) {
        element = PyLong_FromUnsignedLong(self->state[i]);
        if (element == NULL)
            goto Fail;
        PyTuple_SET_ITEM(state, i, element);
    }
    element = PyLong_FromLong((long)(self->index));
    if (element == NULL)
        goto Fail;
    PyTuple_SET_ITEM(state, i, element);
    return state;

Fail:
    Py_DECREF(state);
    return NULL;
}
```

```c
static PyObject *
_random_Random_setstate(RandomObject *self, PyObject *state)
/*[clinic end generated code: output=fd1c3cd0037b6681 input=b3b4efbb1bc66af8]*/
{
    int i;
    unsigned long element;
    long index;
    uint32_t new_state[N];

    if (!PyTuple_Check(state)) {
        PyErr_SetString(PyExc_TypeError,
            "state vector must be a tuple");
        return NULL;
    }
    if (PyTuple_Size(state) != N+1) {
        PyErr_SetString(PyExc_ValueError,
            "state vector is the wrong size");
        return NULL;
    }

    for (i=0; i<N ; i++) {
        element = PyLong_AsUnsignedLong(PyTuple_GET_ITEM(state, i));
        if (element == (unsigned long)-1 && PyErr_Occurred())
            return NULL;
        new_state[i] = (uint32_t)element;
    }

    index = PyLong_AsLong(PyTuple_GET_ITEM(state, i));
    if (index == -1 && PyErr_Occurred())
        return NULL;
    if (index < 0 || index > N) {
        PyErr_SetString(PyExc_ValueError, "invalid state");
        return NULL;
    }
    self->index = (int)index;
    for (i = 0; i < N; i++)
        self->state[i] = new_state[i];

    Py_RETURN_NONE;
}
```

`Lib/random.py` 側（L117, L166-190）:

```python
    VERSION = 3     # used by getstate/setstate
```

```python
    def getstate(self):
        """Return internal state; can be passed to setstate() later."""
        return self.VERSION, super().getstate(), self.gauss_next

    def setstate(self, state):
        """Restore internal state from object returned by getstate()."""
        version = state[0]
        if version == 3:
            version, internalstate, self.gauss_next = state
            super().setstate(internalstate)
        elif version == 2:
            version, internalstate, self.gauss_next = state
            # In version 2, the state was saved as signed ints, which causes
            #   inconsistencies between 32/64-bit systems. The state is
            #   really unsigned 32-bit ints, so we convert negative ints from
            #   version 2 to positive longs for version 3.
            try:
                internalstate = tuple(x % (2 ** 32) for x in internalstate)
            except ValueError as e:
                raise TypeError from e
            super().setstate(internalstate)
        else:
            raise ValueError("state with version %s passed to "
                             "Random.setstate() of version %s" %
                             (version, self.VERSION))
```

**fixture 再生に `getstate/setstate` は要るか**: 要らない。(a-2) の計装で、6 fixture の再生中 `getstate` は構築時の snapshot 1 回だけ、`setstate` は 0 回（`reset()` 専用）。Rust 側 RR/RI/RT は既に `initial_rng.clone()` で同じ snapshot/復元をしているので、`PyRandom` が `Clone` なら十分。S-143/S-144 では **対象外**（実装するなら `(state, index)` の複製で足りる。`gauss_next` は `gauss()` 未使用なので不要）。

### (b-7) `Lib/random.py`: `__init__` / `seed`（L119-164）

```python
    def __init__(self, x=None):
        """Initialize an instance.

        Optional argument x controls seeding, as for Random.seed().
        """

        self.seed(x)
        self.gauss_next = None

    def seed(self, a=None, version=2):
        """Initialize internal state from a seed.

        The only supported seed types are None, int, float,
        str, bytes, and bytearray.

        None or no argument seeds from current time or from an operating
        system specific randomness source if available.

        If *a* is an int, all bits are used.

        For version 2 (the default), all of the bits are used if *a* is a str,
        bytes, or bytearray.  For version 1 (provided for reproducing random
        sequences from older versions of Python), the algorithm for str and
        bytes generates a narrower range of seeds.

        """

        if version == 1 and isinstance(a, (str, bytes)):
            a = a.decode('latin-1') if isinstance(a, bytes) else a
            x = ord(a[0]) << 7 if a else 0
            for c in map(ord, a):
                x = ((1000003 * x) ^ c) & 0xFFFFFFFFFFFFFFFF
            x ^= len(a)
            a = -2 if x == -1 else x

        elif version == 2 and isinstance(a, (str, bytes, bytearray)):
            if isinstance(a, str):
                a = a.encode()
            a = int.from_bytes(a + _sha512(a).digest())

        elif not isinstance(a, (type(None), int, float, str, bytes, bytearray)):
            raise TypeError('The only supported seed types are: None,\n'
                            'int, float, str, bytes, and bytearray.')

        super().seed(a)
        self.gauss_next = None
```

### (b-8) `Lib/random.py`: `_randbelow_with_getrandbits`（L235-243, L264）

```python
    def _randbelow_with_getrandbits(self, n):
        "Return a random int in the range [0,n).  Defined for n > 0."

        getrandbits = self.getrandbits
        k = n.bit_length()  # don't use (n-1) here because n can be 1
        r = getrandbits(k)  # 0 <= r < 2**k
        while r >= n:
            r = getrandbits(k)
        return r
```

```python
    _randbelow = _randbelow_with_getrandbits
```

（`random.Random` 自身は L264 で `_randbelow_with_getrandbits` に固定。`__init_subclass__`（L215-233）は subclass 用で、`random.Random` を直接使う OperantKit には関係ない。）

### (b-9) `Lib/random.py`: `randrange`（L284-356）

```python
    def randrange(self, start, stop=None, step=_ONE):
        """Choose a random item from range(stop) or range(start, stop[, step]).

        Roughly equivalent to ``choice(range(start, stop, step))`` but
        supports arbitrarily large ranges and is optimized for common cases.

        """

        # This code is a bit messy to make it fast for the
        # common case while still doing adequate error checking.
        try:
            istart = _index(start)
        except TypeError:
            istart = int(start)
            if istart != start:
                _warn('randrange() will raise TypeError in the future',
                      DeprecationWarning, 2)
                raise ValueError("non-integer arg 1 for randrange()")
            _warn('non-integer arguments to randrange() have been deprecated '
                  'since Python 3.10 and will be removed in a subsequent '
                  'version',
                  DeprecationWarning, 2)
        if stop is None:
            # We don't check for "step != 1" because it hasn't been
            # type checked and converted to an integer yet.
            if step is not _ONE:
                raise TypeError('Missing a non-None stop argument')
            if istart > 0:
                return self._randbelow(istart)
            raise ValueError("empty range for randrange()")

        # stop argument supplied.
        try:
            istop = _index(stop)
        except TypeError:
            istop = int(stop)
            if istop != stop:
                _warn('randrange() will raise TypeError in the future',
                      DeprecationWarning, 2)
                raise ValueError("non-integer stop for randrange()")
            _warn('non-integer arguments to randrange() have been deprecated '
                  'since Python 3.10 and will be removed in a subsequent '
                  'version',
                  DeprecationWarning, 2)
        width = istop - istart
        try:
            istep = _index(step)
        except TypeError:
            istep = int(step)
            if istep != step:
                _warn('randrange() will raise TypeError in the future',
                      DeprecationWarning, 2)
                raise ValueError("non-integer step for randrange()")
            _warn('non-integer arguments to randrange() have been deprecated '
                  'since Python 3.10 and will be removed in a subsequent '
                  'version',
                  DeprecationWarning, 2)
        # Fast path.
        if istep == 1:
            if width > 0:
                return istart + self._randbelow(width)
            raise ValueError("empty range for randrange() (%d, %d, %d)" % (istart, istop, width))

        # Non-unit step argument supplied.
        if istep > 0:
            n = (width + istep - 1) // istep
        elif istep < 0:
            n = (width + istep + 1) // istep
        else:
            raise ValueError("zero step for randrange()")
        if n <= 0:
            raise ValueError("empty range for randrange()")
        return istart + istep * self._randbelow(n)
```

OperantKit で使う形への対応:
- `randrange(n)`（trial_based.py L227）→ `_randbelow(n)`
- `randrange(0, n)`（fleshler_hoffman.py L149）→ `0 + _randbelow(n)`
- `randrange(0, 2**31 - 1)`（ratio.py L216, time_based.py L254）→ `_randbelow(2147483647)` → `k = 31`、`getrandbits(31)` を `r >= 2147483647` の間（つまり r が 2^31-1 のときだけ）引き直す。

### (b-10) `Lib/random.py`: `shuffle`（L376-383）

```python
    def shuffle(self, x):
        """Shuffle list x in place, and return None."""

        randbelow = self._randbelow
        for i in reversed(range(1, len(x))):
            # pick an element in x[:i+1] with which to exchange x[i]
            j = randbelow(i + 1)
            x[i], x[j] = x[j], x[i]
```

### (b-11) `Lib/random.py`: `expovariate`（L613-628）

```python
    def expovariate(self, lambd):
        """Exponential distribution.

        lambd is 1.0 divided by the desired mean.  It should be
        nonzero.  (The parameter would be called "lambda", but that is
        a reserved word in Python.)  Returned values range from 0 to
        positive infinity if lambd is positive, and from negative
        infinity to 0 if lambd is negative.

        """
        # lambd: rate lambd = 1/mean
        # ('lambda' is a Python reserved word)

        # we use 1-random() instead of random() to preclude the
        # possibility of taking the log of zero.
        return -_log(1.0 - self.random()) / lambd
```

（`_log` は `from math import log as _log`。C の `log()` に落ちる。）

## (c) Rust 側の仕様

### (c-1) `src/helpers/py_random.rs`（新設。d96582a には存在しない）

登録: `src/helpers/mod.rs`（現在は `pub mod checks;` と `pub mod fleshler_hoffman;` の 2 行）に `pub mod py_random;` を追加。

```text
pub struct PyRandom { state: [u32; 624], index: usize }   // index は 0..=624、seed 直後は 624
impl Clone for PyRandom（derive）
```

| メソッド | 仕様（CPython と逐語対応） |
|---|---|
| `fn from_seed(seed: u64) -> PyRandom` | `init_genrand(19650218)` の後 `init_by_array(key, keyused)`。key は seed の 32 bit little-endian word 列: `seed < 2^32` なら `key = [seed as u32]`（keyused=1。seed=0 も keyused=1, key=[0]）、それ以外は `key = [lo32, hi32]`（keyused=2）。Python の seed は非負 int（負は abs される）。OperantKit が渡す seed は `int | None` で、VI の sub-seed `getrandbits(64)` が最大なので u64 で足りる（64 bit 超の int は対象外） |
| `fn genrand_u32(&mut self) -> u32` | (b-1) のまま。`index >= 624` で 624 word を再生成 |
| `fn random(&mut self) -> f64` | `a = genrand_u32() >> 5; b = genrand_u32() >> 6; (a as f64 * 67108864.0 + b as f64) * (1.0 / 9007199254740992.0)`。a, b < 2^27 / 2^26 なので和は 2^53 未満で f64 上で exact、2^-53 倍も exact。C と bit 一致 |
| `fn getrandbits(&mut self, k: u32) -> u64`（k ≤ 64） | k==0 → 0。k ≤ 32 → `genrand_u32() >> (32-k)`。k > 32 → `words = (k-1)/32+1`、下位 word から順に `genrand_u32()`、残り bit が 32 未満の word は `>> (32-k_remaining)`、`word0 \| word1 << 32` |
| `fn randbelow(&mut self, n: u64) -> u64` | `k = bit_length(n)`（n > 0）、`r = getrandbits(k)` を `r >= n` の間繰り返す |
| `fn randrange(&mut self, start: u64, stop: u64) -> u64` | `start + randbelow(stop - start)`（width > 0 のみ使う）。1 引数形は `randbelow(stop)` |
| `fn expovariate(&mut self, lambd: f64) -> f64` | `-(1.0 - self.random()).ln() / lambd`。**除算の形を保つ**（下記 (c-3) 1 参照） |
| `fn shuffle<T>(&mut self, x: &mut [T])` | `for i in (1..x.len()).rev() { let j = self.randbelow(i as u64 + 1); x.swap(i, j as usize) }` |
| `getstate/setstate` | 対象外（`Clone` で代替。(b-6)） |

### (c-2) `SmallRng` の差し替え点（contingency-rs d96582a、grep 結果）

`Cargo.toml` L23: `rand = { version = "0.8", features = ["small_rng"] }`（Cargo.lock では rand 0.8.6 / rand_core 0.6.4。rand 0.9.4 も lock にあるが推移依存。L36 に `getrandom 0.2`（js feature）があり、seed=None の entropy 取得はこれで足りる）。

| ファイル | 行 | 現状 | 差し替え後（Python 対応） |
|---|---|---|---|
| `src/helpers/fleshler_hoffman.rs` | 15-16 | doc comment「RNG is `SmallRng`…」 | 文言更新 |
| 同 | 21-25 | `use rand::prelude::*; use rand::rngs::SmallRng; use rand::seq::SliceRandom; use rand::{Rng, SeedableRng};` | `use super::py_random::PyRandom;` |
| 同 | 27-32 | `fn make_rng(seed: Option<u64>) -> SmallRng`（`seed_from_u64` / `from_entropy`） | `PyRandom::from_seed(s)` / None は entropy seed |
| 同 | 68-69 | `let mut rng = make_rng(seed); raw.shuffle(&mut rng);`（`generate_intervals`） | `rng.shuffle(&mut raw)`（Python `_shuffled` L55-59） |
| 同 | 105-106 | `out.shuffle(&mut rng);`（`generate_ratios`） | 同上（Python L125: `[float(x) for x in rd]` を shuffle してから int に戻す。値の型は列の順序に影響しないので u64 のまま shuffle してよい） |
| 同 | 118, 125 | `let mut rng = make_rng(seed);` / `rng.gen_range(0..n)`（hantula） | `rng.randbelow(n)`（Python L149 `randrange(0, n)`） |
| `src/schedules/interval.rs` | 48-49 | `use rand::rngs::SmallRng; use rand::{Rng, SeedableRng};` | `PyRandom` |
| 同 | 156 | `master_rng: SmallRng`（VI） | `PyRandom` |
| 同 | 198-205 | `VI::generate`: `let subseed: u64 = master_rng.gen();` | `master_rng.getrandbits(64)`（Python interval.py L206） |
| 同 | 275-276 | `initial_rng: SmallRng, current_rng: SmallRng`（RI） | `PyRandom`（clone による snapshot は Python の getstate/setstate と等価） |
| 同 | 294-297 | `RI::new`: `make_master_rng(seed)` → clone → `draw_exp` | 同じ構造のまま型だけ差し替え |
| 同 | 435-440 | `fn make_master_rng(seed: Option<u64>) -> SmallRng` | `PyRandom` |
| 同 | 442-450 | `fn draw_exp(rng, mean)`: `let r: f64 = rng.gen(); -mean * (1.0 - r).ln()` | `rng.expovariate(1.0 / mean)`（Python L285 `expovariate(1.0 / self._mean_interval)`。(c-3) 1） |
| `src/schedules/ratio.rs` | 31 | `use rand::{rngs::SmallRng, Rng, SeedableRng};` | `PyRandom` |
| 同 | 144 | `rng: SmallRng`（VR） | `PyRandom` |
| 同 | 180-183 | `VR::new`: `SmallRng::seed_from_u64(s)` / `from_entropy()` | `PyRandom::from_seed(s)` |
| 同 | 214-216 | `fn next_sub_seed(&mut self) -> u64 { self.rng.gen::<u64>() }` | `self.rng.randrange(0, 2_147_483_647)`（Python ratio.py L216 `randrange(0, 2**31 - 1)`） |
| 同 | 255-258 | `VR::reset` の再 seed | 同じ構造 |
| 同 | 281-282 | `initial_rng: SmallRng, rng: SmallRng`（RR） | `PyRandom` |
| 同 | 307-311 | `RR::new` の seed と clone | 同じ構造 |
| 同 | 334 | `let draw: f64 = self.rng.gen::<f64>();` | `self.rng.random()`（Python L305） |
| `src/schedules/time_based.rs` | 48-49 | imports | `PyRandom` |
| 同 | 175 | `master_rng: SmallRng`（VT） | `PyRandom` |
| 同 | 201-204 | `VT::new` の seed | `PyRandom::from_seed` |
| 同 | 230-235 | `next_sub_seed`: `self.master_rng.gen::<u64>()`（コメントも SmallRng 前提） | `randrange(0, 2_147_483_647)`（Python time_based.py L254） |
| 同 | 277-280 | `VT::reset` | 同じ構造 |
| 同 | 314-315 | `initial_rng / current_rng: SmallRng`（RT） | `PyRandom` |
| 同 | 335-340 | `RT::new` | 同じ構造 |
| 同 | 356-362 | `draw_interval`: `let r: f64 = rng.gen(); -mean * (1.0 - r).ln()` | `rng.expovariate(1.0 / mean)`（Python L351/L360/L377） |
| `src/schedules/trial_based.rs` | 28-29 | imports | `PyRandom` |
| 同 | 56-61 | `fn make_rng(seed: Option<u64>) -> SmallRng` | `PyRandom` |
| 同 | 94, 413 | `rng: SmallRng` フィールド（MatchingToSample / GoNoGo） | `PyRandom` |
| 同 | 137, 191, 374 | `rng.gen_range(0..n_comparisons)` | `rng.randbelow(n_comparisons)`（Python L227 `randrange(n)`） |
| 同 | 454, 513 | `rng.gen::<f64>() < go_probability` | `rng.random()`（Python L533） |
| `src/schedule.rs` | 19 | doc comment に `SmallRng` の語 | 文言のみ |
| `tests/conformance.rs` | 13-18, 539-549, 552-585, 603-607, 609, 728 | 「bit 再生不能」前提の記述と `#[ignore]` | S-144 の範囲（後述） |

trial_based.rs は 6 fixture には関係しないが、`SmallRng` を残すと rand 依存が残るので S-144 で一緒に差し替える。bindings（`src/python.rs` / `ffi.rs` / `wasm.rs` / `uniffi_api.rs`）に `rand` の直接使用は無い（grep 0 件）ので seed の型 `Option<u64>` は変えなくてよい。

### (c-3) PRNG 以外で bit 等価を崩しうる差分（S-144 で要確認。今回測定した範囲の事実のみ）

1. **expovariate の式の形**: Python は `-log(1-u) / lambd`（`lambd = 1.0/mean` を先に計算）、Rust は `-mean * ln(1-u)`。同じ u でも最後の ulp が違う。測定（CPython 3.11.17、`Random(0)` の u を 200000 個）: mean=5.0 で 71015/200000、mean=3.0 で 64366/200000、mean=10.0 で 71038/200000 が不一致。Rust 側は `expovariate(1.0 / mean)` の形にする。`math.log` と Rust `f64::ln` はどちらもこのホストでは glibc の `log()` に落ちるはず（同一ホスト上での一致は S-143 の固定列テストで確かめる。他プラットフォームの libm 差は未確認）。conformance の比較は `reinforced`（bool）と `Reinforcer.time`（= `now`）なので、ulp 差が結果を変えるのは `now + 1e-9 >= arm_time` の判定が反転するときだけ。
2. **`generate_intervals` の剰余補正**: Python は `math.fsum(raw)`（fleshler_hoffman.py L89）、Rust は `raw.iter().copied().sum()`（fleshler_hoffman.rs L66、逐次加算）。fixture の (v, n) = (10, 12), (5, 12), (3, 12) では両者の和が一致することを測定（`fsum == naive`）したが、一般には一致しない。
3. **`generate_ratios` の丸め**: Python `round(x)` は偶数丸め（round half to even）、Rust `f64::round` は 0 から遠い側への丸め。fixture の進行列（v=5, n=12）に `.5` ちょうどの値は無いことを測定したが、一般には異なる。Rust 1.77 以降の `f64::round_ties_even` が Python と同じ。
4. **sub-seed の bit 幅**: Python の VI sub-seed は 64 bit（`Random(sub)` が 2 word key）、VR/VT の sub-seed は 31 bit 未満（1 word key）。`PyRandom::from_seed(u64)` の key 分割がこれを満たす必要がある（(c-1)）。

### (c-4) 参考: rand 0.8.5 側の該当実装（差分の根拠。取得 URL 併記）

- `SmallRng` は 64 bit 環境で `Xoshiro256PlusPlus`（https://raw.githubusercontent.com/rust-random/rand/0.8.5/src/rngs/small.rs L14）。
- `seed_from_u64` は PCG32 で seed を展開（https://raw.githubusercontent.com/rust-random/rand/rand_core-0.6.4/rand_core/src/lib.rs L335-357）。MT の `init_by_array` とは無関係。
- `slice::shuffle` は `for i in (1..len).rev() { self.swap(i, gen_index(rng, i + 1)) }`（https://raw.githubusercontent.com/rust-random/rand/0.8.5/src/seq/mod.rs L586-592）で、ループの形は Python と同じだが `gen_index` → `gen_range(0..ubound as u32)`（同 L659-665）の標本化が `_randbelow` と異なる。
- `gen::<f64>()` は u64 の上位 53 bit を使う乗算法（https://raw.githubusercontent.com/rust-random/rand/0.8.5/src/distributions/float.rs L107-116）。MT の genrand_res53（27 bit + 26 bit）とは異なる。

## (d) Python で生成した検証用の固定列

生成環境: CPython **3.11.17** (main, Oct 1 2026, 15:41:07) [GCC 13.3.0]、Linux x86_64。コマンド: `python3.11 -I .cloud-handoff/S-142/scripts/gen17.py`（浮動小数は `"%.17g"`）。同じスクリプトを 3.12.3 / 3.13.16 でも実行し、全値が一致することを確認（`.cloud-handoff/S-142/scripts/gen_sequences.py` の JSON 出力を diff）。

`random.Random(42)` を毎回作り直して、各 API を先頭から呼ぶ。

**`random()` × 10**

```
0.63942679845788375
0.025010755222666936
0.27502931836911926
0.22321073814882275
0.7364712141640124
0.67669948742291131
0.89217956770484541
0.086938832629416152
0.42192181968527043
0.029797219438070344
```

**`getrandbits(32)` × 10**

```
2746317213, 478163327, 107420369, 3184935163, 1181241943,
1051802512, 958682846, 599310825, 3163119785, 440213415
```

**`randrange(100)` × 10**

```
81, 14, 3, 94, 35, 31, 28, 17, 94, 13
```

**`expovariate(1.0)` × 10**

```
1.020060287274801
0.025328839042738891
0.32162406407496558
0.25258618567011354
1.3335926728080829
1.1291730087985927
2.2272880995513806
0.090952404606566145
0.54804615939649093
0.030250177207358588
```

**`x = list(range(10)); shuffle(x)` × 1**

```
[7, 3, 2, 8, 5, 6, 9, 4, 0, 1]
```

**seed 直後の内部状態（`Random(42).getstate()[1]`）の先頭 4 word と index**（`init_by_array` の pin 用）

```
state[0..4] = [2147483648, 3564348608, 1266698288, 4212342371], index = 624
```

（`state[0] == 0x80000000` は `init_by_array` 末尾の `mt[0] = 0x80000000U` と整合。）

fixture 固有の先頭 draw 値は (a-2) 末尾に列挙した。機械可読版: `.cloud-handoff/S-142/fixed-sequences.json`（`repr` 表記、3.11 の出力）と `.cloud-handoff/S-142/fixed-sequences-17g.txt`（17 桁表記）。

## S-143 の範囲（文案）

> **S-143 — W12 Python 互換 PRNG: `py_random.rs` の実装と固定列の一致 | 実装 | fable**
> - 範囲: contingency-rs に `src/helpers/py_random.rs` を新設し、`src/helpers/mod.rs` に `pub mod py_random;` を追加する。`pub struct PyRandom { state: [u32; 624], index: usize }`（`Clone`, `Debug`）と、`PyRandom::from_seed(seed: u64)`（CPython `random_seed` の int 経路: seed を 32 bit little-endian word 列に分割して `init_by_array`。seed < 2^32 は 1 word、それ以外は 2 word）、`genrand_u32`、`random`（genrand_res53）、`getrandbits(k: u32) -> u64`（k ≤ 64、下位 word から）、`randbelow(n: u64)`（`_randbelow_with_getrandbits`）、`randrange(start, stop)`、`expovariate(lambd)`（`-(1.0 - random()).ln() / lambd` の除算形を保つ）、`shuffle<T>(&mut [T])`（後ろから `randbelow(i+1)` で swap）を実装する。`getstate/setstate` と `gauss_next` は対象外。seed=None の entropy 経路は `getrandom`（Cargo.toml に既存）で 624 word の key を作って `init_by_array` に渡すか、呼び出し側に任せる（どちらにするかは実装時に決め、doc に書く）。同ファイル内の `#[cfg(test)]` に S-142 (d) の固定列（`random()` 10 個・`getrandbits(32)` 10 個・`randrange(100)` 10 個・`expovariate(1.0)` 10 個・`shuffle` 1 回、浮動小数は bit 一致 `==`）と、fixture の先頭 draw（`from_seed(42).randrange(0, 2147483647) == 1373158606`、`from_seed(7).getrandbits(64) == 17485029721327973432`、`from_seed(3).randrange(0, 2147483647) == 511025150`、`from_seed(17485029721327973432)` が 2 word key で panic しないこと、`from_seed(99).random()` 先頭 5 個、`from_seed(5).expovariate(0.2)` 先頭 3 個、`from_seed(11).expovariate(1.0/3.0)` 先頭 3 個、`from_seed(42)` 直後の `state[0..4] == [2147483648, 3564348608, 1266698288, 4212342371]` と `index == 624`）を pin する。スケジュール側（`src/schedules/*.rs`、`src/helpers/fleshler_hoffman.rs`）と `tests/conformance.rs` は触らない（`SmallRng` はそのまま）。
> - 完了条件: `cargo test py_random` が通る。`cargo fmt --check` と `cargo clippy --all-targets -- -D warnings` が通る（既存の CI 設定に従う）。`.cloud-handoff/S-143.md` に、固定列との一致結果（一致した API と個数）と、`f64::ln` と CPython `math.log` の一致が確認できたホスト（OS・libc）を書く。
> - 依存: S-142（`.cloud-handoff/S-142/conformance-plan-f.md` の (b)(c)(d)）。

## S-144 の範囲（文案）

> **S-144 — W12 Python 互換 PRNG: `SmallRng` の差し替えと stochastic fixture 6 本の bit 等価 | 実装 | fable**
> - 範囲: contingency-rs で `SmallRng` を `PyRandom`（S-143）に差し替える。対象は S-142 (c-2) の表の全行: `src/helpers/fleshler_hoffman.rs`（`make_rng`、`generate_intervals` / `generate_ratios` の shuffle、`generate_intervals_hantula1991` の `gen_range(0..n)` → `randbelow(n)`）、`src/schedules/interval.rs`（`VI::generate` の sub-seed → `getrandbits(64)`、`make_master_rng`、`draw_exp` → `expovariate(1.0 / mean)`、RI の snapshot は clone のまま）、`src/schedules/ratio.rs`（`VR::next_sub_seed` → `randrange(0, 2147483647)`、`RR::step` の `gen::<f64>()` → `random()`）、`src/schedules/time_based.rs`（`VT::next_sub_seed` → `randrange(0, 2147483647)`、`RT::draw_interval` → `expovariate(1.0 / mean)`）、`src/schedules/trial_based.rs`（`make_rng`、`gen_range(0..n_comparisons)` → `randbelow`、`gen::<f64>()` → `random()`）、`src/schedule.rs` L19 の doc comment。あわせて S-142 (c-3) の PRNG 以外の差分を潰す: `generate_intervals` の剰余補正の和を Python `math.fsum` と同じ結果になる方法（Neumaier/Shewchuk 型の補正和）にするか、fixture の (v, n) で一致することを根拠に現状維持とするかを決めて doc に書く。`generate_ratios` の `round` は `f64::round_ties_even`（Python `round` と同じ偶数丸め）にする。`tests/conformance.rs` では L552-585 の 6 テストから `#[ignore]` を外し、`run_stochastic_fixture(...)` を `run_fixture(...)` に置き換える（`replayed_fixtures()` L661 は `run_fixture("` を grep するので、置き換えで completeness guard の対象に入る。L609 `STOCHASTIC_TYPES` と L728 の除外分岐は「stochastic でも `run_fixture` 必須」になるよう整理するか削除する。L13-18、L539-549、L603-607 の「bit 再生不能」の文言を更新。`run_stochastic_fixture` L403-450 は不要になれば削除）。`Cargo.toml` の `rand`（L23）は他に使用箇所が無くなれば外し、entropy は `getrandom`（L36）で取る。`src/schedules/{interval,ratio,time_based}.rs` と `src/helpers/fleshler_hoffman.rs` の既存 unit test のうち seed 付きで決定性だけを見るもの（例: interval.rs L619-620、ratio.rs L515-516）はそのまま通るはずだが、`SmallRng` 固有の値を pin している test が無いかは S-144 で grep して確かめる（S-142 では未確認）。contingency-py は変更しない（`conformance/README.md` L153-156 の「trajectory templates」の記述は Python 側の別作業として引き継ぎに書く）。
> - 完了条件: `cargo test --test conformance` で `atomic_vr_seeded_42` / `atomic_vi_seeded_7` / `atomic_vt_seeded_3` / `atomic_rr_seeded_99` / `atomic_ri_seeded_5` / `atomic_rt_seeded_11` が `--ignored` 無しで bit 等価（`run_fixture`）で通り、`conformance_corpus_completeness` も通る。`cargo test` 全体（bindings の smoke test を含む）、`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings` が通る。CHANGELOG に「VI/VR/VT/RI/RR/RT の seed 付き列が Python と bit 等価になった（seed の意味が変わる = 既存 seed の列は変わる）」を書く。
> - 依存: S-143。
