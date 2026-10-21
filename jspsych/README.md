# jspsych-plugin-contingency

:jp: [日本語版 README](README.ja.md)

A [jsPsych](https://www.jspsych.org/) plugin that runs a reinforcement schedule from the contingency engine (this repository, compiled to WebAssembly) for the length of one trial. Each button is an operandum. Clicks are passed to the engine, and a reinforcer is presented whenever the schedule arranges one. Every response and every reinforcer onset/offset is written to the jsPsych data.

Supported schedules: FR, VI, CRF, EXT. The recorded session can be saved as an [OKL v1](https://github.com/OperantKit/OperantKitLog) file (see [Saving as OKL](#saving-as-okl)).

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

The page must be served over http(s); browsers do not fetch `.wasm` from `file://` URLs. Complete pages are in [`examples/fr5.html`](examples/fr5.html) and [`examples/vi.html`](examples/vi.html) (VI 10 s, downloads an OKL file at the end).

## Parameters

| Name | Type | Default | Description |
|---|---|---|---|
| `schedule` | object | required | `{ type: "FR", n: 5 }`, `{ type: "VI", mean: 30, n: 12, seed: 1 }`, `{ type: "CRF" }` or `{ type: "EXT" }`. For VI, `mean` is in seconds, `n` is the number of Fleshler-Hoffman intervals (default 12) and `seed` (optional) fixes their order. The next interval starts at the reinforced response, so it includes the reinforcer presentation time |
| `operanda` | string[] | `["main"]` | One button per operandum; the names are passed to the engine |
| `button_html` | function | `<button class="jspsych-btn">` | `(operandum, index) => html` for one button |
| `stimulus` | HTML string | `""` | Shown above the buttons |
| `reinforcer_html` | HTML string | a large `+` | Shown in place of the stimulus while a reinforcer is presented |
| `reinforcer_duration` | int (ms) | 1000 | Reinforcer presentation time; 0 records onset and offset at the same time |
| `responses_during_reinforcer` | `"ignore"` / `"count"` | `"ignore"` | Whether clicks during reinforcer presentation are recorded and passed to the engine |
| `trial_duration` | int (ms) | null | End the trial after this time |
| `max_reinforcers` | int | null | End the trial after this many reinforcers (after the last presentation ends). Responses during that last presentation are recorded but not passed to the engine |
| `tick_interval` | int (ms) | 50 | Period at which the engine is stepped without a response (for time-based schedules) |
| `end_key` | key | null | Key that ends the trial |
| `wasm_url` | string | null | Location of `contingency_bg.wasm`; by default, next to the plugin script |

At least one of `trial_duration`, `max_reinforcers` or `end_key` should be set; otherwise the trial does not end.

## Data

| Field | Description |
|---|---|
| `schedule`, `schedule_label` | The `schedule` parameter and its contingency-dsl notation, such as `FR 5` or `VI 30s` |
| `operanda` | The `operanda` parameter |
| `trial_start_total_ms` | `jsPsych.getTotalTime()` at the start of the trial |
| `trial_start_perf_ms`, `trial_end_s` | `performance.now()` at the start of the trial, and the trial length in seconds, on the clock of `events` |
| `events` | `{ t, type, id, operandum, potency? }` in time order. `t` is seconds from the trial start; `type` is `response`, `reinforcer_start` or `reinforcer_end`; `id` counts responses and reinforcers separately from 0 within the trial; `operandum` is the button index (`null` for reinforcers arranged by time alone). A reinforcer produced by a response starts at that response's `t`; a reinforcer still being presented when the trial ends gets its `reinforcer_end` at the trial end |
| `n_responses`, `n_reinforcers` | Counts |
| `end_reason` | `duration`, `max_reinforcers` or `end_key` |

## Saving as OKL

`jsPsychContingency.toOkl(data, options)` turns the contingency trials of an experiment into one OKL v1 session (a string); `jsPsychContingency.downloadOkl(text, filename)` offers it as a file. jsPsych's own `localSave` writes only CSV and JSON.

```js
const jsPsych = initJsPsych({
  on_finish: () => {
    const okl = jsPsychContingency.toOkl(jsPsych.data.get(), {
      session_name: "s01",
      subject_id: "p01",                                       // optional
      wall_clock_start: jsPsych.getStartTime().toISOString(), // optional
      meta: { jspsych_version: jsPsych.version() },           // optional
    });
    jsPsychContingency.downloadOkl(okl, "s01.txt");
  },
});
```

- Each contingency trial becomes one phase: `phase_enter` with the label `trial <trial_index> <schedule_label>`, its events, then `phase_exit` at the trial end. Trials of other plugins are skipped.
- Timestamps are seconds from the start of the experiment (`clock_type = "PerformanceNow"`, `session_start = 0.0`). All of them are on the `performance.now()` clock; jsPsych's own clock is used once, to place time 0 at the experiment start. Response and reinforcer ids are renumbered through the whole session.
- The `schedule` of each trial is kept in `meta` as JSON (`trial_<trial_index>_schedule`). Contingency records without the plugin's data (for example a trial ended by `jsPsych.abortExperiment`) are skipped and counted in `meta` as `skipped_trials`.
- Strings that OKL v1 cannot represent (control characters other than TAB, LF and CR, and U+0085 / U+2028 / U+2029) and integers beyond 2^53 make `toOkl` throw instead of writing a file that reads back differently.
- Only canonical OKL v1 event types are written (`response`, `reinforcer_start`, `reinforcer_end`, `phase_enter`, `phase_exit`), so any OKL v1 reader can load the file, for example `session_recorder.reader.read_log` in Python.
- Other options: `replication_index`, `experiment_name`, `protocol_id`. `plugin_version` is added to `meta` automatically.

## Development

Node.js and the Rust toolchain with `wasm-pack` are required (`.mise.toml` pins Node).

```sh
npm install
npm run wasm:build      # builds ../pkg from the Rust sources
npm run build           # dist/ (ESM, CJS and <script> builds) plus contingency_bg.wasm
npm run test:e2e        # builds, serves this directory over http and clicks through the trials
```

The tests serve jsPsych from `node_modules` instead of the CDN, so they run offline. The OKL test reads the exported file back with the Python package `session-recorder`; set `SESSION_RECORDER_PYTHON` to an interpreter that can import it (default `python3`; the test fails, with that message, when the import fails).

## License

MIT, as the rest of this repository.
