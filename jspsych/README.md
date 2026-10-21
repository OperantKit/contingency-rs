# jspsych-plugin-contingency

:jp: [日本語版 README](README.ja.md)

A [jsPsych](https://www.jspsych.org/) plugin that runs a reinforcement schedule from the contingency engine (this repository, compiled to WebAssembly) for the length of one trial. Each button is an operandum. Clicks are passed to the engine, and a reinforcer is presented whenever the schedule arranges one. Every response and every reinforcer onset/offset is written to the jsPsych data.

Supported schedules: FR, CRF, EXT.

## Usage

```html
<script src="https://unpkg.com/jspsych@8.3.0"></script>
<!-- contingency_bg.wasm must sit in the same directory as this script. -->
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

The page must be served over http(s); browsers do not fetch `.wasm` from `file://` URLs. A complete page is in [`examples/fr5.html`](examples/fr5.html).

## Parameters

| Name | Type | Default | Description |
|---|---|---|---|
| `schedule` | object | required | `{ type: "FR", n: 5 }`, `{ type: "CRF" }` or `{ type: "EXT" }` |
| `operanda` | string[] | `["main"]` | One button per operandum; the names are passed to the engine |
| `button_html` | function | `<button class="jspsych-btn">` | `(operandum, index) => html` for one button |
| `stimulus` | HTML string | `""` | Shown above the buttons |
| `reinforcer_html` | HTML string | a large `+` | Shown in place of the stimulus while a reinforcer is presented |
| `reinforcer_duration` | int (ms) | 1000 | Reinforcer presentation time; 0 records onset and offset at the same time |
| `responses_during_reinforcer` | `"ignore"` / `"count"` | `"ignore"` | Whether clicks during reinforcer presentation are recorded and passed to the engine |
| `trial_duration` | int (ms) | null | End the trial after this time |
| `max_reinforcers` | int | null | End the trial after this many reinforcers (after the last presentation ends) |
| `tick_interval` | int (ms) | 50 | Period at which the engine is stepped without a response (for time-based schedules) |
| `end_key` | key | null | Key that ends the trial |
| `wasm_url` | string | null | Location of `contingency_bg.wasm`; by default, next to the plugin script |

At least one of `trial_duration`, `max_reinforcers` or `end_key` should be set; otherwise the trial does not end.

## Data

| Field | Description |
|---|---|
| `schedule`, `schedule_label` | The `schedule` parameter and a readable form such as `FR 5` |
| `operanda` | The `operanda` parameter |
| `trial_start_total_ms` | `jsPsych.getTotalTime()` at the start of the trial |
| `events` | `{ t, type, id, operandum, potency? }` in time order. `t` is seconds from the trial start; `type` is `response`, `reinforcer_start` or `reinforcer_end`; `id` counts responses and reinforcers separately from 0 within the trial; `operandum` is the button index (`null` for reinforcers arranged by time alone) |
| `n_responses`, `n_reinforcers` | Counts |
| `end_reason` | `duration`, `max_reinforcers` or `end_key` |

## Development

Node.js and the Rust toolchain with `wasm-pack` are required (`.mise.toml` pins Node).

```sh
npm install
npm run wasm:build      # builds ../pkg from the Rust sources
npm run build           # dist/ (ESM, CJS and <script> builds) plus contingency_bg.wasm
npx playwright test     # serves this directory over http and clicks through the trials
```

The tests serve jsPsych from `node_modules` instead of the CDN, so they run offline.

## License

MIT, as the rest of this repository.
