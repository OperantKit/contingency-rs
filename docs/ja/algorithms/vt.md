# VT — 変動時間 (Variable Time)

:gb: [English version](../../en/algorithms/vt.md)

## 参考文献

Zeiler, M. D. (1968). Fixed and variable schedules of
response-independent reinforcement. *JEAB*, 11(4), 405-414.
https://doi.org/10.1901/jeab.1968.11-405

Fleshler, M., & Hoffman, H. S. (1962). A progression for generating
variable-interval schedules. *JEAB*, 5(4), 529-530.
https://doi.org/10.1901/jeab.1962.5-529

## 数学的定義

（FT と同じく）反応非依存の強化であるが、強化子間間隔は平均
`mean_interval` の Fleshler-Hoffman 進行からサンプルされる。プールが
尽きると、決定論的に新しいプールが再生成される。

## 状態変数

VR と同じ。ただし反応カウンタを `anchor` が置き換える:

| 名前 | 型 | 用途 |
|---|---|---|
| `mean` | float (const) | 目標算術平均。 |
| `n_intervals` | int (const) | サイクルごとのプールサイズ（デフォルト 12）。 |
| `seed` | int? (const) | マスタシード。 |
| `rng` | Random | サブシード生成用のマスタ RNG。 |
| `sequence` | list[float] | 現在の間隔長プール。 |
| `cursor` | int | プールへのインデックス。 |
| `requirement` | float | 現在の間隔長 (`sequence[cursor]`)。 |
| `anchor` | float? | 経過時間基準。最初のステップ前は `None`。 |
| `last_now` | float? | 単調時間チェック用。 |

サブシード定義域: `rng.randrange(0, 2**31 - 1)`（VR と同じ）。

## Step 擬似コード

```
fn step(now, event):
    check_monotonic(last_now, now)
    check_event_time(now, event)
    last_now = now
    if anchor is None:
        anchor = now
        return Outcome::unreinforced()
    if now - anchor + TIME_TOL >= requirement:
        anchor = now
        advance_requirement()
        return Outcome::reinforced(Reinforcer { time: now })
    return Outcome::unreinforced()
```

## Reset セマンティクス

- `rng = Random(seed)`
- シーケンスを決定論的に再生成（構築直後と同じ軌跡）
- `anchor = None`
- `last_now = None`

## エッジケース

- VR と同じ条件に加えて、FT の初回ステップアンカリングとステップ
  ごとの単一発火。

## 決定性

VR/VI と同じ PRNG に関する注意。言語横断 fixture はシーケンスを
直接固定する。

## Swift オリジナルとの差異

Swift の VT は別個の Fleshler-Hoffman 生成器から供給される次の間隔を
伴う FT に委譲していた。本移植版は生成器を内部に束縛する。
