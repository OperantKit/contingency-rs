import { expect, test } from "@playwright/test";

import { escapeTsv, toOkl } from "../src/okl";

// Pure-function tests of the OKL v1 writer; no browser is involved.

const trial = (overrides: object = {}) => ({
  trial_type: "contingency",
  trial_index: 0,
  time_elapsed: 4000,
  schedule: { type: "FR", n: 2 },
  schedule_label: "FR 2",
  operanda: ["main"],
  trial_start_total_ms: 1000,
  // The performance clock is 500 ms ahead of jsPsych's clock in these records.
  trial_start_perf_ms: 1500,
  trial_end_s: 3.0,
  events: [
    { t: 0.5, type: "response", id: 0, operandum: 0 },
    { t: 1.0, type: "response", id: 1, operandum: 0 },
    { t: 1.0, type: "reinforcer_start", id: 0, operandum: 0, potency: 1 },
    { t: 1.5, type: "reinforcer_end", id: 0, operandum: 0 },
  ],
  n_responses: 2,
  n_reinforcers: 1,
  end_reason: "max_reinforcers",
  ...overrides,
});

const bodyOf = (text: string) => text.slice(text.indexOf("# ---\n") + "# ---\n".length);

test("header follows the order of the conformance fixtures and declares the canonical types", () => {
  const text = toOkl([trial()], { session_name: "s1" });
  expect(text.slice(0, text.indexOf("# ---\n") + "# ---\n".length)).toBe(
    [
      "# OKL v1",
      '# session_name = "s1"',
      '# clock_type = "PerformanceNow"',
      "# session_start = 0.0",
      "# meta:",
      '#   trial_0_schedule = "{\\"type\\":\\"FR\\",\\"n\\":2}"',
      "# events:",
      "#   response          : id:int operandum:int?",
      "#   reinforcer_start  : id:int potency:float operandum:int?",
      "#   reinforcer_end    : id:int operandum:int?",
      "#   phase_enter       : label:str name:str? time:str? location:str? cue:str?",
      "#   phase_exit        : label:str",
      "# ---",
      "",
    ].join("\n")
  );
});

test("optional top-level keys and meta are written only when given, in fixture order", () => {
  const text = toOkl([], {
    session_name: "s",
    subject_id: "p01",
    replication_index: 2,
    experiment_name: "demo",
    protocol_id: "VI-v1",
    wall_clock_start: "2026-01-01T00:00:00.000Z",
    meta: { jspsych_version: "8.3.0", tick_interval_ms: 50, gain: 0.25, practice: false },
  });
  expect(text).toBe(
    [
      "# OKL v1",
      '# session_name = "s"',
      '# clock_type = "PerformanceNow"',
      "# session_start = 0.0",
      '# subject_id = "p01"',
      "# replication_index = 2",
      '# experiment_name = "demo"',
      '# protocol_id = "VI-v1"',
      '# wall_clock_start = "2026-01-01T00:00:00.000Z"',
      "# meta:",
      '#   jspsych_version = "8.3.0"',
      "#   tick_interval_ms = 50",
      "#   gain = 0.25",
      "#   practice = false",
      "# events:",
      "# ---",
      "",
    ].join("\n")
  );
});

test("body: one phase per trial, session timestamps on one clock, session-wide ids", () => {
  const second = trial({
    trial_index: 1,
    time_elapsed: 7000,
    trial_start_total_ms: 5000,
    trial_start_perf_ms: 5500,
    trial_end_s: 2.0,
    schedule: { type: "VI", mean: 30, n: 12, seed: 1 },
    schedule_label: "VI 30s",
    events: [
      { t: 0.25, type: "response", id: 0, operandum: 0 },
      { t: 0.25, type: "reinforcer_start", id: 0, operandum: 0, potency: 2.5 },
      { t: 0.75, type: "reinforcer_end", id: 0, operandum: 0 },
      { t: 1.0, type: "reinforcer_start", id: 1, operandum: null, potency: 1e-10 },
      { t: 1.0, type: "reinforcer_end", id: 1, operandum: null },
    ],
  });
  // Given out of order, with a trial of another plugin in between.
  const text = toOkl([second, { trial_type: "html-keyboard-response", trial_index: 9 }, trial()], {
    session_name: "s",
  });
  expect(text).toContain('#   trial_1_schedule = "{\\"type\\":\\"VI\\",\\"mean\\":30,\\"n\\":12,\\"seed\\":1}"');
  expect(bodyOf(text)).toBe(
    [
      "1.0\tphase_enter\ttrial 0 FR 2\t-\t-\t-\t-",
      "1.5\tresponse\t0\t0",
      "2.0\tresponse\t1\t0",
      "2.0\treinforcer_start\t0\t1.0\t0",
      "2.5\treinforcer_end\t0\t0",
      "4.0\tphase_exit\ttrial 0 FR 2",
      "5.0\tphase_enter\ttrial 1 VI 30s\t-\t-\t-\t-",
      "5.25\tresponse\t2\t0",
      "5.25\treinforcer_start\t1\t2.5\t0",
      "5.75\treinforcer_end\t1\t0",
      "6.0\treinforcer_start\t2\t1e-10\t-",
      "6.0\treinforcer_end\t2\t-",
      "7.0\tphase_exit\ttrial 1 VI 30s",
      "",
    ].join("\n")
  );
});

test("a reinforcer without an end is closed at the trial end", () => {
  const cut = trial({
    trial_end_s: 2.0,
    events: [
      { t: 0.5, type: "response", id: 0, operandum: 0 },
      { t: 0.5, type: "reinforcer_start", id: 0, operandum: 0, potency: 1 },
    ],
  });
  expect(bodyOf(toOkl([cut], { session_name: "s" }))).toBe(
    [
      "1.0\tphase_enter\ttrial 0 FR 2\t-\t-\t-\t-",
      "1.5\tresponse\t0\t0",
      "1.5\treinforcer_start\t0\t1.0\t0",
      "3.0\treinforcer_end\t0\t0",
      "3.0\tphase_exit\ttrial 0 FR 2",
      "",
    ].join("\n")
  );
});

test("a trial that starts before the previous one ended is an error, not hidden", () => {
  const overlapping = trial({ trial_index: 1, trial_start_perf_ms: 3000, trial_start_total_ms: 2500 });
  expect(() => toOkl([trial(), overlapping], { session_name: "s" })).toThrow(/earlier than the previous record/);
});

test("records without the plugin's data are skipped and counted", () => {
  // e.g. the data jsPsych stores when abortExperiment ends a contingency trial
  const aborted = { trial_type: "contingency", trial_index: 1, message: "stopped" };
  const text = toOkl([trial(), aborted], { session_name: "s" });
  expect(text).toContain("#   skipped_trials = 1\n");
  expect(bodyOf(text).split("\n").filter((l) => l.includes("phase_enter"))).toHaveLength(1);
});

test("a DataCollection-like object is filtered to contingency trials", () => {
  const records = [trial(), { trial_type: "survey", trial_index: 1 }];
  const collection = {
    filter: (query: { trial_type: string }) => ({
      values: () => records.filter((r) => r.trial_type === query.trial_type),
    }),
  };
  expect(toOkl(collection, { session_name: "s" })).toBe(toOkl([trial()], { session_name: "s" }));
});

test("strings are escaped: TOML in the header, TSV in the body", () => {
  const text = toOkl([trial({ schedule_label: "a\tb\\c" })], { session_name: 'say "hi"\n' });
  expect(text).toContain('# session_name = "say \\"hi\\"\\n"');
  expect(text).toContain("phase_enter\ttrial 0 a\\tb\\\\c\t-");
  expect(escapeTsv("-")).toBe("\\-");
  expect(escapeTsv("--")).toBe("--");
  expect(escapeTsv("x\ny\rz")).toBe("x\\ny\\rz");
});

test("values OKL v1 cannot represent are rejected instead of written wrongly", () => {
  const s = (session_name: string) => () => toOkl([], { session_name });
  expect(s("a\u0001b")).toThrow(/U\+0001/);
  expect(s("x\u2028y")).toThrow(/U\+2028/);
  expect(() => toOkl([trial({ schedule_label: "a\u0085b" })], { session_name: "s" })).toThrow(/U\+0085/);
  expect(() => toOkl([], { session_name: "s", meta: { "1st": 1 } })).toThrow(/meta key/);
  expect(() => toOkl([], { session_name: "s", meta: { big: 1e21 } })).toThrow(/too large/);
  expect(() => toOkl([], { session_name: "s", meta: { x: Infinity } })).toThrow(/finite/);
});

test("missing session_name and unknown event types are rejected", () => {
  expect(() => toOkl([], {} as { session_name: string })).toThrow(/session_name/);
  expect(() =>
    toOkl([trial({ events: [{ t: 0, type: "lick", id: 0, operandum: 0 }] })], { session_name: "s" })
  ).toThrow(/lick/);
});
