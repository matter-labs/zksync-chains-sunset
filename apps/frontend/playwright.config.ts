import { defineConfig, devices } from "@playwright/test";
import { fileURLToPath } from "node:url";
import path from "node:path";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "../..");
const claimAppUrl =
  process.env.PLAYWRIGHT_CLAIM_APP_URL ?? "http://127.0.0.1:4521";
const migrateAppUrl =
  process.env.PLAYWRIGHT_MIGRATE_APP_URL ?? "http://127.0.0.1:4522";

process.env.PLAYWRIGHT_CLAIM_APP_URL = claimAppUrl;
process.env.PLAYWRIGHT_MIGRATE_APP_URL = migrateAppUrl;

export default defineConfig({
  testDir: "tests/e2e",
  testIgnore: ["**/*compose.spec.ts"],
  fullyParallel: false,
  workers: 1,
  retries: process.env.CI ? 2 : 0,
  use: {
    baseURL: claimAppUrl,
    trace: "on-first-retry",
  },
  // Lightweight frontend-only browser coverage. Seeded browser/runtime
  // validation lives in playwright.compose.config.ts.
  webServer: {
    command: "bash apps/frontend/scripts/e2e_webserver_preview.sh",
    cwd: repoRoot,
    url: claimAppUrl,
    reuseExistingServer: false,
    timeout: 600_000,
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
});
