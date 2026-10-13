import { chromium } from "@playwright/test";
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
const browser = await chromium.connectOverCDP("http://127.0.0.1:9223");
const page = browser.contexts()[0].pages()[0];
await page.reload();
await page.getByLabel("VRChatユーザー名").waitFor({ timeout: 30000 });
const invoke = (name, args = {}) =>
  page.evaluate(
    ([name, args]) => window.__TAURI_INTERNALS__.invoke(name, args),
    [name, args],
  );
const status = await invoke("vrchat_auth_status");
assert.equal(status.loggedIn, false);
assert.equal(status.saved, false);
assert.equal(
  await page.getByLabel("VRChatパスワード").getAttribute("type"),
  "password",
);
assert.equal(await page.getByLabel("VRChatパスワード").inputValue(), "");
assert.equal(
  await page
    .getByRole("button", { name: "ログインしてDB構築", exact: true })
    .isDisabled(),
  true,
);
const progress = await invoke("corpus_status");
assert.equal(progress.target, 1000);
assert(
  await invoke("corpus_start").then(
    () => false,
    () => true,
  ),
);
await page.screenshot({ path: "artifacts/world-db-login.png" });
writeFileSync(
  "artifacts/corpus-ui-smoke.json",
  JSON.stringify(
    {
      version: "0.3.0",
      unauthenticatedStartBlocked: true,
      passwordInputMasked: true,
      emptyCredentialsBlocked: true,
      realLoginTested: false,
      realWorldCount: (await invoke("load_data")).catalog.worlds.length,
    },
    null,
    2,
  ),
);
await browser.close();
console.log("Corpus UI acceptance passed; live VRChat login awaits the user.");
