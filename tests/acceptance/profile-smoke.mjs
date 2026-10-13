import { chromium } from "@playwright/test";
import { writeFileSync } from "node:fs";
import assert from "node:assert/strict";
const browser = await chromium.connectOverCDP("http://127.0.0.1:9223");
const page = browser.contexts()[0].pages()[0];
const invoke = (name, args = {}) =>
  page.evaluate(
    ([name, args]) => window.__TAURI_INTERNALS__.invoke(name, args),
    [name, args],
  );
const prefs = await invoke("get_preferences");
try {
  await invoke("cancel_inference");
  await invoke("save_preferences", {
    value: {
      ...prefs,
      model: "chat-balanced",
      backend: "cpu",
      threads: 2,
      idleSeconds: 30,
    },
  });
  if (!(await invoke("setup_status")).ready) await invoke("install_assets");
  const start = Date.now();
  const plan = await invoke("ai_plan", {
    query: "雨の夜に2人でゆっくりできる場所",
    previous: null,
  });
  assert.equal(plan.conditions.people, 2);
  const runtime = await invoke("runtime_status");
  assert.equal(runtime.device, "cpu");
  const record = {
    model: "Qwen3 1.7B Q8_0",
    backend: "cpu",
    threads: 2,
    coldPlanMs: Date.now() - start,
    plan,
    runtime,
  };
  await invoke("cancel_inference");
  assert.equal((await invoke("runtime_status")).running, false);
  // Add a known public world from a VRChat official launch URL, without broad API searches.
  const rain = await invoke("add_world", {
    input:
      "https://vrchat.com/home/launch?worldId=wrld_a729bf90-1220-423a-9630-7e60851e7475",
    refresh: false,
  });
  record.addedWorld = { id: rain.world.worldId, name: rain.world.name };
  writeFileSync(
    "artifacts/profile-smoke.json",
    JSON.stringify(record, null, 2),
  );
  console.log(record);
} finally {
  await invoke("cancel_inference");
  await invoke("save_preferences", {
    value: { ...prefs, catalogMode: "personal" },
  });
  await browser.close();
}
