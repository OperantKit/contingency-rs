# jspsych-plugin-contingency

:gb: [English README](README.md)

[jsPsych](https://www.jspsych.org/) のプラグインです。この repo の随伴性エンジン（WebAssembly にビルドしたもの）で、1 試行のあいだ 1 つの強化スケジュールを動かします。ボタン 1 つが 1 つのオペランダムです。クリックはエンジンに渡され、スケジュールが強化を配置したときに強化子を提示します。すべての反応と、強化子の提示開始・終了は jsPsych のデータに記録されます。

対応スケジュール: FR、VI、CRF、EXT。記録したセッションは [OKL v1](https://github.com/OperantKit/OperantKitLog) のファイルとして保存できます（[OKL で保存する](#okl-で保存する) 参照）。

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

ページは http(s) で配信してください。ブラウザは `file://` の URL から `.wasm` を読み込みません。動くページは [`examples/fr5.html`](examples/fr5.html) と [`examples/vi.html`](examples/vi.html)（VI 10 秒、終了時に OKL ファイルをダウンロード）にあります。

## パラメータ

| 名前 | 型 | 既定 | 説明 |
|---|---|---|---|
| `schedule` | object | 必須 | `{ type: "FR", n: 5 }`・`{ type: "VI", mean: 30, n: 12, seed: 1 }`・`{ type: "CRF" }`・`{ type: "EXT" }`。VI の `mean` は秒、`n` は Fleshler-Hoffman の間隔の個数（既定 12）、`seed`（省略可）は間隔の順序を固定する。次の間隔は強化された反応の時点から始まるので、強化子の提示時間を含む |
| `operanda` | string[] | `["main"]` | オペランダムごとにボタンを 1 つ出す。名前はエンジンに渡す |
| `button_html` | function | `<button class="jspsych-btn">` | ボタン 1 つの HTML を返す `(operandum, index) => html` |
| `stimulus` | HTML 文字列 | `""` | ボタンの上に出す |
| `reinforcer_html` | HTML 文字列 | 大きな `+` | 強化子の提示中、刺激の代わりに出す |
| `reinforcer_duration` | int (ms) | 1000 | 強化子の提示時間。0 なら提示開始と終了を同じ時刻に記録する |
| `responses_during_reinforcer` | `"ignore"` / `"count"` | `"ignore"` | 強化子の提示中のクリックを記録してエンジンに渡すか |
| `trial_duration` | int (ms) | null | この時間で試行を終える |
| `max_reinforcers` | int | null | この回数の強化で試行を終える（最後の提示が終わってから）。最後の提示中の反応は記録するが、エンジンには渡さない |
| `tick_interval` | int (ms) | 50 | 反応が無いときにエンジンを進める周期（時間に基づくスケジュール用） |
| `end_key` | キー | null | 試行を終えるキー |
| `wasm_url` | string | null | `contingency_bg.wasm` の場所。既定はプラグインのスクリプトと同じ場所 |

`trial_duration`・`max_reinforcers`・`end_key` のどれかは指定してください。どれも無いと試行が終わりません。

## データ

| 欄 | 説明 |
|---|---|
| `schedule`, `schedule_label` | `schedule` パラメータと、`FR 5`・`VI 30s` のような contingency-dsl の表記 |
| `operanda` | `operanda` パラメータ |
| `trial_start_total_ms` | 試行開始時の `jsPsych.getTotalTime()` |
| `trial_start_perf_ms`, `trial_end_s` | 試行開始時の `performance.now()` と、試行の長さ（秒）。どちらも `events` と同じ時計 |
| `events` | 時刻順の `{ t, type, id, operandum, potency? }`。`t` は試行開始からの秒、`type` は `response`・`reinforcer_start`・`reinforcer_end`、`id` は試行内で反応と強化子を別々に 0 から数えた番号、`operandum` はボタンの番号（時間だけで配置された強化子では `null`）。反応で生じた強化子の開始時刻はその反応の `t` と同じ。試行の終了時にまだ提示中の強化子には、試行の終了時刻で `reinforcer_end` を記録する |
| `n_responses`, `n_reinforcers` | 件数 |
| `end_reason` | `duration`・`max_reinforcers`・`end_key` |

## OKL で保存する

`jsPsychContingency.toOkl(data, options)` は、実験のうち contingency の試行を 1 つの OKL v1 セッション（文字列）にします。`jsPsychContingency.downloadOkl(text, filename)` はそれをファイルとしてダウンロードさせます。jsPsych の `localSave` は CSV と JSON しか書けません。

```js
const jsPsych = initJsPsych({
  on_finish: () => {
    const okl = jsPsychContingency.toOkl(jsPsych.data.get(), {
      session_name: "s01",
      subject_id: "p01",                                       // 省略可
      wall_clock_start: jsPsych.getStartTime().toISOString(), // 省略可
      meta: { jspsych_version: jsPsych.version() },           // 省略可
    });
    jsPsychContingency.downloadOkl(okl, "s01.txt");
  },
});
```

- contingency の 1 試行が 1 つのフェーズになります。ラベル `trial <trial_index> <schedule_label>` の `phase_enter`、その試行のイベント、試行終了時の `phase_exit` の順です。ほかのプラグインの試行は書き出しません。
- 時刻は実験開始からの秒です（`clock_type = "PerformanceNow"`、`session_start = 0.0`）。時刻はすべて `performance.now()` の時計で、jsPsych 自身の時計は、0 秒を実験開始に合わせるために 1 回だけ使います。反応と強化子の番号はセッション全体で振り直します。
- 各試行の `schedule` は `meta` に JSON で残します（`trial_<trial_index>_schedule`）。プラグインのデータを持たない contingency の記録（`jsPsych.abortExperiment` で終わった試行など）は書き出さず、その件数を `meta` の `skipped_trials` に書きます。
- OKL v1 で表せない文字列（TAB・LF・CR 以外の制御文字と U+0085・U+2028・U+2029）や 2^53 を超える整数があると、読み戻すと値が変わるファイルを書く代わりに、`toOkl` は例外を投げます。
- 書き出すのは OKL v1 の標準のイベント型（`response`・`reinforcer_start`・`reinforcer_end`・`phase_enter`・`phase_exit`）だけなので、OKL v1 の読み手ならどれでも読めます（例: Python の `session_recorder.reader.read_log`）。
- ほかに指定できるもの: `replication_index`・`experiment_name`・`protocol_id`。`meta` には `plugin_version` を自動で足します。

## 開発

Node.js と、`wasm-pack` の入った Rust ツールチェーンが要ります（Node の版は `.mise.toml` で固定）。

```sh
npm install
npm run wasm:build      # Rust のソースから ../pkg をビルド
npm run build           # dist/（ESM・CJS・<script> 用）と contingency_bg.wasm
npm run test:e2e        # ビルドしてから、このディレクトリを http で配信し、試行をクリックで進める
```

テストは jsPsych を CDN でなく `node_modules` から配信するので、オフラインで走ります。OKL のテストは書き出したファイルを Python のパッケージ `session-recorder` で読み戻します。それを import できるインタプリタを `SESSION_RECORDER_PYTHON` に指定してください（既定は `python3`。import できないときはその旨を出してテストを失敗にします）。

## ライセンス

MIT（この repo の他の部分と同じ）。
