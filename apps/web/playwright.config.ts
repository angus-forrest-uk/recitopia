import { defineConfig, devices } from "@playwright/test";

const apiPort = 18077;
const webPort = 15173;

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: true,
  reporter: "list",
  use: {
    baseURL: `http://127.0.0.1:${webPort}`,
    trace: "on-first-retry",
  },
  webServer: [
    {
      command: `cd ../api-rs && RECITOPIA_DB_PATH=:memory: RECITOPIA_API_PORT=${apiPort} RECITOPIA_RUST_STORE_MODE=read-write cargo run --features bundled-duckdb`,
      url: `http://127.0.0.1:${apiPort}/api/health`,
      reuseExistingServer: false,
      timeout: 600_000,
    },
    {
      command: `RECITOPIA_API_URL=http://127.0.0.1:${apiPort} bun run dev -- --host 127.0.0.1 --port ${webPort}`,
      url: `http://127.0.0.1:${webPort}`,
      reuseExistingServer: false,
      timeout: 120_000,
    },
  ],
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
});
