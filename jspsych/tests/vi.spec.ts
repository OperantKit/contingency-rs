import { expect, Page, test } from "@playwright/test";

import { ContingencyEvent, fixtureUrl, routeOfflineJsPsych, trialData, useOfflineJsPsych } from "./helpers";

const { pageErrors } = useOfflineJsPsych(["VI: an invalid"]);

const RESPONSE_PERIOD_MS = 500;
const N_RESPONSES = 120; // 60 s of simulated time

/**
 * Run a VI trial on a fake clock: one click every 500 ms for 60 s, then the end key.
 * Returns the trial data and, for each response, whether it was reinforced.
 */
async function runVi(page: Page, schedule: object) {
  // Pause the fake clock before the page loads so the trial starts at a fixed time and
  // every response time is exact (the clock only moves in runFor).
  await page.clock.install({ time: 0 });
  await page.clock.pauseAt(1000);
  await page.goto(fixtureUrl({ schedule, reinforcer_duration: 0, end_key: "q" }));
  const button = page.locator("#jspsych-contingency-btngroup button");
  await expect(button).toBeVisible();
  for (let i = 0; i < N_RESPONSES; i++) {
    await button.click();
    await page.clock.runFor(RESPONSE_PERIOD_MS);
  }
  await page.keyboard.press("q");
  const [data] = await trialData(page);
  const events: ContingencyEvent[] = data.events;
  const reinforced: boolean[] = [];
  events.forEach((event, i) => {
    if (event.type === "response") reinforced.push(events[i + 1]?.type === "reinforcer_start");
  });
  return { data, events, reinforced };
}

test("VI: some but not all responses are reinforced, each at the moment of a response", async ({ page }) => {
  const { data, events, reinforced } = await runVi(page, { type: "VI", mean: 5, n: 12, seed: 7 });
  expect(data.schedule_label).toBe("VI 5s");
  expect(data.end_reason).toBe("end_key");
  expect(data.n_responses).toBe(N_RESPONSES);

  // 60 s at a mean of 5 s: some but far from all of the 120 responses are reinforced.
  expect(data.n_reinforcers).toBeGreaterThanOrEqual(5);
  expect(data.n_reinforcers).toBeLessThan(N_RESPONSES / 4);
  expect(reinforced.filter(Boolean).length).toBe(data.n_reinforcers);

  // Every reinforcer follows a response at the same moment, on that response's operandum.
  events.forEach((event, i) => {
    if (event.type !== "reinforcer_start") return;
    const response = events[i - 1];
    expect(response.type).toBe("response");
    expect(event.t).toBe(response.t);
    expect(event.operandum).toBe(0);
  });

  // The fake clock steps 500 ms between responses, so the times are exact.
  const responses = events.filter((e) => e.type === "response");
  responses.forEach((event, i) => expect(event.t).toBeCloseTo((i * RESPONSE_PERIOD_MS) / 1000, 6));
});

test("VI: the same seed reproduces the same reinforced responses; another seed does not", async ({ browser }) => {
  test.setTimeout(120_000);
  const run = async (seed: number) => {
    const context = await browser.newContext({ baseURL: test.info().project.use.baseURL });
    const page = await context.newPage();
    const errors: string[] = [];
    await routeOfflineJsPsych(page, errors);
    const { reinforced } = await runVi(page, { type: "VI", mean: 5, n: 12, seed });
    await context.close();
    expect(errors).toEqual([]);
    return reinforced;
  };
  const first = await run(7);
  const again = await run(7);
  const other = await run(8);
  expect(again).toEqual(first);
  expect(other).not.toEqual(first);
});

test("VI: n defaults to 12, and seed may be omitted", async ({ browser }) => {
  test.setTimeout(120_000);
  const run = async (schedule: object) => {
    const context = await browser.newContext({ baseURL: test.info().project.use.baseURL });
    const page = await context.newPage();
    const errors: string[] = [];
    await routeOfflineJsPsych(page, errors);
    const result = await runVi(page, schedule);
    await context.close();
    expect(errors).toEqual([]);
    return result;
  };
  const implicit = await run({ type: "VI", mean: 5, seed: 7 });
  const twelve = await run({ type: "VI", mean: 5, n: 12, seed: 7 });
  const four = await run({ type: "VI", mean: 5, n: 4, seed: 7 });
  expect(implicit.reinforced).toEqual(twelve.reinforced);
  expect(four.reinforced).not.toEqual(twelve.reinforced);
  const unseeded = await run({ type: "VI", mean: 5 });
  expect(unseeded.data.schedule).toEqual({ type: "VI", mean: 5 });
  expect(unseeded.data.n_reinforcers).toBeGreaterThan(0);
});

test("VI: an invalid mean is reported, not silently run", async ({ page }) => {
  await page.goto(fixtureUrl({ schedule: { type: "VI", mean: 0 } }));
  await expect.poll(() => pageErrors().join("\n")).toContain("VI mean must be a positive number");
});

test("examples/vi.html loads and starts a VI trial", async ({ page }) => {
  await page.goto("/examples/vi.html");
  await expect(page.locator("#jspsych-contingency-btngroup button")).toBeVisible();
  await expect(page.locator("#jspsych-contingency-stimulus")).toContainText("10 s");
});
