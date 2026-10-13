import { describe, it, expect } from "vitest";
import { needsTranslation } from "./WorldPresentation";
describe("説明文の言語判定", () => {
  it("英語・混在した外国語本文を日本語訳の対象にする", () => {
    expect(
      needsTranslation(
        "Rainy Daze is a cozy hangout world with highly customizable weather.",
      ),
    ).toBe(true);
    expect(
      needsTranslation(
        "日本語名: A cozy starting point to customize your avatar and explore menus.",
      ),
    ).toBe(true);
  });
  it("日本語本文の固有名詞や空の説明を翻訳扱いにしない", () => {
    expect(
      needsTranslation(
        "YouTubeを友達と見ながら、毛布で作った秘密基地でくつろげるワールドです。",
      ),
    ).toBe(false);
    expect(needsTranslation("")).toBe(false);
  });
});
