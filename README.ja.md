# contingency-rs

:gb: [English README](README.md)

Rust 製強化スケジュールエンジン。[`contingency-py`](../contingency-py/) の Rust 移植。実行可能仕様は Python パッケージの `contingency-py/conformance/` 配下の 20 件の適合フィクスチャ — 本クレートはそれらをリプレイして検証する。

## スコープ

- 原子的スケジュール: FR, VR, RR, CRF, FI, VI, RI, FT, VT, RT, EXT
- Limited Hold ラッパ
- 複合スケジュール: Concurrent (+ COD + COR), Multiple, Chained, Tandem, Alternative
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

## 意味論的不変条件 (Python 移植と共通)

詳細は `docs/correspondence.md` と Python パッケージの `docs/handoff-summary.md` を参照。主なもの:

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
- Reynolds, G. S. (1961). Behavioral contrast. *JEAB*, 4(1), 57-71.
- Hodos, W. (1961). Progressive ratio as a measure of reward strength. *Science*, 134, 943-944.
- Hursh, S. R. (1980). Economic concepts for the analysis of behavior. *JEAB*, 34(2), 219-238.

## ライセンス

MIT. [LICENSE](LICENSE) を参照。
