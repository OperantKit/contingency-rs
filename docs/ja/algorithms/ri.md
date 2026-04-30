# RI — ランダム間隔 (Random Interval)

:gb: [English version](../../en/algorithms/ri.md)

## 参考文献

Catania, A. C., & Reynolds, G. S. (1968). A quantitative analysis of
the responding maintained by interval schedules of reinforcement.
*Journal of the Experimental Analysis of Behavior*, 11(3, Pt. 2),
327-383. https://doi.org/10.1901/jeab.1968.11-s327

## 数学的定義

強化子間の各間隔は指数分布 `Exp(1 / mean_interval)` から独立に
サンプルされる。過程は無記憶: 任意の瞬間に次の強化子が `[t, t+dt)`
で利用可能になるハザードは経過時間と無関係に `dt / mean_interval`
である。FI/VI と同様に、強化子を受け取るには反応が必要。

## 状態変数

| 名前 | 型 | 用途 |
|---|---|---|
| `mean_interval` | float (const) | 指数分布の平均。 |
| `rng` | Random | `expovariate` 引きの供給源。 |
| `initial_state` | Random.State | 構築時のスナップショット。 |
| `arm_time` | float | 次の強化利用可能時刻。`rng.expovariate(1/mean)` に初期化。 |
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
        arm_time = now + rng.expovariate(1 / mean_interval)
        return Outcome::reinforced(Reinforcer { time: now })
    return Outcome::unreinforced()
```

## Reset セマンティクス

- `rng.setstate(initial_state)`
- `arm_time = rng.expovariate(1 / mean_interval)`（新たな初期引き）
- `last_now = None`

注: `reset()` 後の最初の引きは復元された RNG 状態から来るため、
構築直後と同じ初期 `arm_time` を生成する。

## エッジケース

- `mean_interval <= 0` は `Config` を送出。
- legacy ソースの RI の Swift オリジナル（FI 上の no-op ラッパ）と
  同様、現在の実装は間隔機構の上に指数サンプリングを追加している。

## LimitedHold フック

- `_arm_time`, `_withdraw_and_rearm(now)`（新しい指数間隔を引く）。

## 決定性

RR と同じ PRNG の注意が当てはまる。RI の conformance fixture は、
各 `arm_time` を fixture に事前記録することで引き列を固定すべき。

## Swift オリジナルとの差異

Swift ファイル `RI.swift` は `FI` への薄いデリゲート。実際の指数
サンプリングはリアクティブ・パイプラインから外部供給されていた。
本移植版は指数サンプリングを明示的かつ内部的にする。
