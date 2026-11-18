import { defineConfig, devices } from "@playwright/test";
import { fileURLToPath } from "node:url";
import path from "node:path";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "../..");
const claimAppUrl =
  process.env.PLAYWRIGHT_CLAIM_APP_URL ?? "http://127.0.0.1:4421";
const migrateAppUrl =
  process.env.PLAYWRIGHT_MIGRATE_APP_URL ?? "http://127.0.0.1:4422";

process.env.PLAYWRIGHT_CLAIM_APP_URL = claimAppUrl;
process.env.PLAYWRIGHT_MIGRATE_APP_URL = migrateAppUrl;

export default defineConfig({
  testDir: "tests/e2e",
  testMatch: ["seeded-compose.spec.ts"],
  fullyParallel: false,
  workers: 1,
  retries: process.env.CI ? 1 : 0,
  use: {
    baseURL: claimAppUrl,
    trace: "on-first-retry",
  },
  webServer: {
    command: "bash apps/frontend/scripts/e2e_webserver_compose.sh",
    cwd: repoRoot,
    url: claimAppUrl,
    reuseExistingServer: false,
    timeout: 1_800_000,
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
});
