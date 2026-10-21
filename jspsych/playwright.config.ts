import { defineConfig, devices } from "@playwright/test";

const PORT = 8787;

export default defineConfig({
  testDir: "tests",
  fullyParallel: false,
  reporter: "list",
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    ...devices["Desktop Chrome"],
  },
  // The .wasm cannot be fetched from file:// URLs, so the pages are served over http.
  webServer: {
    command: `python3 -m http.server ${PORT} --bind 127.0.0.1`,
    url: `http://127.0.0.1:${PORT}/examples/fr5.html`,
    reuseExistingServer: false,
  },
});
