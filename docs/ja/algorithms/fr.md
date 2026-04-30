# FR — 固定比率 (Fixed Ratio)

:gb: [English version](../../en/algorithms/fr.md)

## 参考文献

Ferster, C. B., & Skinner, B. F. (1957). *Schedules of reinforcement*.
Appleton-Century-Crofts.

## 数学的定義

比率要件 `n >= 1` が与えられたとき、`k` 番目の反応で `k mod n == 0` を
満たすたびに（すなわち `n` 反応ごとに）強化子が供給される。

## 状態変数

| 名前 | 型 | 用途 |
|---|---|---|
| `n` | int (const) | 比率要件 (`>= 1`)。 |
| `count` | int | 前回の強化子以降に蓄積された反応数。 |
| `last_now` | float? | 単調チェック用の最後に観測された `now`。 |

## Step 擬似コード

```
fn step(now, event):
    check_monotonic(last_now, now)
    check_event_time(now, event)
    last_now = now
    if event is None:
        return Outcome::unreinforced()
    count += 1
    if count >= n:
        count = 0
        return Outcome::reinforced(Reinforcer { time: now })
    return Outcome::unreinforced()
```

## Reset セマンティクス

- `count = 0`
- `last_now = None`

## エッジケース

- `n = 1` は CRF と等価。すべての反応が強化される。
- `n < 1` は `Config` エラーを送出。
- `n` は `int` でなければならない（bool は拒否）。
- `None` イベントは決して `count` を進めない。

## 決定性

決定論的。RNG なし。

## Swift オリジナルとの差異

Swift 実装は Rx リアクティブ・ストリームによって駆動される純粋関数
的な述語 (`numOfResponses >= value`) だった。本移植版はカウンタ状態
を所有し、強化時にリセットする。リアクティブ・チェインのアキュムレ
ータを「強化サイクルごとのカウント」として解釈する場合、意味論は
一致する。
