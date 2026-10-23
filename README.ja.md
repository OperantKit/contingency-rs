# contingency-rs

:gb: [English README](README.md)

Rust 製強化スケジュールエンジン。[`contingency-py`](https://github.com/OperantKit/contingency-py) の Rust 移植。実行可能仕様は Python パッケージの `contingency-py/conformance/` 配下の 39 件の適合フィクスチャ — 本クレートはそれらをリプレイして検証する。

## スコープ

- 原子的スケジュール: FR, VR, RR, CRF, FI, VI, RI, FT, VT, RT, EXT
- Limited Hold ラッパ
- 複合スケジュール: Concurrent (+ COD + FRCO), Multiple, Chained, Tandem, Alternative
- 分化強化: DRO (Resetting / Momentary), DRL, DRH
- 累進比率 (Progressive Ratio) + ステップ関数 (arithmetic, geometric, Richardson-Roberts)
- Fleshler-Hoffman VI/VR 生成器 (1962 + Hantula 1991)
- `contingency-hil` バイナリ (HAL JSONL ワイヤプロトコル)

## ビルド

```sh
cargo build --release
cargo test
```

### フィーチャーフラグ

- `python` — PyO3 拡張モジュール `contingency_core` をビルド
- `uniffi` — UniFFI 経由で Swift / Kotlin / KMP スキャフォルディングをビルド
- `serial` — シリアルポート経由の装置バックエンド (`hw::serial_backend`) を有効化

## 30 秒で動く例

本クレートは crates.io に公開していないため、git 依存として追加する (並立コンポーネントのキーに `indexmap` を使う):

```toml
[dependencies]
contingency = { git = "https://github.com/OperantKit/contingency-rs" }
indexmap = "2"
```

並立 VI 30 秒 VI 60 秒スケジュールを切替遅延 (COD) 2 秒で組み、模擬反応で駆動して、オペランダムごとの強化子数を数える:

```rust
use contingency::{ResponseEvent, ScheduleBuilder};
use indexmap::IndexMap;

fn main() -> contingency::Result<()> {
    // 並立 VI 30 秒 VI 60 秒、COD 2 秒。Fleshler-Hoffman 間隔 (n = 12) をシード固定で生成。
    let mut components = IndexMap::new();
    components.insert("left".to_string(), ScheduleBuilder::vi(30.0, 12, Some(42))?);
    components.insert("right".to_string(), ScheduleBuilder::vi(60.0, 12, Some(7))?);
    let mut schedule = ScheduleBuilder::concurrent(components, 2.0, 0)?;

    // 模擬被験体: 0.5 秒ごとに 1 反応、10 秒ごとにオペランダムを切り替え、10 分間。
    let (mut left, mut right) = (0, 0);
    for i in 1..=1200 {
        let now = i as f64 * 0.5;
        let operandum = if (i / 20) % 2 == 0 { "left" } else { "right" };
        let outcome = schedule.step(now, Some(&ResponseEvent::on(operandum, now)))?;
        if outcome.reinforced {
            if operandum == "left" { left += 1 } else { right += 1 }
        }
    }
    println!("left={left} right={right}"); // left=16 right=6 — シード固定なので毎回同じ
    assert_eq!((left, right), (16, 6));
    Ok(())
}
```

`schedule.step(now, event)` は時刻を `now` まで進める。`event` は反応なら `Some(&ResponseEvent)`、時間経過だけなら `None` で、戻り値の `Outcome.reinforced` が `now` で強化子を提示するかを表す。同じエンジンは Python 版 [contingency-py](https://github.com/OperantKit/contingency-py) (`pip install contingency-py`) としても使え、こちらは [contingency-dsl](https://github.com/OperantKit/contingency-dsl) で書いたスケジュールをそのままコンパイルできる。

## 自作の 50 行では足りない理由

短い自作ループは次の点を誤りやすく、どれも報告するデータを変えてしまう:

- **変動スケジュールの間隔.** VI/VR の値は Fleshler-Hoffman (1962) の数列で生成する (Hantula, 1991 の変法も `helpers::fleshler_hoffman` にある)。場当たり的な一様乱数や指数乱数ではない。`n_intervals` と `seed` はコンストラクタの明示的な引数なので、実行を正確に再現できる。
- **並立スケジュールの切替遅延.** `Concurrent` は各反応をそのオペランダムをキーとするコンポーネントに振り分け、他のコンポーネントは `None` で進めて時間ベースのコンポーネントを止めない。切替後 `cod` 時間単位はどの反応も強化せず、その間に設定された強化子は COD 明けの最初の反応まで保持する (Herrnstein, 1961)。固定比率の切替要件 (`frco`) と方向別 COD (`Concurrent::with_extensions`) にも対応する。
- **Python 版との適合性.** `tests/conformance.rs` は [contingency-py](https://github.com/OperantKit/contingency-py) の 39 件の JSON フィクスチャをリプレイする。決定的な 31 件 (嫌悪制御とラッパー型を含む) は Python の `reinforced` と強化子 (時刻・強度・ラベル) がビット単位 (誤差 1e-9 以内) で一致しなければならず、決定的フィクスチャを追加してリプレイを書き忘れると網羅性テストが失敗する。シード付きの 8 件 (VR, VI, RI, RR, VT, RT と試行型の MatchingToSample, GoNoGo) は両実装の擬似乱数生成器が異なるため `#[ignore]` にしており、`cargo test -- --ignored` で強化回数だけを確かめる。
- **性質テスト.** `tests/properties.rs` の 13 個の `proptest` 性質が、Fleshler-Hoffman の平均保存、VR/VI/RR のシード決定性、DRL/DRH の閾値、時刻の逆行の拒否などの不変条件を確かめる。
- **1 つのエンジンを複数の環境で.** 同じスケジュールを C ABI (`include/contingency.h`、cbindgen で生成)、PyO3 (`python` フィーチャー)、WebAssembly (`wasm32` ターゲット)、UniFFI 経由の Swift/Kotlin (`uniffi` フィーチャー) から使える。`tests/e2e_c_ffi.rs` は実際の C プログラムをライブラリに対してコンパイル・実行する。
- **入力の検証.** 時刻の逆行は `ContingencyError::State` を返し、不正なパラメータ (正でない平均間隔、2 未満の並立コンポーネント数など) は `ContingencyError::Config` を返す。

## 意味論的不変条件 (Python 移植と共通)

詳細は `docs/en/correspondence.md` (日本語版は `docs/ja/correspondence.md`) と Python パッケージの `docs/handoff-summary.md` を参照。主なもの:

- `TIME_TOL = 1e-9` を monotonic / event-time 検査に一貫適用。
- First-step anchoring: FT/VT/RT/DRO は最初の `step()` で anchor、FI/VI/RI は構築時に anchor。
- `Concurrent` は全コンポーネントを毎 step advance。`Chained`/`Tandem` はアクティブコンポーネントのみ step。
- COD は event にマッチしたコンポーネントのみ抑制 (他オペランダムの tick 強化は素通し)。
- Momentary DRO は半開区間 `[anchor, now)` を使用。
- `RR`/`RI`/`RT` は構築時に RNG state をスナップショットし、`reset()` で同一のドロー列をリプレイ。

## 参考文献

- Ferster, C. B., & Skinner, B. F. (1957). *Schedules of reinforcement*. Appleton-Century-Crofts.
- Fleshler, M., & Hoffman, H. S. (1962). A progression for generating variable-interval schedules. *JEAB*, 5(4), 529-530. https://doi.org/10.1901/jeab.1962.5-529
- Hantula, D. A. (1991). A simple BASIC program to generate values for variable-interval schedules of reinforcement. *JABA*, 24(4), 799-801.
- Catania, A. C. (1966). Concurrent operants. In W. K. Honig (Ed.), *Operant behavior* (pp. 213-270). Appleton-Century-Crofts.
- Herrnstein, R. J. (1961). Relative and absolute strength of response as a function of frequency of reinforcement. *JEAB*, 4(3), 267-272. https://doi.org/10.1901/jeab.1961.4-267
- Reynolds, G. S. (1961). Behavioral contrast. *JEAB*, 4(1), 57-71.
- Hodos, W. (1961). Progressive ratio as a measure of reward strength. *Science*, 134, 943-944.
- Hursh, S. R. (1980). Economic concepts for the analysis of behavior. *JEAB*, 34(2), 219-238.

## ライセンス

MIT. [LICENSE](LICENSE) を参照。
