import { chromium } from "@playwright/test";
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";

const browser = await chromium.connectOverCDP("http://127.0.0.1:9223");
try {
  const page = browser.contexts()[0].pages()[0];
  const invoke = (name, args = {}) =>
    page.evaluate(
      ([name, args]) => window.__TAURI_INTERNALS__.invoke(name, args),
      [name, args],
    );
  const progress = await invoke("corpus_status");
  assert.equal(progress.stage, "complete");
  assert.equal(progress.completed, 1000);
  assert.equal(progress.indexed, 1000);
  const data = await invoke("load_data");
  assert.equal(data.catalog.worlds.length, 1000);
  assert.equal(data.catalog.fixture, false);
  await page
    .getByRole("button", { name: "新しいチャット", exact: true })
    .click();
  await page
    .getByRole("textbox", { name: "メッセージ", exact: true })
    .fill("ちるわのおすすめ");
  const started = Date.now();
  await page
    .getByRole("textbox", { name: "メッセージ", exact: true })
    .press("Enter");
  await page.locator(".world-card").first().waitFor({ timeout: 180000 });
  const elapsed = Date.now() - started;
  await page.getByRole("button", { name: "応答を停止", exact: true }).waitFor({state:"hidden",timeout:180000});
  await page.waitForFunction(
    () =>
      [...document.querySelectorAll(".world-thumbnail")].filter(
        (img) => img.complete && img.naturalWidth > 0,
      ).length >= 3,
    null,
    { timeout: 60000 },
  );
  const names = await page.locator(".world-card h3").allTextContents();
  const imageCount = await page
    .locator(".world-thumbnail")
    .evaluateAll(
      (es) => es.filter((img) => img.complete && img.naturalWidth > 0).length,
    );
  await page.screenshot({ path: "artifacts/corpus-live-search.png" });
  writeFileSync(
    "artifacts/corpus-live-smoke.json",
    JSON.stringify(
      {
        version: "0.3.3",
        query: "ちるわのおすすめ",
        worlds: 1000,
        indexed: 1000,
        elapsedMs: elapsed,
        cards: await page.locator(".world-card").count(),
        names,
        loadedImages: imageCount,
      },
      null,
      2,
    ) + "\n",
  );
  console.log("Live search verified", { elapsed, names, imageCount });
} finally {
  await browser.close();
}
