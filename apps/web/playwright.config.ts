import { defineConfig, devices } from "@playwright/test";
import { deployedWebURL, repository } from "./e2e/deployment";

const baseURL = await deployedWebURL();

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: false,
  workers: 1,
  forbidOnly: Boolean(process.env["CI"]),
  retries: 0,
  reporter: [["list"], ["html", { open: "never" }]],
  use: {
    ...devices["Desktop Chrome"],
    baseURL,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  webServer: {
    command: "just deploy up --detached=false",
    cwd: repository,
    url: new URL("/healthz", baseURL).href,
    // Native weights and container builds can take minutes on a clean checkout.
    timeout: 15 * 60_000,
    reuseExistingServer: !process.env["CI"],
    gracefulShutdown: { signal: "SIGTERM", timeout: 30_000 },
    stdout: "pipe",
    stderr: "pipe",
  },
});
