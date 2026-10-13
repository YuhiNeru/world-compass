import { chromium } from "@playwright/test";
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
const browser = await chromium.connectOverCDP("http://127.0.0.1:9223");
const context = browser.contexts()[0],
  page = context.pages()[0];
await page.waitForSelector(".chat-composer", { timeout: 30000 });
const invoke = (name, args = {}) =>
  page.evaluate(
    ([name, args]) => window.__TAURI_INTERNALS__.invoke(name, args),
    [name, args],
  );
const data = await invoke("load_data");
assert.equal(data.catalog.fixture, false);
const report = { version: "0.2.1", date: "2026-10-09", worlds: [] };
// Reuse an existing conversation without deleting or renaming any history.
const history = page.locator(".chat-history button");
assert((await history.count()) > 0);
await history.first().click();
await page.locator(".world-card").first().waitFor();
await page.waitForFunction(
  () => {
    const images = [...document.querySelectorAll(".world-thumbnail")];
    return (
      images.length >= 2 &&
      images.every((img) => img.complete && img.naturalWidth > 0)
    );
  },
  {},
  { timeout: 60000 },
);
await page.waitForFunction(
  () =>
    [...document.querySelectorAll(".translation-label")].filter((el) =>
      el.textContent.includes("日本語訳 · ローカルAI"),
    ).length >= 2,
  {},
  { timeout: 180000 },
);
const cards = page.locator(".chat-turn").last().locator(".world-card");
for (const world of data.catalog.worlds) {
  const original = world.description;
  const translated = await invoke("translate_description", {
    id: world.worldId,
    original,
  });
  assert.equal(translated.cached, true);
  assert(/[\u3040-\u30ff\u4e00-\u9fff]/.test(translated.text));
  const card = cards.filter({
    has: page.getByRole("heading", { name: world.name, exact: true }),
  });
  assert.equal(
    await card.locator(".translation-label").innerText(),
    "日本語訳 · ローカルAI",
  );
  const originalView = card.locator(".original-description");
  await originalView.locator("summary").click();
  assert.equal(await originalView.locator("p").innerText(), original);
  await originalView.locator("summary").click();
  report.worlds.push({
    id: world.worldId,
    name: world.name,
    translation: translated.text,
    cached: true,
  });
}
await page.screenshot({ path: "artifacts/images-and-translation.png" });
// Cached images still decode with the WebView offline; translation reads from local SQLite.
await context.setOffline(true);
for (const world of data.catalog.worlds) {
  const length = await page.evaluate(async (id) => {
    const bytes = await window.__TAURI_INTERNALS__.invoke("world_image", {
      id,
    });
    return bytes.byteLength ?? bytes.length;
  }, world.worldId);
  assert(length > 100);
}
await context.setOffline(false);
await page.reload();
await page.waitForSelector(".chat-composer");
await page.locator(".chat-history button").first().click();
await page.waitForFunction(
  () =>
    [...document.querySelectorAll(".translation-label")].filter((el) =>
      el.textContent.includes("日本語訳 · ローカルAI"),
    ).length >= 2,
  {},
  { timeout: 30000 },
);
assert(
  (await invoke("load_data")).catalog.worlds.every(
    (w) =>
      w.description ===
      data.catalog.worlds.find((original) => original.worldId === w.worldId)
        ?.description,
  ),
);
await invoke("cancel_inference");
report.originalPreserved = true;
report.offlineImages = true;
report.reloadCached = true;
writeFileSync(
  "artifacts/presentation-smoke.json",
  JSON.stringify(report, null, 2),
);
console.log("Images and translation acceptance passed", report);
await browser.close();
