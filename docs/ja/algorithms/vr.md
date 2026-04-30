# VR — 変動比率 (Variable Ratio)

:gb: [English version](../../en/algorithms/vr.md)

## 参考文献

Ferster, C. B., & Skinner, B. F. (1957). *Schedules of reinforcement*.
Appleton-Century-Crofts.

Fleshler, M., & Hoffman, H. S. (1962). A progression for generating
variable-interval schedules. *Journal of the Experimental Analysis of
Behavior*, 5(4), 529-530. https://doi.org/10.1901/jeab.1962.5-529

## 数学的定義

比率要件は算術平均 `mean` を持つ Fleshler-Hoffman 進行からサンプル
される。各強化時、シーケンス内の次の比率要件が活性要件となる。
シーケンスが尽きると、マスタ RNG から導出されたサブシードを用いて
新しいシーケンスが生成される。

生成器定義は `fleshler-hoffman.md` を参照。

## 状態変数

| 名前 | 型 | 用途 |
|---|---|---|
| `mean` | float (const) | 目標とする算術平均比率。 |
| `n_intervals` | int (const) | サイクルごとのプールサイズ（デフォルト 12）。 |
| `seed` | int? (const) | 決定性のためのマスタシード。 |
| `rng` | Random | マスタ RNG。サイクルごとのサブシードを導出。 |
| `sequence` | list[int] | 現在の比率要件プール。 |
| `cursor` | int | `sequence` へのインデックス。 |
| `count` | int | 現在の要件に向けて蓄積された反応数。 |
| `requirement` | int | 現在の比率要件 (`sequence[cursor]`)。 |
| `last_now` | float? | 単調時間チェック用。 |

## Step 擬似コード

```
fn step(now, event):
    check_monotonic(last_now, now)
    check_event_time(now, event)
    last_now = now
    if event is None:
        return Outcome::unreinforced()
    count += 1
    if count >= requirement:
        count = 0
        advance_requirement()
        return Outcome::reinforced(Reinforcer { time: now })
    return Outcome::unreinforced()

fn advance_requirement():
    cursor += 1
    if cursor >= len(sequence):
        reload_sequence()
    else:
        requirement = sequence[cursor]

fn reload_sequence():
    sub_seed = rng.randrange(0, 2**31 - 1)
    sequence = fleshler_hoffman::generate_ratios(mean, n_intervals, seed=sub_seed)
    cursor = 0
    requirement = sequence[0]
```

## Reset セマンティクス

- `rng = Random(seed)`（保存されたマスタシードから再シード）
- `count = 0`
- `last_now = None`
- シーケンス再生成: `reload_sequence()` — `seed` が設定されている
  場合、ビット同一の軌跡を生む。

## エッジケース

- `mean <= 0` は `Config` を送出。
- `n_intervals < 1` は `Config` を送出。
- サブシード定義域は `[0, 2**31 - 1)`。Rust は Python の `Random` と
  のビット同一出力を保つため同じ範囲を使う必要がある。

## 決定性

`seed` が供給されれば完全に決定論的。言語横断でのビット同一性は
Rust 移植が `random.Random(seed).randrange(0, 2**31 - 1)` と
`random.Random(sub_seed).shuffle(...)` — Python の Mersenne
Twister — を再現する場合のみ成り立つ。`contingency-rs` で推奨される
アプローチは、シード付きランダム列を不透明な **fixture 入力** として
扱うこと: conformance コーパスは VR 軌跡をシードで固定するが、Rust
実装は異なる PRNG を使用し、代わりに `sequence` フィールドを JSON
から読み込む fixture をリプレイして検証してよい。

(`conformance/` コーパス参照: 確率的 fixture は該当する場合、生の
`sequence` プールを含めており、Rust はこれを直接消費できる。)

## Swift オリジナルとの差異

Swift の VR はリアクティブ・ストリームから供給される可変値を伴う
FR に委譲していた。Fleshler-Hoffman 生成器はプールを外部で生成して
いた。本移植版は生成器を VR クラスに束縛し、シーケンス枯渇時に
プールが決定論的に再生成されるようにしている。
