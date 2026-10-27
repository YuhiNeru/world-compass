import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "../../tests/acceptance",
  testMatch: "*.spec.ts",
  fullyParallel: false,
  workers: 1,
  use: {
    baseURL: "http://127.0.0.1:1437",
    viewport: { width: 1280, height: 960 },
    launchOptions: {
      executablePath:
        "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
    },
  },
  webServer: {
    command: "npm run build && npm exec -- vite preview --host 127.0.0.1 --port 1437 --strictPort",
    url: "http://127.0.0.1:1437",
    reuseExistingServer: false,
    timeout: 90000,
  },
  reporter: "list",
  outputDir: "../../artifacts/playwright",
});
