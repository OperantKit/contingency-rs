import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { expect, Page, test } from "@playwright/test";

const jspsychDir = fileURLToPath(new URL("../node_modules/jspsych/", import.meta.url));

export type ContingencyEvent = { t: number; type: string; id: number; operandum: number | null; potency?: number };

/**
 * Serve the CDN copy of jsPsych from node_modules so the tests run offline and against
 * the version pinned in package.json, and fail on any page error (an exception inside a
 * click handler leaves the trial running). Tests whose title starts with one of
 * `allowErrorsIn` expect page errors and check them themselves.
 */
export function useOfflineJsPsych(allowErrorsIn: string[] = []): { pageErrors: () => string[] } {
  let pageErrors: string[] = [];

  test.beforeEach(async ({ page }) => {
    pageErrors = [];
    await routeOfflineJsPsych(page, pageErrors);
  });

  test.afterEach(async ({}, testInfo) => {
    if (allowErrorsIn.some((prefix) => testInfo.title.startsWith(prefix))) return;
    expect(pageErrors).toEqual([]);
  });

  return { pageErrors: () => pageErrors };
}

/** Route the jsPsych CDN URLs to node_modules and collect page errors into `errors`. */
export async function routeOfflineJsPsych(page: Page, errors: string[]): Promise<void> {
  page.on("pageerror", (error) => errors.push(error.message));
  await page.route(/^https:\/\/unpkg\.com\/jspsych@8\.3\.0(\/.*)?$/, (route) => {
    const url = new URL(route.request().url());
    const isCss = url.pathname.endsWith(".css");
    const file = isCss ? "css/jspsych.css" : "dist/index.browser.js";
    route.fulfill({
      body: readFileSync(path.join(jspsychDir, file)),
      contentType: isCss ? "text/css" : "application/javascript",
    });
  });
}

/** Fixture page running one trial (`?trial=`) or a timeline of trials (`?timeline=`). */
export function fixtureUrl(trial: object): string {
  return `/tests/fixture.html?trial=${encodeURIComponent(JSON.stringify(trial))}`;
}

export function timelineUrl(trials: object[]): string {
  return `/tests/fixture.html?timeline=${encodeURIComponent(JSON.stringify(trials))}`;
}

export async function trialData(page: Page, count = 1) {
  await page.waitForFunction((n) => jsPsych.data.get().count() >= n, count);
  return page.evaluate(() => jsPsych.data.get().values());
}

declare const jsPsych: any;
