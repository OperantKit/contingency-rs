import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { expect, Page, test } from "@playwright/test";

const jspsychDir = fileURLToPath(new URL("../node_modules/jspsych/", import.meta.url));

// Serve the CDN copy of jsPsych from node_modules so the tests run offline and
// against the version pinned in package.json.
let pageErrors: string[] = [];

test.beforeEach(async ({ page }) => {
  pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  await page.route(/^https:\/\/unpkg\.com\/jspsych@8\.3\.0(\/.*)?$/, (route) => {
    const url = new URL(route.request().url());
    const isCss = url.pathname.endsWith(".css");
    const file = isCss ? "css/jspsych.css" : "dist/index.browser.js";
    route.fulfill({
      body: readFileSync(path.join(jspsychDir, file)),
      contentType: isCss ? "text/css" : "application/javascript",
    });
  });
});

// An exception inside a click handler leaves the trial running, so fail on any page error.
test.afterEach(async ({}, testInfo) => {
  if (testInfo.title.startsWith("an unsupported schedule type")) return;
  expect(pageErrors).toEqual([]);
});

type ContingencyEvent = { t: number; type: string; id: number; operandum: number | null; potency?: number };

async function trialData(page: Page) {
  await page.waitForFunction(() => jsPsych.data.get().count() > 0);
  return page.evaluate(() => jsPsych.data.get().values());
}

test("FR 5: the fifth click is reinforced and recorded in the jsPsych data", async ({ page }) => {
  await page.goto("/examples/fr5.html");
  const button = page.locator("#jspsych-contingency-btngroup button");
  for (let i = 0; i < 4; i++) {
    await button.click();
  }
  await expect(page.locator("#jspsych-contingency-reinforcer")).toBeHidden();
  await button.click();
  await expect(page.locator("#jspsych-contingency-reinforcer")).toBeVisible();

  const [data] = await trialData(page);
  expect(data.trial_type).toBe("contingency");
  expect(data.schedule).toEqual({ type: "FR", n: 5 });
  expect(data.schedule_label).toBe("FR 5");
  expect(data.n_responses).toBe(5);
  expect(data.n_reinforcers).toBe(1);
  expect(data.end_reason).toBe("max_reinforcers");

  const events: ContingencyEvent[] = data.events;
  expect(events.map((e) => e.type)).toEqual([
    "response",
    "response",
    "response",
    "response",
    "response",
    "reinforcer_start",
    "reinforcer_end",
  ]);
  expect(events.filter((e) => e.type === "response").map((e) => e.id)).toEqual([0, 1, 2, 3, 4]);
  const fifth = events[4];
  const start = events[5];
  const end = events[6];
  expect(start).toMatchObject({ id: 0, operandum: 0, potency: 1 });
  expect(start.t).toBeGreaterThanOrEqual(fifth.t);
  expect(start.t - fifth.t).toBeLessThan(0.05);
  expect(end.id).toBe(0);
  // reinforcer_duration is 500 ms.
  expect(end.t - start.t).toBeGreaterThanOrEqual(0.45);
  for (let i = 1; i < events.length; i++) {
    expect(events[i].t).toBeGreaterThanOrEqual(events[i - 1].t);
  }
});

function fixtureUrl(trial: object) {
  return `/tests/fixture.html?trial=${encodeURIComponent(JSON.stringify(trial))}`;
}

test("CRF: every click is reinforced", async ({ page }) => {
  await page.goto(fixtureUrl({ schedule: { type: "CRF" }, reinforcer_duration: 0, max_reinforcers: 3 }));
  const button = page.locator("#jspsych-contingency-btngroup button");
  for (let i = 0; i < 3; i++) {
    await button.click();
  }
  const [data] = await trialData(page);
  expect(data.n_responses).toBe(3);
  expect(data.n_reinforcers).toBe(3);
  expect(data.end_reason).toBe("max_reinforcers");
  expect(data.events.map((e: ContingencyEvent) => e.type)).toEqual([
    "response",
    "reinforcer_start",
    "reinforcer_end",
    "response",
    "reinforcer_start",
    "reinforcer_end",
    "response",
    "reinforcer_start",
    "reinforcer_end",
  ]);
});

test("EXT: clicks are recorded but never reinforced; end_key ends the trial", async ({ page }) => {
  await page.goto(fixtureUrl({ schedule: { type: "EXT" }, end_key: "q" }));
  const button = page.locator("#jspsych-contingency-btngroup button");
  for (let i = 0; i < 10; i++) {
    await button.click();
  }
  await page.keyboard.press("q");
  const [data] = await trialData(page);
  expect(data.n_responses).toBe(10);
  expect(data.n_reinforcers).toBe(0);
  expect(data.end_reason).toBe("end_key");
});

test("trial_duration ends the trial and responses during the reinforcer are ignored by default", async ({ page }) => {
  await page.goto(
    fixtureUrl({ schedule: { type: "CRF" }, reinforcer_duration: 60000, trial_duration: 1500 })
  );
  const button = page.locator("#jspsych-contingency-btngroup button");
  await button.click();
  await expect(page.locator("#jspsych-contingency-reinforcer")).toBeVisible();
  await button.click();
  await button.click();
  const [data] = await trialData(page);
  expect(data.end_reason).toBe("duration");
  expect(data.n_responses).toBe(1);
  expect(data.n_reinforcers).toBe(1);
});

test("two operanda: the button index is recorded as the operandum", async ({ page }) => {
  await page.goto(
    fixtureUrl({ schedule: { type: "FR", n: 2 }, operanda: ["left", "right"], reinforcer_duration: 0, max_reinforcers: 1 })
  );
  await page.locator('#jspsych-contingency-btngroup button[data-operandum="1"]').click();
  await page.locator('#jspsych-contingency-btngroup button[data-operandum="0"]').click();
  const [data] = await trialData(page);
  const responses = data.events.filter((e: ContingencyEvent) => e.type === "response");
  expect(responses.map((e: ContingencyEvent) => e.operandum)).toEqual([1, 0]);
  expect(data.n_reinforcers).toBe(1);
  expect(data.operanda).toEqual(["left", "right"]);
});

test("an unsupported schedule type is reported, not silently run", async ({ page }) => {
  await page.goto(fixtureUrl({ schedule: { type: "XYZ" } }));
  await expect.poll(() => pageErrors.join("\n")).toContain("unsupported schedule type");
});

declare const jsPsych: any;
