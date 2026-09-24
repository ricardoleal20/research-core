// Browser E2E layer (docs/e2e.md): the real UI over `vite dev` + the
// in-memory mock (mockActive when no Tauri runtime answers). The config
// boots the vite dev server on a fixed free port before the run; every
// worker context hits it fresh — the mock's state is per-page and resets
// on each load, so tests are deterministic without any seeding API.
import { defineConfig, devices } from "@playwright/test";

// A fixed free port for the suite (vite's config pins 1420 for the Tauri
// dev flow; the E2E run overrides it via the CLI flag below, which wins
// over vite.config.ts per Vite's own precedence rules).
const PORT = 5188;
const BASE_URL = `http://127.0.0.1:${PORT}`;

export default defineConfig({
  testDir: "./e2e",
  timeout: 30_000,
  expect: { timeout: 8_000 },
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: [
    ["html", { open: "never", outputFolder: "playwright-report" }],
    ["list"],
  ],
  use: {
    baseURL: BASE_URL,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    command: `npm run dev -- --port ${PORT} --strictPort`,
    url: BASE_URL,
    reuseExistingServer: false,
    timeout: 60_000,
  },
});