# RT — ランダム時間 (Random Time)

:gb: [English version](../../en/algorithms/rt.md)

## 参考文献

Catania, A. C., & Reynolds, G. S. (1968). A quantitative analysis of
the responding maintained by interval schedules of reinforcement.
*JEAB*, 11(3 Pt 2), 327-383. https://doi.org/10.1901/jeab.1968.11-s327

## 数学的定義

（FT と同じく）反応非依存の強化。強化子間間隔は
`Exp(1 / mean_interval)` からサンプルされる。無記憶: 経過時間に
かかわらず一定のハザード `1 / mean_interval`。

## 状態変数

| 名前 | 型 | 用途 |
|---|---|---|
| `mean` | float (const) | 指数分布の平均。 |
| `rng` | Random | `expovariate` の供給源。 |
| `initial_state` | Random.State | 構築時のスナップショット。 |
| `requirement` | float | 現在の間隔長（構築時に引かれ、発火後に再度引かれる）。 |
| `anchor` | float? | 基準時刻。最初のステップ前は `None`。 |
| `last_now` | float? | 単調時間チェック用。 |

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
        requirement = rng.expovariate(1 / mean)
        return Outcome::reinforced(Reinforcer { time: now })
    return Outcome::unreinforced()
```

## Reset セマンティクス

- `rng.setstate(initial_state)`
- `requirement = rng.expovariate(1 / mean)`（新たな初期引き）
- `anchor = None`
- `last_now = None`

## エッジケース

- `mean <= 0` は `Config` を送出。
- 初回ステップアンカリングとステップごと単一発火の意味論は FT と
  同じ。
- 最初の `requirement` はステップ発生前の構築時に引かれるため、
  初期引きは構築時の RNG 状態で取得される。`reset()` 後は引きが
  正確に再現される。

## 決定性

conformance fixture は連続する `requirement` 値を固定する。

## Swift オリジナルとの差異

Swift RT は外部供給の間隔を伴う FT に委譲していた。RI と同じ構造的
差異。
