import { test, expect } from "@playwright/test";
import fixture from "../../fixtures/catalog/worlds.json" with { type: "json" };
test("ログイン・二段階認証・DB構築・中断と再開のUI", async ({ page }) => {
  const external: string[] = [];
  page.on("request", (r) => {
    if (!/^(127\.0\.0\.1|localhost)$/.test(new URL(r.url()).hostname))
      external.push(r.url());
  });
  await page.addInitScript(
    ({ fixture }) => {
      const w = window as any;
      localStorage.setItem("world-compass-ai", "false");
      const dataset = {
        ...fixture,
        worlds: Array.from({ length: 1000 }, (_, i) => ({
          ...fixture.worlds[0],
          worldId: `fixture-stress-${i}`,
          name: `Chill Room ${i}`,
        })),
      };
      let auth = {
        loggedIn: false,
        saved: false,
        displayName: "",
        twoFactorMethods: [] as string[],
      };
      let progress = {
        running: false,
        stage: "ready",
        completed: 0,
        indexed: 0,
        queued: 0,
        skipped: 0,
        target: 1000,
        message: "ログイン後に開始できます",
        bytes: 0,
      };
      w.isTauri = true;
      w.__TAURI_INTERNALS__ = {
        transformCallback: () => 1,
        unregisterCallback: () => {},
        invoke: async (name: string, args: any) => {
          if (name === "load_data") return { catalog: dataset, personal: {} };
          if (name === "load_chats") return [];
          if (name === "save_chats") {
            w.__savedChats = args.chats;
            return;
          }
          if (name === "search_candidates")
            return {
              ids: dataset.worlds.map((world) => world.worldId),
              weights: {},
            };
          if (name === "setup_status")
            return {
              root: "test",
              ram: 48000000000,
              free: 10000000000,
              gpu: "test",
              ready: true,
              busy: false,
              manifest: { version: "test", assets: [] },
            };
          if (name === "get_preferences")
            return {
              model: "chat-quality",
              backend: "cpu",
              threads: 2,
              idleSeconds: 30,
              apiContact: "test@example.com",
              catalogMode: "demo",
            };
          if (name === "vrchat_auth_status") return auth;
          if (name === "corpus_status") return progress;
          if (name === "corpus_expand") {
            progress = {
              ...progress,
              running: true,
              stage: "discovering",
              completed: 1200,
              indexed: 1100,
              target: 1300,
              message: "カテゴリー順で追加中",
              expanded: true,
              category: "チル・睡眠・ホーム・カフェ",
              queriesDone: 3,
              queriesTotal: 345,
              limitedQueries: 1,
            } as any;
            w.__expansionStarts = (w.__expansionStarts ?? 0) + 1;
            return;
          }
          if (name === "vrchat_login") {
            auth = {
              ...auth,
              saved: true,
              twoFactorMethods: ["totp", "emailOtp"],
            };
            return auth;
          }
          if (name === "vrchat_verify") {
            if (args.code !== "123456") throw "認証コードが一致しませんでした";
            auth = {
              loggedIn: true,
              saved: true,
              displayName: "UI Test",
              twoFactorMethods: [],
            };
            return auth;
          }
          if (name === "corpus_start") {
            progress = {
              ...progress,
              running: true,
              stage: "fetching",
              message: "情報取得中",
            };
            w.__importStarts = (w.__importStarts ?? 0) + 1;
            return;
          }
          if (name === "corpus_pause") {
            progress = {
              ...progress,
              running: false,
              stage: "paused",
              completed: 25,
              message: "中断しました",
            };
            return;
          }
          return 1;
        },
      };
    },
    { fixture },
  );
  await page.goto("/");
  const password = page.getByLabel("VRChatパスワード");
  await expect(password).toHaveAttribute("type", "password");
  await expect(
    page.getByRole("button", { name: "ログインしてDB構築", exact: true }),
  ).toBeDisabled();
  await page.getByLabel("VRChatユーザー名").fill("synthetic-ui-user");
  await password.fill("synthetic-ui-password");
  await page
    .getByRole("button", { name: "ログインしてDB構築", exact: true })
    .click();
  await expect(password).toHaveCount(0);
  await page.getByLabel("VRChat認証方法").selectOption("emailOtp");
  await page.getByLabel("VRChat認証コード").fill("000000");
  await page
    .getByRole("button", { name: "認証してDB構築", exact: true })
    .click();
  await expect(page.getByRole("alert")).toContainText(
    "認証コードが一致しませんでした",
  );
  await expect(page.getByLabel("VRChat認証コード")).toHaveValue("");
  await page.getByLabel("VRChat認証コード").fill("123456");
  await page
    .getByRole("button", { name: "認証してDB構築", exact: true })
    .click();
  await expect(page.getByText("UI Testでログイン済み")).toBeVisible();
  await expect(
    page.getByRole("button", { name: "中断して保存", exact: true }),
  ).toBeVisible();
  expect(await page.evaluate(() => (window as any).__importStarts)).toBe(1);
  await page.getByRole("button", { name: "中断して保存", exact: true }).click();
  await page
    .getByRole("button", { name: "続きからDB構築", exact: true })
    .click();
  expect(await page.evaluate(() => (window as any).__importStarts)).toBe(2);
  await page.getByRole("button", { name: "中断して保存", exact: true }).click();
  await page
    .getByRole("button", { name: "日本語ワールドの収集を拡張", exact: true })
    .click();
  expect(await page.evaluate(() => (window as any).__expansionStarts)).toBe(1);
  await expect(page.getByText(/総件数は未確定/)).toBeVisible();
  await expect(page.getByText(/検索巡回 3 \/ 345/)).toBeVisible();
  await expect(page.getByLabel("ワールドDB構築進捗")).toHaveCount(0);
  await page.getByRole("button", { name: "チャット", exact: true }).click();
  await page
    .getByRole("textbox", { name: "メッセージ" })
    .fill("ちるわのおすすめ");
  await page.getByRole("textbox", { name: "メッセージ" }).press("Enter");
  await expect(page.locator(".world-card")).toHaveCount(9);
  await page.waitForFunction(
    () => !!(window as any).__savedChats?.[0]?.turns?.[0]?.reply,
  );
  const stored = await page.evaluate(
    () => (window as any).__savedChats[0].turns[0].reply,
  );
  expect(stored.worlds.length).toBe(9);
  expect(stored.ids.length).toBe(9);
  expect(Object.keys(stored.scores).length).toBe(9);
  expect(JSON.stringify(stored).length).toBeLessThan(100000);
  expect(external).toEqual([]);
});
