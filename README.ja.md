# contingency-rs

:gb: [English README](README.md)

contingency-rs は、OperantKit の強化スケジュールエンジンの Rust 版です。FR 5 や並立 VI VI のようなスケジュールと、起きた反応を順に渡すと、強化子をいつ提示するかを決めます。実験箱や課題を動かすプログラムが Python でないときに使います。たとえば Rust や C の制御プログラム、ブラウザの実験（WebAssembly。[jsPsych](https://www.jspsych.org/) 用プラグインつき）、Swift や Kotlin のアプリ、ハードウェア・イン・ザ・ループ（HIL）の試験装置です。[OperantKit](https://github.com/OperantKit) の中では [contingency-py](https://github.com/OperantKit/contingency-py) の移植にあたり、実行可能な仕様は Python パッケージの 39 個の適合性 fixture です。本クレートはそれらを再生して検証します。

- **contingency-py と同じ結果になります。** `tests/conformance.rs` は、乱数の種を持つ 8 本を含む 39 本すべての fixture を再生します。CPython の `random.Random` を移植しているので、種付きの抽選も一致します。
- **変動スケジュールの値は Fleshler–Hoffman (1962) の数列で作ります。** 並立スケジュールでは切替遅延（COD）を扱います。COD は、オペランダム（レバーやキー）を切り替えた直後に反応を強化しない時間のことです。
- **1 つのエンジンを複数の環境から呼べます。** Rust・C・Python・WebAssembly・Swift/Kotlin が同じスケジュールのコードを使います。

AI のコーディングエージェント向けの案内は [AGENTS.md](AGENTS.md) にあります。

## インストール

クレート（名前は `contingency`）は crates.io に登録されていません。`Cargo.toml` に git の依存として加えます。並立スケジュールの成分に名前を付けるため `indexmap` も必要です。

```toml
[dependencies]
contingency = { git = "https://github.com/OperantKit/contingency-rs" }
indexmap = "2"
```

## 例

COD 2 秒の並立 VI 30 秒 VI 60 秒スケジュールを、0.5 秒ごとに反応し 10 秒ごとにオペランダムを切り替える仮想被験体で 10 分間動かします。

```rust
use contingency::{ResponseEvent, ScheduleBuilder};
use indexmap::IndexMap;

fn main() -> contingency::Result<()> {
    // 並立 VI 30 秒 VI 60 秒、COD 2 秒。Fleshler-Hoffman の間隔（n = 12）、種は固定。
    let mut components = IndexMap::new();
    components.insert("left".to_string(), ScheduleBuilder::vi(30.0, 12, Some(42))?);
    components.insert("right".to_string(), ScheduleBuilder::vi(60.0, 12, Some(7))?);
    let mut schedule = ScheduleBuilder::concurrent(components, 2.0, 0)?;

    // 仮想被験体: 0.5 秒ごとに 1 反応、10 秒ごとにオペランダムを切り替え、10 分間。
    let (mut left, mut right) = (0, 0);
    for i in 1..=1200 {
        let now = i as f64 * 0.5;
        let operandum = if (i / 20) % 2 == 0 { "left" } else { "right" };
        let outcome = schedule.step(now, Some(&ResponseEvent::on(operandum, now)))?;
        if outcome.reinforced {
            if operandum == "left" { left += 1 } else { right += 1 }
        }
    }
    println!("left={left} right={right}"); // left=16 right=8 — 毎回同じで、同じ種の contingency-py とも一致する
    assert_eq!((left, right), (16, 8));
    Ok(())
}
```

`schedule.step(now, event)` は時計を `now` まで進めます。`event` は反応なら `Some(&ResponseEvent)`、時間の経過だけなら `None` です。`outcome.reinforced` は `now` に強化子を提示するかどうかを表します。

## 含まれるもの

| 分野 | 内容 |
|---|---|
| 単一スケジュール | FR、VR、RR、CRF、FI、VI、RI、FT、VT、RT、EXT、リミテッドホールド |
| 複合スケジュール | 並立（COD、固定比率の切替要件、方向別の COD つき）、多元、混合、連鎖、連接、選択、連言、挿入、重畳、二次スケジュール |
| 分化強化と累進 | DRO（リセット型・瞬時型）、DRL、DRH、パーセンタイル、累進比率（等差・等比・Richardson–Roberts）、調整、連動 |
| その他の手続き | タイムアウト、反応コスト、Sidman 型回避、弁別回避、逃避、見本合わせ、Go/No-Go |
| 値の生成 | Fleshler–Hoffman（Fleshler & Hoffman, 1962）と Hantula (1991) の変法。`helpers::fleshler_hoffman` にあります |
| ハードウェア抽象化層（`hw`） | 装置のインタフェースと、仮想の装置、シリアルポート接続（`serial` 機能） |
| HIL ブリッジ | `contingency-hil` バイナリ。改行区切りの JSON で反応を受け取り、スケジュールを動かして強化子を送り返す TCP サーバです（`contingency-hil --help`） |
| バインディング | C（`include/contingency.h`、cbindgen で生成）、Python（`python` 機能、モジュール名 `contingency_core`）、WebAssembly（`wasm32` ターゲット）、Swift/Kotlin（`uniffi` 機能） |
| jsPsych プラグイン | [`jspsych/`](jspsych/README.ja.md): ブラウザで 1 試行のあいだ FR・VI・CRF・EXT を動かします |

Cargo の機能（feature）は `python`・`uniffi`・`serial` で、既定ではどれも無効です。

2 つのエンジンが共有する振る舞い（時間の許容差 1e-9、各スケジュールがいつ時間を数え始めるか、COD が強化をどう止めるか、など）は [Python と Rust の対応表](docs/ja/correspondence.md) の 5 節にあります。スケジュールのインタフェースは [protocol.md](docs/ja/protocol.md) に書いてあります。

## 制限

- crates.io に登録されておらず、API は変わることがあります。
- contingency-dsl の式を読むパーサはありません。文字で書いたスケジュールを動かすには contingency-py で組み立てるか、ここで `ScheduleBuilder` を使って組み立てます。
- `contingency-hil` は `stimulus` メッセージを捨て、ホストから送られた `reinforcer` メッセージも無視します。
- jsPsych プラグインが扱えるのは FR・VI・CRF・EXT だけです。

## 開発

```sh
git clone https://github.com/OperantKit/contingency-rs
git clone https://github.com/OperantKit/contingency-py   # the conformance tests read ../contingency-py/conformance/
cd contingency-rs
cargo test
```

両方の README の Rust のコードブロックは doctest として実行されます（`cargo test --doc`）。

## 参考文献

- Fleshler, M., & Hoffman, H. S. (1962). A progression for generating variable-interval schedules. *Journal of the Experimental Analysis of Behavior*, 5(4), 529-530. https://doi.org/10.1901/jeab.1962.5-529
- Hantula, D. A. (1991). A simple BASIC program to generate values for variable-interval schedules of reinforcement. *Journal of Applied Behavior Analysis*, 24(4), 799-801. https://doi.org/10.1901/jaba.1991.24-799

## ライセンス

MIT です。[LICENSE](LICENSE) を参照してください。
