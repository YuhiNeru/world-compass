import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "../../tests/acceptance",
  testMatch: "*.spec.ts",
  fullyParallel: false,
  workers: 1,
  use: {
    baseURL: "http://127.0.0.1:1420",
    viewport: { width: 1280, height: 960 },
    launchOptions: {
      executablePath:
        "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
    },
  },
  webServer: {
    command: "npm run dev",
    url: "http://127.0.0.1:1420",
    reuseExistingServer: true,
    timeout: 30000,
  },
  reporter: "list",
  outputDir: "../../artifacts/playwright",
});
