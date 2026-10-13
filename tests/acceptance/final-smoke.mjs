import { chromium } from "@playwright/test";
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
const browser = await chromium.connectOverCDP("http://127.0.0.1:9223");
const page = browser.contexts()[0].pages()[0];
await page.waitForSelector(".chat-composer");
const invoke = (name, args = {}) =>
  page.evaluate(
    ([name, args]) => window.__TAURI_INTERNALS__.invoke(name, args),
    [name, args],
  );
const prefs = await invoke("get_preferences");
assert.equal(prefs.model, "chat-quality");
assert.equal(prefs.catalogMode, "personal");
assert((await invoke("setup_status")).ready);
const home = "wrld_4432ea9b-729c-46e3-8eaf-846aa0a37fdd";
const conditions = {
  people: null,
  platform: null,
  required: [],
  preferred: [],
  excluded: [],
};
const start = Date.now();
const answer = await invoke("ai_answer", {
  requestId: "final-unknown",
  query: "雨音がある？Quest対応？軽い？",
  ids: [home],
  conditions,
  history: [],
  intent: "question",
});
assert(/未確認|不明|確認でき|情報がありません/.test(answer));
await page.getByRole("button", { name: "新しいチャット", exact: true }).click();
await page
  .getByRole("textbox", { name: "メッセージ", exact: true })
  .fill(`https://vrchat.com/home/world/${home}`);
await page
  .getByRole("button", { name: "メッセージを送信", exact: true })
  .click();
await page
  .getByRole("button", { name: "応答を停止", exact: true })
  .waitFor({ state: "hidden", timeout: 120000 });
assert.equal(await page.locator(".messages .world-card").count(), 1);
await page
  .getByRole("textbox", { name: "メッセージ", exact: true })
  .fill("登録したワールドを紹介して");
await page
  .getByRole("button", { name: "メッセージを送信", exact: true })
  .click();
await page
  .getByRole("button", { name: "応答を停止", exact: true })
  .waitFor({ state: "hidden", timeout: 120000 });
const intro = await page.locator(".reply-intro").last().innerText();
assert(/VRChat Home|Rainy Daze/.test(intro));
assert(
  (await page.locator(".chat-turn").last().locator(".world-card").count()) >= 2,
);
assert(
  !intro.includes("雨音を楽し") && !intro.includes("雨音や天気変化を楽し"),
);
await new Promise((r) => setTimeout(r, 600));
const chats = await invoke("load_chats");
assert(chats.some((c) => c.turns.at(-1)?.reply?.answer === intro));
await page.screenshot({ path: "artifacts/practical-desktop.png" });
await invoke("cancel_inference");
assert.equal((await invoke("runtime_status")).running, false);
writeFileSync(
  "artifacts/final-smoke.json",
  JSON.stringify(
    {
      date: "2026-10-09",
      unknownAnswer: answer,
      uiAnswer: intro,
      totalMs: Date.now() - start,
      registeredWorldCount: (await invoke("load_data")).catalog.worlds.length,
      urlImport: true,
      durableChats: true,
      processesStopped: true,
    },
    null,
    2,
  ),
);
console.log("Final native acceptance passed", answer, intro);
await browser.close();
