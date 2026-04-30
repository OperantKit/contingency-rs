# FT — 固定時間 (Fixed Time)

:gb: [English version](../../en/algorithms/ft.md)

## 参考文献

Zeiler, M. D. (1968). Fixed and variable schedules of
response-independent reinforcement. *Journal of the Experimental
Analysis of Behavior*, 11(4), 405-414.
https://doi.org/10.1901/jeab.1968.11-405

Catania, A. C., & Reynolds, G. S. (1968). A quantitative analysis of
the responding maintained by interval schedules of reinforcement.
*JEAB*, 11(3 Pt 2), 327-383. https://doi.org/10.1901/jeab.1968.11-s327

## 数学的定義

被験体の行動とは無関係に、`interval` 時間単位ごとに強化子が供給
される。強化決定については反応は無視される（ただしタイムスタンプ
は依然として検証される）。

`now - anchor >= interval` になり次第発火する。ここで `anchor` は
スケジュールの内部基準時刻。

## 状態変数

| 名前 | 型 | 用途 |
|---|---|---|
| `interval` | float (const) | 固定強化子間間隔。 |
| `anchor` | float? | 経過計算用の基準時刻。最初のステップ前は `None`。 |
| `last_now` | float? | 単調時間チェック用。 |

## Step 擬似コード

```
fn step(now, event):
    check_monotonic(last_now, now)
    check_event_time(now, event)
    last_now = now
    if anchor is None:          // 初回ステップでアンカリング
        anchor = now
        return Outcome::unreinforced()
    if now - anchor + TIME_TOL >= interval:
        anchor = now
        return Outcome::reinforced(Reinforcer { time: now })
    return Outcome::unreinforced()
```

## Reset セマンティクス

- `anchor = None`
- `last_now = None`

## エッジケース

- `interval <= 0` は `Config` を送出。
- **初回ステップでアンカリング**。`step()` の最初の呼び出しが
  クロックをアンカーし、強化なしを返す。最初の強化子は `now -
  anchor >= interval` を満たす次のステップで発火する。つまり最初の
  ステップが `now = 0` で到着する呼び出し側は `now >= interval` で
  最初の強化子を見る。最初のステップが `now = 5` で到着する呼び出し
  側は `now >= 5 + interval` で最初の強化子を見る。
- **ステップごとに単一発火**。呼び出し側が前回発火から `k > 1` 間隔
  遅れてステップした場合、発生する強化子は **1 つだけ** で、`anchor
  = now`。次の強化子にはさらに完全な `interval` が必要となる。FT は
  見逃した強化子をキューしない。

## 決定性

決定論的。RNG なし。

## Swift オリジナルとの差異

Swift は FT を Rx パイプラインによる外部状態追跡を伴う純粋述語
(`milliseconds >= value`) として扱った。本移植版はアンカーを所有し、
クロック開始時点の曖昧さを排除するため初回ステップアンカリングの
意味論を明示的に定義する。
