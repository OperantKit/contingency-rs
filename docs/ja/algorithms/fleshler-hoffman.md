# Fleshler-Hoffman — VI / VR プール生成器

:gb: [English version](../../en/algorithms/fleshler-hoffman.md)

## 参考文献

Fleshler, M., & Hoffman, H. S. (1962). A progression for generating
variable-interval schedules. *Journal of the Experimental Analysis of
Behavior*, 5(4), 529-530. https://doi.org/10.1901/jeab.1962.5-529

Hantula, D. A. (1991). A simple BASIC program to generate values for
variable-interval schedules of reinforcement. *Journal of Applied
Behavior Analysis*, 24(4), 799-801.
https://doi.org/10.1901/jaba.1991.24-799

## 目的

算術平均が目標値 `v` に等しい `n` 個の間隔（または比率）値のプール
を生成する。シードが供給された場合、プールは決定論的にシャッフル
される。VI, VR, VT によって内部利用される。

## アルゴリズム（raw progression）

`m ∈ 1..n` について:

- `m == n` の場合:

  ```
  vi[m] = v * (1 + log(n))
  ```

- それ以外:

  ```
  s1 = (1 + log(n)) + (n - m) * log(n - m)
  s2 = (n - m + 1) * log(n - m + 1)
  vi[m] = v * (s1 - s2)
  ```

これは算術平均が概ね `v` になる `n` 個の実数値リストを生成する。

## 公開関数

### `generate_intervals(v, n=12, seed=None) -> list[float]`

1. raw progression `vi[1..n]` を計算。
2. 平均保存: `vi[0] += v * n - sum(vi)` を調整し、シャッフル後の
   プールの平均が float 丸めの範囲内で正確に `v` になるようにする。
   Python は float 誤差を最小化するため合計に `math.fsum` を使用。
3. `random.Random(seed)` を用いて in-place シャッフル。
4. シャッフルされたリストを返す。

`n == 0` の場合は空プール。

### `generate_ratios(v, n=12, seed=None) -> list[int]`

1. raw progression を計算。
2. 各値を正の整数に丸める: `max(1, round(vi[m]))`。
3. 末尾を補正:
   - `head_sum = sum(rd[:-1])`、`target_total = round(v * n)` とする。
   - `surplus = target_total - head_sum`。
   - `surplus >= 1` なら `rd[-1] = surplus`。
   - そうでなければ `rd[-1] = 1`、`surplus -= 1` とし、その後、末尾
     から歩いて `rd[i] >= 2` の要素を 1 減らし `surplus` を 1 増やす
     操作を `surplus >= 0` になるまで行う。これは Swift のフォール
     バックに一致する。
4. `random.Random(seed)` でシャッフル。
5. 整数を返す。

### `generate_intervals_hantula1991(v, n=12, seed=None) -> list[int]`

Hantula (1991) の BASIC プログラム・バリアントを再現: リトライ
ループ（Hantula の GOTO 130）を用いて、ゼロ初期化されたスロット
へのランダム配置を行う。`generate_intervals` と同じ平均だが整数値
で、Hantula 固有の配置戦略を持つ。使用頻度は低く、文献との整合性
のために含まれている。

## 決定性

`seed` が供給されれば決定論的。言語横断の注意: Python の
`random.Random` は Mersenne Twister である。conformance fixture は
生成プールを直接固定するため、Rust は MT をビット単位で再現せずに
それを消費できる。

## エッジケース

- `v` は `int` でも `float` でもよい。内部で `float` に強制変換される。
- `n == 0` → 空リスト。
- `generate_ratios` では、raw progression の四捨五入が十分大きく
  すでに head の合計が目標を超えている場合、「余剰フォールバック」
  経路が発動する — ループが末尾から歩き、`>= 2` の値を減らして
  バランスが回復するまで続ける。これは Swift ソースのフォール
  バック経路に一致する。

## Swift オリジナルとの差異

アルゴリズム的に同一。Python 移植版は:

1. float ドリフトを減らすため、単純合計の代わりに `math.fsum` を
   使用する。
2. 主要生成器から Hantula 式のリトライループを削除する
  （`random.shuffle` 経由で同じ分布を生成する）。
3. 文献を正確に再現するため `generate_intervals_hantula1991` を
   利用可能なまま残す。
