import { chromium } from "@playwright/test";
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
const browser = await chromium.connectOverCDP("http://127.0.0.1:9223");
const page = browser.contexts()[0].pages()[0];
await page.waitForSelector(".chat-composer", { timeout: 30000 });
const invoke = (command, args = {}) =>
  page.evaluate(
    ([command, args]) => window.__TAURI_INTERNALS__.invoke(command, args),
    [command, args],
  );
const prefs = await invoke("get_preferences");
const record = {
  date: "2026-10-08",
  model: prefs.model,
  backend: prefs.backend,
  plans: [],
};
if (!(await invoke("setup_status")).ready) {
  console.log("Installing staged, pinned assets...");
  await invoke("install_assets");
}
assert((await invoke("setup_status")).ready);
const originalMode = prefs.catalogMode;
await invoke("save_preferences", { value: { ...prefs, catalogMode: "demo" } });
const inputs = [
  "雨の夜に2人でゆっくりできる場所",
  "Quest対応が必須、ゲームなしで4人で写真を撮りたい",
  "静かな森で一人でぼーっとしたい",
  "人数は6人、雨音は絶対必要。軽さは未確認でいい",
  "こんにちは",
];
for (const query of inputs) {
  const t = Date.now();
  const plan = await invoke("ai_plan", { query, previous: null });
  assert(["chat", "search", "question"].includes(plan.intent));
  record.plans.push({ query, plan, milliseconds: Date.now() - t });
  console.log(query, plan, Date.now() - t);
}
assert.equal(record.plans[0].plan.conditions.people, 2);
const conditions = {
  people: 2,
  platform: null,
  required: [],
  preferred: ["rain", "night", "chill"],
  excluded: [],
};
const start = Date.now();
const scores = await invoke("semantic_scores", { query: inputs[0] });
assert(scores["fixture-rain-room"] > scores["fixture-puzzle-island"]);
record.embeddingMs = Date.now() - start;
await page.evaluate(async () => {
  window.__smokeTokens = [];
  const id = window.__TAURI_INTERNALS__.transformCallback((e) =>
    window.__smokeTokens.push({
      requestId: e.payload.requestId,
      text: e.payload.text,
      time: Date.now(),
    }),
  );
  window.__smokeEvent = await window.__TAURI_INTERNALS__.invoke(
    "plugin:event|listen",
    { event: "chat-token", target: { kind: "Any" }, handler: id },
  );
});
const answerStart = Date.now();
const answer = await invoke("ai_answer", {
  requestId: "practical-smoke",
  query: inputs[0],
  ids: ["fixture-rain-room", "fixture-moon-terrace"],
  conditions,
  history: [],
  intent: "search",
});
const tokens = await page.evaluate(() =>
  window.__smokeTokens.filter((t) => t.requestId === "practical-smoke"),
);
assert(tokens.length > 1);
assert.equal(tokens.at(-1).text, answer);
assert(answer.length > 10);
record.answer = {
  text: answer,
  milliseconds: Date.now() - answerStart,
  firstTokenMs: tokens[0].time - answerStart,
  events: tokens.length,
};
record.runtime = await invoke("runtime_status");
assert(record.runtime.running);
await invoke("cancel_inference");
assert.equal((await invoke("runtime_status")).running, false);
const home = "wrld_4432ea9b-729c-46e3-8eaf-846aa0a37fdd";
const result = await invoke("add_world", {
  input: `https://vrchat.com/home/world/${home}`,
  refresh: false,
});
assert.equal(result.world.worldId, home);
assert.equal(result.world.attributes.rain.value, null);
const cached = await invoke("add_world", { input: home, refresh: false });
assert.equal(cached.cached, true);
await invoke("save_preferences", {
  value: { ...prefs, catalogMode: "personal" },
});
const personal = await invoke("load_data");
assert.equal(personal.catalog.fixture, false);
assert(personal.catalog.worlds.every((w) => w.worldId.startsWith("wrld_")));
const publicStart = Date.now();
const publicAnswer = await invoke("ai_answer", {
  requestId: "public-smoke",
  query: "雨音がある？Quest対応？軽い？",
  ids: [home],
  conditions: {
    people: null,
    platform: null,
    required: [],
    preferred: [],
    excluded: [],
  },
  history: [],
  intent: "question",
});
record.publicAnswer = {
  text: publicAnswer,
  milliseconds: Date.now() - publicStart,
};
assert(/未確認|確認でき|不明/.test(publicAnswer));
// Stop an in-flight response and confirm both managed processes disappear.
const pending = invoke("ai_answer", {
  requestId: "cancel-smoke",
  query: "詳しく教えて",
  ids: [home],
  conditions,
  history: [],
  intent: "question",
}).catch((e) => String(e));
await new Promise((r) => setTimeout(r, 250));
await invoke("cancel_inference");
await pending;
assert.equal((await invoke("runtime_status")).running, false);
// Real UI uses streams and durable SQLite conversations.
await page.reload();
await page.waitForSelector(".chat-composer");
await page.getByRole("button", { name: "新しいチャット", exact: true }).click();
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
assert(intro.length > 10);
assert(
  /VRChat Home|Rainy Daze/.test(intro),
  "Registered world must be offered for a request to list registered worlds",
);
assert((await page.locator(".messages .world-card").count()) > 0);
await new Promise((r) => setTimeout(r, 600));
const chats = await invoke("load_chats");
assert(
  chats.some((c) =>
    c.turns.some(
      (t) => t.text === "登録したワールドを紹介して" && t.reply?.answer,
    ),
  ),
);
await page.screenshot({ path: "artifacts/practical-desktop.png" });
await invoke("cancel_inference");
record.publicWorlds = personal.catalog.worlds.map((w) => ({
  id: w.worldId,
  name: w.name,
  platforms: w.platforms,
}));
record.cached = true;
record.cancelled = true;
record.persisted = true;
record.uiAnswer = intro;
writeFileSync(
  "artifacts/practical-smoke.json",
  JSON.stringify(record, null, 2),
);
console.log(
  "Practical desktop acceptance passed",
  record.answer,
  record.publicAnswer,
);
await browser.close();
