import { execFileSync, spawnSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";

import { expect, test } from "@playwright/test";

import { timelineUrl, trialData, useOfflineJsPsych } from "./helpers";

useOfflineJsPsych();

// The OKL reference reader is the Python package `session-recorder`. Point
// SESSION_RECORDER_PYTHON at an interpreter that can import it (default: python3).
// Without it the round-trip test fails rather than skipping, so a green run always
// means the file was read back.
const PYTHON = process.env.SESSION_RECORDER_PYTHON ?? "python3";
const hasReader = spawnSync(PYTHON, ["-c", "import session_recorder"]).status === 0;

const READ_LOG = `
import json, sys, warnings
from collections import Counter
from session_recorder.reader import iter_records, read_log
warnings.simplefilter("error")
log = read_log(sys.argv[1], on_unknown="error")
lines = list(iter_records(sys.argv[1], on_unknown="error"))
print(json.dumps({
    "session_name": log.meta.session_name,
    "clock_type": log.meta.clock_type,
    "subject_id": log.meta.subject_id,
    "n_events": len(log.events),
    "types": Counter(r.type for r in log.raw_records),
    "phases": [[m.kind, m.label] for m in log.phase_markers],
    "unknown_types": sorted(log.unknown_types),
    "timestamps": [r.timestamp for r in lines],
    "reinforcers": [[r.type, r.timestamp, r.args["id"]] for r in lines if r.type.startswith("reinforcer_")],
}))
`;

test("a VI + FR session exported with toOkl is read back by session-recorder", async ({ page }, testInfo) => {
  expect(hasReader, `set SESSION_RECORDER_PYTHON to a Python that imports session_recorder (tried ${PYTHON})`).toBe(true);

  await page.clock.install({ time: 0 });
  await page.goto(
    timelineUrl([
      { schedule: { type: "VI", mean: 2, n: 12, seed: 3 }, reinforcer_duration: 200, trial_duration: 20000 },
      { schedule: { type: "FR", n: 3 }, reinforcer_duration: 0, max_reinforcers: 2 },
    ])
  );
  const button = page.locator("#jspsych-contingency-btngroup button");
  await expect(button).toBeVisible();
  for (let i = 0; i < 30; i++) {
    await button.click();
    await page.clock.runFor(500);
  }
  await page.clock.runFor(10000);
  await expect.poll(() => page.evaluate(() => jsPsych.data.get().count())).toBe(1);
  for (let i = 0; i < 6; i++) {
    await button.click();
  }
  const trials = await trialData(page, 2);

  const text: string = await page.evaluate(() =>
    jsPsychContingency.toOkl(jsPsych.data.get(), {
      session_name: "e2e",
      subject_id: "p01",
      wall_clock_start: jsPsych.getStartTime().toISOString(),
      meta: { jspsych_version: jsPsych.version() },
    })
  );
  expect(text).toContain("#   plugin_version = ");
  const file = testInfo.outputPath("session.txt");
  writeFileSync(file, text);

  const result = JSON.parse(execFileSync(PYTHON, ["-c", READ_LOG, file], { encoding: "utf8" }));
  expect(result.session_name).toBe("e2e");
  expect(result.clock_type).toBe("PerformanceNow");
  expect(result.subject_id).toBe("p01");
  expect(result.unknown_types).toEqual([]);

  const count = (type: string) =>
    trials.reduce((n: number, t: any) => n + t.events.filter((e: any) => e.type === type).length, 0);
  expect(result.types.response).toBe(trials[0].n_responses + trials[1].n_responses);
  expect(result.types.reinforcer_start).toBe(trials[0].n_reinforcers + trials[1].n_reinforcers);
  expect(result.types.reinforcer_end).toBe(count("reinforcer_end"));
  expect(result.types.reinforcer_start).toBeGreaterThan(2);
  expect(result.n_events).toBe(count("response") + count("reinforcer_start") + count("reinforcer_end"));

  expect(result.phases).toEqual([
    ["enter", "trial 0 VI 2s"],
    ["exit", "trial 0 VI 2s"],
    ["enter", "trial 1 FR 3"],
    ["exit", "trial 1 FR 3"],
  ]);
  // Every body line, phase markers included, is in time order.
  const stamps: number[] = result.timestamps;
  expect(stamps.length).toBe(result.n_events + 4);
  for (let i = 1; i < stamps.length; i++) expect(stamps[i]).toBeGreaterThanOrEqual(stamps[i - 1]);

  // Each reinforcer_start is followed by exactly one reinforcer_end with the same id.
  const started = new Map<number, number>();
  const ended = new Set<number>();
  for (const [type, ts, id] of result.reinforcers as [string, number, number][]) {
    if (type === "reinforcer_start") {
      expect(started.has(id)).toBe(false);
      started.set(id, ts);
    } else {
      expect(started.has(id) && !ended.has(id)).toBe(true);
      expect(ts).toBeGreaterThanOrEqual(started.get(id)!);
      ended.add(id);
    }
  }
  expect([...started.keys()]).toEqual([...Array(started.size).keys()]);
  expect(ended.size).toBe(started.size);
});

test("a reinforcer cut off by the end of the trial is closed at the trial end", async ({ page }) => {
  await page.clock.install({ time: 0 });
  await page.clock.pauseAt(1000);
  await page.goto(
    timelineUrl([{ schedule: { type: "CRF" }, reinforcer_duration: 60000, trial_duration: 800 }])
  );
  await page.locator("#jspsych-contingency-btngroup button").click();
  await page.clock.runFor(1000);
  const [data] = await trialData(page);
  expect(data.events.map((e: any) => e.type)).toEqual(["response", "reinforcer_start", "reinforcer_end"]);
  expect(data.events[2].t).toBeCloseTo(0.8, 6);
  expect(data.trial_end_s).toBeCloseTo(0.8, 6);
  const text: string = await page.evaluate(() => jsPsychContingency.toOkl(jsPsych.data.get(), { session_name: "cut" }));
  expect(text).toMatch(/\treinforcer_end\t0\t0\n[^\n]*\tphase_exit\t/);
});

test("downloadOkl offers the text as a file", async ({ page }) => {
  await page.goto(timelineUrl([{ schedule: { type: "EXT" }, trial_duration: 100 }]));
  await trialData(page);
  const [download] = await Promise.all([
    page.waitForEvent("download"),
    page.evaluate(() =>
      jsPsychContingency.downloadOkl(jsPsychContingency.toOkl(jsPsych.data.get(), { session_name: "dl" }), "dl.txt")
    ),
  ]);
  expect(download.suggestedFilename()).toBe("dl.txt");
  const body = readFileSync(await download.path(), "utf8");
  expect(body.startsWith('# OKL v1\n# session_name = "dl"\n')).toBe(true);
});

declare const jsPsych: any;
declare const jsPsychContingency: any;
