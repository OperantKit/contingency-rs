# jspsych-plugin-contingency

:gb: [English README](README.md)

[jsPsych](https://www.jspsych.org/) のプラグインです。この repo の随伴性エンジン（WebAssembly にビルドしたもの）で、1 試行のあいだ 1 つの強化スケジュールを動かします。ボタン 1 つが 1 つのオペランダムです。クリックはエンジンに渡され、スケジュールが強化を配置したときに強化子を提示します。すべての反応と、強化子の提示開始・終了は jsPsych のデータに記録されます。

対応スケジュール: FR、CRF、EXT。

## 使い方

```html
<script src="https://unpkg.com/jspsych@8.3.0"></script>
<!-- contingency_bg.wasm はこのスクリプトと同じディレクトリに置く。 -->
<script src="dist/index.browser.js"></script>
<link href="https://unpkg.com/jspsych@8.3.0/css/jspsych.css" rel="stylesheet" />
<script>
  const jsPsych = initJsPsych();
  jsPsych.run([
    {
      type: jsPsychContingency,
      schedule: { type: "FR", n: 5 },
      reinforcer_duration: 500,
      max_reinforcers: 10,
    },
  ]);
</script>
```

ページは http(s) で配信してください。ブラウザは `file://` の URL から `.wasm` を読み込みません。動く 1 ページは [`examples/fr5.html`](examples/fr5.html) にあります。

## パラメータ

| 名前 | 型 | 既定 | 説明 |
|---|---|---|---|
| `schedule` | object | 必須 | `{ type: "FR", n: 5 }`・`{ type: "CRF" }`・`{ type: "EXT" }` |
| `operanda` | string[] | `["main"]` | オペランダムごとにボタンを 1 つ出す。名前はエンジンに渡す |
| `button_html` | function | `<button class="jspsych-btn">` | ボタン 1 つの HTML を返す `(operandum, index) => html` |
| `stimulus` | HTML 文字列 | `""` | ボタンの上に出す |
| `reinforcer_html` | HTML 文字列 | 大きな `+` | 強化子の提示中、刺激の代わりに出す |
| `reinforcer_duration` | int (ms) | 1000 | 強化子の提示時間。0 なら提示開始と終了を同じ時刻に記録する |
| `responses_during_reinforcer` | `"ignore"` / `"count"` | `"ignore"` | 強化子の提示中のクリックを記録してエンジンに渡すか |
| `trial_duration` | int (ms) | null | この時間で試行を終える |
| `max_reinforcers` | int | null | この回数の強化で試行を終える（最後の提示が終わってから） |
| `tick_interval` | int (ms) | 50 | 反応が無いときにエンジンを進める周期（時間に基づくスケジュール用） |
| `end_key` | キー | null | 試行を終えるキー |
| `wasm_url` | string | null | `contingency_bg.wasm` の場所。既定はプラグインのスクリプトと同じ場所 |

`trial_duration`・`max_reinforcers`・`end_key` のどれかは指定してください。どれも無いと試行が終わりません。

## データ

| 欄 | 説明 |
|---|---|
| `schedule`, `schedule_label` | `schedule` パラメータと、`FR 5` のような読める表記 |
| `operanda` | `operanda` パラメータ |
| `trial_start_total_ms` | 試行開始時の `jsPsych.getTotalTime()` |
| `events` | 時刻順の `{ t, type, id, operandum, potency? }`。`t` は試行開始からの秒、`type` は `response`・`reinforcer_start`・`reinforcer_end`、`id` は試行内で反応と強化子を別々に 0 から数えた番号、`operandum` はボタンの番号（時間だけで配置された強化子では `null`） |
| `n_responses`, `n_reinforcers` | 件数 |
| `end_reason` | `duration`・`max_reinforcers`・`end_key` |

## 開発

Node.js と、`wasm-pack` の入った Rust ツールチェーンが要ります（Node の版は `.mise.toml` で固定）。

```sh
npm install
npm run wasm:build      # Rust のソースから ../pkg をビルド
npm run build           # dist/（ESM・CJS・<script> 用）と contingency_bg.wasm
npx playwright test     # このディレクトリを http で配信し、試行をクリックで進める
```

テストは jsPsych を CDN でなく `node_modules` から配信するので、オフラインで走ります。

## ライセンス

MIT（この repo の他の部分と同じ）。
