// Launch the built app with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223.
// This script tests our own WebView and IPC; no VRChat/browser credentials are accessed.
import { chromium } from "@playwright/test";
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
const browser = await chromium.connectOverCDP("http://127.0.0.1:9223");
const context = browser.contexts()[0];
let page = context.pages()[0];
for (let i = 0; i < 50 && !page; i++) {
  await new Promise((r) => setTimeout(r, 100));
  page = context.pages()[0];
}
assert(page, "App WebView should exist");
await page.waitForSelector(".chat-composer", { timeout: 30000 });
const invoke = (name, args = {}) =>
  page.evaluate(
    ([name, args]) => window.__TAURI_INTERNALS__.invoke(name, args),
    [name, args],
  );
const data = await invoke("load_data");
assert.equal(data.catalog.worlds.length, 10);
assert.equal(data.catalog.fixture, true);
const status = await invoke("setup_status");
assert(
  status.ready,
  "Pinned assets should be installed in test data directory",
);
const id = "fixture-rain-room",
  value = {
    favorite: true,
    visited: true,
    excluded: false,
    note: "Desktop SQLite acceptance note",
  };
await invoke("save_personal", { id, value });
assert.deepEqual((await invoke("load_data")).personal[id], value);
const query = "雨の夜に2人でゆっくりできる場所";
const conditionStart = Date.now();
const raw = await invoke("ai_conditions", { query });
assert.equal(raw.people, 2);
assert.equal(raw.platform, null);
const conditions = {
  people: 2,
  platform: null,
  required: [],
  preferred: ["rain", "night", "chill"],
  excluded: [],
};
const scoresStart = Date.now();
const scores = await invoke("semantic_scores", { query });
assert.equal(Object.keys(scores).length, 10);
assert(scores["fixture-rain-room"] > scores["fixture-puzzle-island"]);
const reasonsStart = Date.now();
const reasons = await invoke("ai_reasons", {
  ids: [id, "fixture-moon-terrace", "fixture-rain-station"],
  conditions,
});
const reasonsEnded = Date.now();
for (const item of reasons.items) {
  assert(
    [id, "fixture-moon-terrace", "fixture-rain-station"].includes(item.worldId),
  );
  assert(item.evidence.length > 0);
}
await invoke("cancel_inference");
assert.equal((await invoke("runtime_status")).running, false);
const uiStart = Date.now();
await page.getByRole("button", { name: "設定", exact: true }).click();
await page.getByRole("checkbox", { name: "ローカルAIを使う" }).check();
await page.getByRole("checkbox", { name: "AIで提案根拠を選ぶ" }).uncheck();
await page.getByRole("button", { name: "新しいチャット", exact: true }).click();
await page
  .getByRole("textbox", { name: "メッセージ" })
  .fill("Questで雨音必須、夜に2人でゆっくりできる場所");
await page
  .getByRole("button", { name: "メッセージを送信", exact: true })
  .click();
await page
  .getByText("ローカルAI + 通常検索 · 架空カタログによる提案", { exact: true })
  .waitFor({ timeout: 120000 });
assert.equal(
  await page.locator(".world-grid").first().locator("article").count(),
  1,
);
await invoke("cancel_inference");
await invoke("save_personal", {
  id,
  value: { favorite: false, visited: false, excluded: false, note: "" },
});
await page.screenshot({ path: "artifacts/desktop-preview.png" });
const report = {
  date: "2026-10-08",
  profile: "release",
  query,
  rawConditions: raw,
  conditionsMs: scoresStart - conditionStart,
  semanticMs: reasonsStart - scoresStart,
  reasonsMs: reasonsEnded - reasonsStart,
  uiSearchMs: Date.now() - uiStart,
  scores,
  reasons,
  dataRoot: status.root,
  ramBytes: status.ram,
  cleanWindows: false,
  vrchatConcurrent: false,
};
writeFileSync("artifacts/desktop-smoke.json", JSON.stringify(report, null, 2));
console.log(JSON.stringify(report, null, 2));
await browser.close();
