# 分化強化 — DRO / DRL / DRH

:gb: [English version](../../en/algorithms/dro-drl-drh.md)

## 参考文献

Ferster, C. B., & Skinner, B. F. (1957). *Schedules of reinforcement*.
Appleton-Century-Crofts.

Reynolds, G. S. (1961). Behavioral contrast. *JEAB*, 4(1), 57-71.
https://doi.org/10.1901/jeab.1961.4-57

Reynolds, G. S. (1964). Accurate and rapid reconditioning of
spaced-responding by differential reinforcement of other behavior.
*JEAB*, 7(3), 223-224. https://doi.org/10.1901/jeab.1964.7-223

Zeiler, M. D. (1977). Schedules of reinforcement: The controlling
variables. In W. K. Honig & J. E. R. Staddon (Eds.), *Handbook of
operant behavior* (pp. 201-232). Prentice-Hall.

---

## DRO — 他行動の分化強化 (Differential Reinforcement of Other behavior)

一定間隔にわたる反応の **不在** を強化する。2 つのバリアントが
サポートされる。

### Resetting バリアント（デフォルト）

各反応は DRO タイマーを `now` にリセットする。間に反応を挟むこと
なく `now - anchor >= interval` を満たす最初の **tick** で強化子が
供給される。強化後、タイマーは `now` で再スタートする。

間隔境界にちょうどまたはそれ以降に到着する反応は **強化しない** —
いかなる反応もタイマーをリセットする。境界ベースの強化には
momentary バリアントを使用すること。

### Momentary バリアント

タイマーは反応とは独立に連続して動く。各間隔境界 (`now >= anchor +
interval`) で、`[anchor, now)` — ちょうど `now` の反応は次のウィン
ドウに属する半開ウィンドウ — に **反応がない** 場合のみ強化子が
供給される。アンカーは outcome に関わらず各境界で `now` に進む。

### 状態変数

| 名前 | 型 | 用途 |
|---|---|---|
| `interval` | float (const) | ウィンドウ長 (`> 0`)。 |
| `type` | "resetting" \| "momentary" | バリアント。 |
| `anchor` | float? | タイマー / ウィンドウ開始。最初のステップ前は `None`。 |
| `has_response_in_window` | bool | momentary のみ: 現在のウィンドウ開始後に反応が発生したか。 |
| `last_now` | float? | 単調時間チェック用。 |

### Step 擬似コード

```
fn step(now, event):
    // 単調 + イベント検証
    last_now = now
    if anchor is None:
        anchor = now
        if event is not None:
            has_response_in_window = true
        return Outcome::unreinforced()
    if type == "resetting":
        if event is not None:
            anchor = now                // タイマーリセット。強化なし。
            return Outcome::unreinforced()
        if now - anchor + TIME_TOL >= interval:
            anchor = now
            return Outcome::reinforced(Reinforcer { time: now })
        return Outcome::unreinforced()
    else:  // momentary
        reinforced = false
        if now - anchor + TIME_TOL >= interval:
            if not has_response_in_window:
                reinforced = true
            anchor = now
            has_response_in_window = false
        if event is not None:
            has_response_in_window = true
        if reinforced:
            return Outcome::reinforced(Reinforcer { time: now })
        return Outcome::unreinforced()
```

### Reset セマンティクス

- `anchor = None`, `has_response_in_window = false`, `last_now = None`。

### 決定性

決定論的。RNG なし。

---

## DRL — 低頻度反応の分化強化 (Differential Reinforcement of Low rate)

反応間時間 (IRT) が少なくとも `interval` 以上の反応を強化する。

### 定義

`now` における反応は、以下を満たす場合に強化される:

- 構築 / リセット以降の **最初** の反応であるか、**または**
- 前回の反応が `now` より少なくとも `interval` 前に発生している
  （TOL の余裕を伴って `now - last_response_time >= interval`）。

強化されるか否かに関わらず、**すべて** の反応が
`last_response_time` を更新する。`event=None` の tick は決して強化
せず、単調時間チェック以外の状態変更も行わない。

### 状態

| 名前 | 型 |
|---|---|
| `interval` | float (const) |
| `last_response_time` | float? |
| `last_now` | float? |

### Step 擬似コード

```
fn step(now, event):
    // 単調 + イベント検証
    last_now = now
    if event is None:
        return Outcome::unreinforced()
    prev = last_response_time
    last_response_time = now         // 常に更新
    if prev is None or now - prev + TIME_TOL >= interval:
        return Outcome::reinforced(Reinforcer { time: now })
    return Outcome::unreinforced()
```

### Reset

`last_response_time = None`, `last_now = None`。

---

## DRH — 高頻度反応の分化強化 (Differential Reinforcement of High rate)

直近 `time_window` 時間単位（スライディング・ウィンドウ）内に少な
くとも `response_count` 個の反応が発生しているときに反応を強化する。

### 定義

直近の反応タイムスタンプの FIFO を維持する。各ステップ（tick か
反応か）で、`now - time_window` より厳密に古いタイムスタンプを
（TOL の余裕を伴って）evict する。反応時、右側に `now` を追加する。
ウィンドウが `>= response_count` を保持していれば、強化する。

ウィンドウは強化時に **空にならない** — 持続する高頻度応答列は、
各該当する反応で強化子を生成し続ける。

### 状態

| 名前 | 型 |
|---|---|
| `response_count` | int (const, `>= 1`) |
| `time_window` | float (const, `> 0`) |
| `window` | deque[float] |
| `last_now` | float? |

### Step 擬似コード

```
fn step(now, event):
    // 単調 + イベント検証
    last_now = now
    evict_old(now)                        // w[0] < cutoff - TOL を除去
    if event is None:
        return Outcome::unreinforced()
    window.push_back(now)
    if len(window) >= response_count:
        return Outcome::reinforced(Reinforcer { time: now })
    return Outcome::unreinforced()

fn evict_old(now):
    cutoff = now - time_window
    while window and window[0] < cutoff - TIME_TOL:
        window.pop_front()
```

### Reset

`window.clear()`, `last_now = None`。

### エッジケース

- `response_count < 1` または `time_window <= 0`: `Config`。
- 境界: ちょうど `now - time_window` のタイムスタンプはウィンドウ
  内に留まる（TOL 余裕付きの包含的）。
- DSL ブリッジが使用する DRH は常に `response_count = 2` で構成
  される: DSL の `DRHNs` 表現は時間値のみを運び、最大 IRT として
  解釈される。

## 決定性

3 つすべて決定論的。RNG なし。

## Swift オリジナルとの差異

これらのスケジュールは legacy Swift パッケージに存在しない。3 つ
すべて `contingency-py` で新規。
