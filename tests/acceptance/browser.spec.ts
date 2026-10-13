import { test, expect } from "@playwright/test";
test("チャット・追加条件・会話保存・お気に入り・オフライン", async ({
  page,
  context,
}) => {
  const external: string[] = [];
  page.on("request", (r) => {
    if (!/^(127\.0\.0\.1|localhost)$/.test(new URL(r.url()).hostname))
      external.push(r.url());
  });
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "今日は、どこへ行きましょう？" }),
  ).toBeVisible();
  await expect(page.locator(".world-card")).toHaveCount(0);
  await page.screenshot({ path: "artifacts/chat-welcome.png" });
  await page
    .getByRole("textbox", { name: "メッセージ" })
    .fill("雨の夜に2人でゆっくりできる場所");
  await page.getByRole("textbox", { name: "メッセージ" }).press("Enter");
  await expect(page.locator(".chat-turn")).toHaveCount(1);
  await expect(
    page
      .locator(".chat-turn")
      .first()
      .getByRole("heading", { name: "雨待ちの小さな部屋" }),
  ).toBeVisible();
  await page.screenshot({ path: "artifacts/chat-reply.png" });
  await page
    .getByRole("button", { name: "Quest対応にして", exact: true })
    .click();
  await expect(page.locator(".chat-turn")).toHaveCount(2);
  await expect(
    page.locator(".chat-turn").last().locator(".condition-chips"),
  ).toContainText("QUEST対応");
  await expect(
    page.locator(".chat-turn").last().locator(".condition-chips"),
  ).toContainText("雨音");
  await page
    .getByRole("textbox", { name: "メッセージ" })
    .fill("雨音必須で探して");
  await page.getByRole("button", { name: "メッセージを送信" }).click();
  await expect(
    page
      .locator(".chat-turn")
      .last()
      .locator(".world-grid")
      .first()
      .locator("article"),
  ).toHaveCount(1);
  await page
    .locator(".chat-turn")
    .last()
    .getByRole("button", { name: "雨待ちの小さな部屋をお気に入り登録" })
    .click();
  await page.getByRole("button", { name: /マイライブラリ/ }).click();
  await page
    .getByPlaceholder("ここで過ごした時間をメモする")
    .fill("会話で見つけた場所");
  await page.getByPlaceholder("ここで過ごした時間をメモする").blur();
  await page.reload();
  await page.getByRole("button", { name: /マイライブラリ/ }).click();
  await expect(
    page.getByPlaceholder("ここで過ごした時間をメモする"),
  ).toHaveValue("会話で見つけた場所");
  await page.locator(".chat-history button").first().click();
  await expect(page.locator(".chat-turn")).toHaveCount(3);
  await expect(
    page.locator(".chat-turn").first().locator(".condition-chips"),
  ).not.toContainText("QUEST対応");
  await page.screenshot({ path: "artifacts/chat-conversation.png" });
  await page.getByRole("button", { name: "新しいチャット" }).click();
  await expect(
    page.getByRole("heading", { name: "今日は、どこへ行きましょう？" }),
  ).toBeVisible();
  await context.setOffline(true);
  await page
    .getByRole("textbox", { name: "メッセージ" })
    .fill("Questで雨音必須、実測で軽いことが必須");
  await page.getByRole("button", { name: "メッセージを送信" }).click();
  await expect(page.locator(".reply-intro")).toContainText(
    "確認済みの候補が見つかりませんでした",
  );
  await page.getByText(/必須条件が未確認の候補/).click();
  await expect(
    page.getByRole("heading", { name: "雨待ちの小さな部屋" }),
  ).toBeVisible();
  expect(external).toEqual([]);
  await context.setOffline(false);
  await page.getByRole("button", { name: "新しいチャット" }).click();
  await page.setViewportSize({ width: 840, height: 640 });
  await expect(
    page.getByRole("textbox", { name: "メッセージ" }),
  ).toBeInViewport();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await page.screenshot({ path: "artifacts/chat-small.png" });
});
