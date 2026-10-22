import { expect, Page, test } from "@playwright/test";

import { fixtureUrl, timelineUrl, trialData, useOfflineJsPsych } from "./helpers";

useOfflineJsPsych();

declare const jsPsych: any;
declare const jsPsychContingency: any;

const activeSchedules = (page: Page) => page.evaluate(() => jsPsychContingency.activeSchedules);

test("a trial ended by jsPsych.abortExperiment frees its schedule", async ({ page }) => {
  await page.goto(fixtureUrl({ schedule: { type: "FR", n: 5 } }));
  const button = page.locator("#jspsych-contingency-btngroup button");
  await expect(button).toBeVisible();
  await button.click();
  expect(await activeSchedules(page)).toBe(1);

  await page.evaluate(() => jsPsych.abortExperiment("aborted"));
  await expect.poll(() => activeSchedules(page)).toBe(0);
  await expect(page.locator("body")).toContainText("aborted");
});

test("abortExperiment during a reinforcer presentation frees the schedule once", async ({ page }) => {
  await page.goto(fixtureUrl({ schedule: { type: "CRF" }, reinforcer_duration: 10_000 }));
  const button = page.locator("#jspsych-contingency-btngroup button");
  await button.click();
  await expect(page.locator("#jspsych-contingency-reinforcer")).toBeVisible();
  await page.evaluate(() => jsPsych.abortExperiment("aborted"));
  await expect.poll(() => activeSchedules(page)).toBe(0);
  // A late timer after the abort must not touch the freed schedule (a use after free
  // throws "null pointer passed to rust", which the afterEach check would report).
  await page.waitForTimeout(200);
  expect(await activeSchedules(page)).toBe(0);
});

test("trials that end on their own free their schedule once, across a timeline", async ({ page }) => {
  await page.goto(
    timelineUrl([
      { schedule: { type: "CRF" }, reinforcer_duration: 0, max_reinforcers: 1 },
      { schedule: { type: "CRF" }, reinforcer_duration: 0, max_reinforcers: 1 },
    ])
  );
  const button = page.locator("#jspsych-contingency-btngroup button");
  await button.click();
  await trialData(page, 1);
  await button.click();
  await trialData(page, 2);
  await page.waitForFunction(() => (window as any).experimentFinished === true);
  expect(await activeSchedules(page)).toBe(0);
});

test("the second trial of a timeline is freed when the experiment is aborted during it", async ({ page }) => {
  await page.goto(
    timelineUrl([
      { schedule: { type: "CRF" }, reinforcer_duration: 0, max_reinforcers: 1 },
      { schedule: { type: "FR", n: 3 } },
    ])
  );
  const button = page.locator("#jspsych-contingency-btngroup button");
  await button.click();
  await trialData(page, 1);
  await expect(button).toBeVisible();
  await expect.poll(() => activeSchedules(page)).toBe(1);
  await page.evaluate(() => jsPsych.abortExperiment("aborted"));
  await expect.poll(() => activeSchedules(page)).toBe(0);
});

test("schedule_label writes seconds without binary rounding noise", async ({ page }) => {
  // 0.1 + 0.2 is 0.30000000000000004 in IEEE 754.
  await page.goto(fixtureUrl({ schedule: { type: "VI", mean: 0.1 + 0.2, seed: 1 }, end_key: "q" }));
  await expect(page.locator("#jspsych-contingency-btngroup button")).toBeVisible();
  await page.keyboard.press("q");
  const [data] = await trialData(page);
  expect(data.schedule_label).toBe("VI 0.3s");
});

test("schedule_label keeps up to three decimals of a VI mean", async ({ page }) => {
  await page.goto(
    timelineUrl([
      { schedule: { type: "VI", mean: 2.5, seed: 1 }, end_key: "q" },
      { schedule: { type: "VI", mean: 1.23456, seed: 1 }, end_key: "q" },
      { schedule: { type: "VI", mean: 30, seed: 1 }, end_key: "q" },
      // Below a millisecond the label keeps significant digits instead of reading "VI 0s".
      { schedule: { type: "VI", mean: 0.0001234, seed: 1 }, end_key: "q", tick_interval: 1000 },
    ])
  );
  for (let i = 1; i <= 4; i++) {
    await expect(page.locator("#jspsych-contingency-btngroup button")).toBeVisible();
    await page.keyboard.press("q");
    await trialData(page, i);
  }
  const data = await trialData(page, 4);
  expect(data.map((d: { schedule_label: string }) => d.schedule_label)).toEqual([
    "VI 2.5s",
    "VI 1.235s",
    "VI 30s",
    "VI 0.000123s",
  ]);
});
