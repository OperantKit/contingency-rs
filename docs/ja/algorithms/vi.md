# VI — 変動間隔 (Variable Interval)

:gb: [English version](../../en/algorithms/vi.md)

## 参考文献

Ferster, C. B., & Skinner, B. F. (1957). *Schedules of reinforcement*.
Appleton-Century-Crofts.

Fleshler, M., & Hoffman, H. S. (1962). A progression for generating
variable-interval schedules. *Journal of the Experimental Analysis of
Behavior*, 5(4), 529-530. https://doi.org/10.1901/jeab.1962.5-529

## 数学的定義

連続する強化子間の間隔は、算術平均 `mean_interval` を持つシャッ
フルされた Fleshler-Hoffman 進行からサンプルされる。FI と同様に、
強化子を受け取るには反応が必要 — tick のみでは十分でない。プール
が尽きると、マスタ RNG から引かれるサブシードを用いて新しいプール
が生成される。

## 状態変数

| 名前 | 型 | 用途 |
|---|---|---|
| `mean_interval` | float (const) | 目標算術平均。 |
| `n_intervals` | int (const) | サイクルごとのプールサイズ（デフォルト 12）。 |
| `seed` | int? (const) | マスタシード。 |
| `rng` | Random | サブシードを引く (`rng.getrandbits(64)`)。 |
| `sequence` | list[float] | 現在の間隔長プール。 |
| `cursor` | int | `sequence` への次インデックス（1 から始まる。インデックス 0 は `arm_time` で消費）。 |
| `arm_time` | float | 次の強化目標時刻（`sequence[0]` に初期化）。 |
| `last_now` | float? | 単調時間チェック用。 |

## Step 擬似コード

```
fn step(now, event):
    check_monotonic(now, last_now)
    check_event_time(now, event)
    last_now = now
    if event is None:
        return Outcome::unreinforced()
    if now + TIME_TOL >= arm_time:
        arm_time = now + next_interval()
        return Outcome::reinforced(Reinforcer { time: now })
    return Outcome::unreinforced()

fn next_interval() -> float:
    if cursor >= len(sequence):
        sequence = generate_sequence()   // rng.getrandbits(64) で新プール
        cursor = 0
    value = sequence[cursor]
    cursor += 1
    return value
```

## Reset セマンティクス

- `rng = Random(seed)`
- 決定論的にシーケンスを再生成
- `cursor = 1`
- `arm_time = sequence[0]`
- `last_now = None`

## エッジケース

- `mean_interval <= 0` は `Config` を送出。
- `n_intervals < 1` は `Config` を送出。
- サブシード定義域: `rng.getrandbits(64)` — 64 ランダムビットを
  Fleshler-Hoffman 生成器にシードとして渡す。

## LimitedHold フック

- `_arm_time` (read)
- `_withdraw_and_rearm(now)` — `arm_time = now + next_interval()`

## 決定性

VR と同じ論理: Python 内では決定論的。言語横断でのビット同一性は、
Rust に Python の Mersenne Twister を再現させるのではなく、
conformance fixture で **sequence** を固定することで追跡する。

## Swift オリジナルとの差異

Swift の VI は `FleshlerHoffman` 生成器から供給される次の値を伴う
FI に委譲していた。本移植版は生成器を VI スケジュールに束縛し、
マスタ RNG からサブシードを導出することでシーケンス再生成を
決定論的にする。
