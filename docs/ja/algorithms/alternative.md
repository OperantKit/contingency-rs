# Alternative — どちらか先に発火する二項複合

:gb: [English version](../../en/algorithms/alternative.md)

## 参考文献

Ferster, C. B., & Skinner, B. F. (1957). *Schedules of reinforcement*.
Appleton-Century-Crofts.

## 数学的定義

2 つのコンポーネント・スケジュールから成る二項複合。各ステップで
同じ `(now, event)` が **両方** のコンポーネントに転送される。
いずれかが強化済み outcome を返したら、両コンポーネントがリセット
され、強化済み outcome が `meta["alternative_winner"] ∈ {"first",
"second"}` とともに表面化される。

両方が同じステップで強化することになった場合、**first が勝つ**。
いずれにせよ両方がリセットされる。

## 状態変数

| 名前 | 型 |
|---|---|
| `first` | Schedule |
| `second` | Schedule |
| `last_now` | float? |

## Step 擬似コード

```
fn step(now, event):
    // 単調 + イベント検証
    first_outcome = first.step(now, event)
    second_outcome = second.step(now, event)

    if first_outcome.reinforced:
        first.reset()
        second.reset()
        return Outcome::reinforced_with_meta(
            first_outcome.reinforcer,
            {"alternative_winner": "first"})
    if second_outcome.reinforced:
        first.reset()
        second.reset()
        return Outcome::reinforced_with_meta(
            second_outcome.reinforcer,
            {"alternative_winner": "second"})
    return Outcome::unreinforced()
```

## Reset セマンティクス

- `first.reset()`, `second.reset()`, `last_now = None`。

## エッジケース

- 厳密に二項。3 個以上のコンポーネントには左結合ネストを使用する:
  `Alternative(Alternative(a, b), c)`（DSL ブリッジの挙動を反映）。
- どちら（もし勝つなら）が勝つかに関わらず、両方のコンポーネントが
  各呼び出しでステップされる — これは反応ベースと時間ベースのコン
  ポーネントが正しく競争できるようにするために必須。
- 勝ち後、コンポーネントはリセットされクロックが再スタートする —
  次のサイクルは新鮮に始まる。

## 決定性

コンポーネントの決定性を継承する。

## Swift オリジナルとの差異

legacy Swift パッケージに Alternative は存在しない。
