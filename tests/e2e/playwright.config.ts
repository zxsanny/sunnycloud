import { defineConfig } from "@playwright/test";
import { readAcceptanceEnv } from "./support/env.ts";

const origin = readAcceptanceEnv().ACC_ORIGIN;

// http://localhost is a secure context for Chromium, which is what lets the Secure __Host- cookies work without TLS.
export default defineConfig({
  testDir: "./tests",
  timeout: 300_000,
  expect: { timeout: 15_000 },
  workers: 1,
  fullyParallel: false,
  retries: 0,
  reporter: [["list"]],
  outputDir: process.env.E2E_OUTPUT ?? "test-results",
  use: {
    baseURL: origin,
    browserName: "chromium",
    headless: true,
    acceptDownloads: true,
    actionTimeout: 30_000,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
});
