# Progressive Ratio — ステップ関数パラメータ化スケジュール

:gb: [English version](../../en/algorithms/progressive-ratio.md)

## 参考文献

Hodos, W. (1961). Progressive ratio as a measure of reward strength.
*Science*, 134(3483), 943-944.
https://doi.org/10.1126/science.134.3483.943

Hursh, S. R. (1980). Economic concepts for the analysis of behavior.
*JEAB*, 34(2), 219-238. https://doi.org/10.1901/jeab.1980.34-219

Richardson, N. R., & Roberts, D. C. S. (1996). Progressive ratio
schedules in drug self-administration studies in rats: A method to
evaluate reinforcing efficacy. *Journal of Neuroscience Methods*,
66(1), 1-11. https://doi.org/10.1016/0165-0270(95)00153-0

## 数学的定義

各連続する強化子は、前回より多くの反応を要求する。強化インデックス
`n`（0 始まり）を要求反応数にマップする規則が **ステップ関数**:

```
r_n = step_fn(n)  ここで r_n >= 1（正整数）
```

`ProgressiveRatio` は `step_fn` をラップし、自身では決して終了しない
（ブレークポイント検出なし — これはセッション・ランナーに存在する）。

## ステップ関数ファミリ

### 算術 (`arithmetic(start, step)`)

`r_n = start + n * step`。制約: `start >= 1`, `step >= 1`。

### 幾何 (`geometric(start, ratio)`)

`r_n = max(1, round(start * ratio ** n))`。
制約: `start >= 1`, `ratio > 1.0`。

### Richardson-Roberts (`richardson_roberts()`)

ハードコードされた 30 要素列（インデックス 0..29）:

```
1, 2, 4, 6, 9, 12, 16, 20, 25, 32, 40, 50, 62, 77, 95, 118,
145, 178, 219, 268, 328, 402, 492, 603, 737, 901, 1102, 1347,
1647, 2012
```

インデックス 29 を超えると、最後の 2 値の比（`2012 / 1647 ≈
1.2217`）を用いて幾何的に外挿する:

```
r_n = max(1, round(2012 * (2012/1647) ** (n - 29)))  (n >= 30 で)
```

## 状態変数

| 名前 | 型 | 用途 |
|---|---|---|
| `step_fn` | callable `(int) -> int` | ステップ関数。 |
| `index` | int | **次** に獲得される強化子の 0 始まりインデックス。 |
| `count` | int | `step_fn(index)` に向けて蓄積された反応数。 |
| `last_now` | float? | 単調時間チェック用。 |

## Step 擬似コード

```
fn step(now, event):
    // 単調 + イベント検証
    last_now = now
    if event is None:
        return Outcome::unreinforced()
    count += 1
    requirement = resolve_requirement(index)
    if count >= requirement:
        count = 0
        index += 1
        return Outcome::reinforced(Reinforcer { time: now })
    return Outcome::unreinforced()

fn resolve_requirement(i):
    v = step_fn(i)
    if v is not int or v < 1:
        raise Config(...)
    return v
```

## Reset セマンティクス

- `index = 0`, `count = 0`, `last_now = None`。
- ステップ関数は保持される（構成の一部）。

## エッジケース

- `step_fn` は callable でなければならない（遅延検証）。
- 無効な戻り値（非 int、`< 1`）は **構築時ではなく**、`step_fn(index)`
  を参照する最初の反応で `Config` を送出する。Rust 移植版もこの
  遅延性を反映すべき。
- ブレークポイント終了なし — スケジュールは永続する。

## 決定性

決定論的（ステップ関数は純粋）。

## Swift オリジナルとの差異

legacy Swift パッケージに PR は存在しない。Richardson-Roberts と
算術 / 幾何ファミリはすべて `contingency-py` で新規。
