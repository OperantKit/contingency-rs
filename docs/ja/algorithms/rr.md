# RR — ランダム比率 (Random Ratio)

:gb: [English version](../../en/algorithms/rr.md)

## 参考文献

Ferster, C. B., & Skinner, B. F. (1957). *Schedules of reinforcement*.
Appleton-Century-Crofts.

## 数学的定義

各反応は確率 `p ∈ (0, 1]` で独立に強化される。ベルヌーイ引きは
統計的に独立であり、反応数に対して無記憶である。

## 状態変数

| 名前 | 型 | 用途 |
|---|---|---|
| `p` | float (const) | 強化確率。 |
| `rng` | Random | ベルヌーイ引きの供給源。 |
| `initial_state` | Random.State | `reset()` 用の構築時 RNG スナップショット。 |
| `last_now` | float? | 単調時間チェック用。 |

## Step 擬似コード

```
fn step(now, event):
    check_monotonic(last_now, now)
    check_event_time(now, event)
    last_now = now
    if event is None:
        return Outcome::unreinforced()
    if rng.random() < p:
        return Outcome::reinforced(Reinforcer { time: now })
    return Outcome::unreinforced()
```

## Reset セマンティクス

- `rng.setstate(initial_state)` — ベルヌーイ列を復元。
- `last_now = None`

初期状態スナップショットは `reset()` ごとではなく構築時に取得される。
そのため、`Random` をスケジュール間で共有する呼び出し側は、共有
RNG が他所で進められていても、スケジュール **自身の** 初期引きに
戻ることを観測する。

## エッジケース

- `p <= 0` または `p > 1` は `Config` を送出。
- `p == 1.0` は CRF / FR(1) と等価。
- `rng=None` は内部で新しくシード無しの `Random()` を生成する。
  したがって決定論的な conformance fixture には、構築時にシード付き
  `Random` を供給する必要がある。

## 決定性

`rng` が供給され、かつシードが設定されていれば決定論的。言語横断で
のビット同一性については VR と同じ PRNG に関する注意が当てはまる。
conformance fixture では、JSON `expect` ブロックにステップごとの
強化決定を bool として直接書き込み、決定を固定する。

## Swift オリジナルとの差異

Swift の RR はランタイムサンプル値を伴う FR に委譲していた。本
移植版は直接のベルヌーイ過程。Swift の比率サンプラを `p = 1 / mean`
の幾何引きとして解釈する場合、両者は統計的に等価。
