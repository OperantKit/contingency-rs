# CRF & EXT — 極限スケジュール

:gb: [English version](../../en/algorithms/crf-ext.md)

## 参考文献

Skinner, B. F. (1957). *Schedules of reinforcement* (with C. B.
Ferster). Appleton-Century-Crofts.

## CRF — 連続強化 (Continuous Reinforcement)

すべての反応が強化される。CRF は比率 1 の FR の極限ケース。

### 定義

```python
def CRF() -> FR:
    return FR(1)
```

クラスではなくファクトリ。Rust 移植版は
`Crf::new() -> Fr { Fr::new(1) }` を提供すべき。

## EXT — 消去 (Extinction)

どの反応も決して強化されない。`step()` は `now` や `event` に関わら
ず、常に強化なしの `Outcome` を返す。

### 状態変数

| 名前 | 型 | 用途 |
|---|---|---|
| `last_now` | float? | 単調時間チェック用。 |

他の状態なし。

### Step 擬似コード

```
fn step(now, event):
    check_monotonic(last_now, now)
    check_event_time(now, event)
    last_now = now
    return Outcome::unreinforced()
```

### Reset セマンティクス

- `last_now = None`

### 決定性

完全に決定論的。RNG なし。

### Swift オリジナルとの差異

意味論は同一。Swift 実装は純粋述語から `false` を返していた。本移植
版は強化なしの `Outcome` を返し、さらに `Schedule` プロトコルの単調
時間契約も強制する。
