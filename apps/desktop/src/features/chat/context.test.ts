import { describe, expect, it } from "vitest";
import { conversationConditions } from "./context";
describe("conversation conditions", () => {
  it("retains atmosphere while changing platform and people", () => {
    const first = conversationConditions("雨の夜に2人でゆっくりできる場所");
    const quest = conversationConditions("Quest対応にして", first);
    const four = conversationConditions("4人で行きたい", quest);
    expect(four).toMatchObject({
      people: 4,
      platform: "quest",
      preferred: ["rain", "night", "chill"],
    });
  });
  it("replaces conflicting preferences and resets on explicit request", () => {
    const first = conversationConditions("自然の中でゲームをしたい");
    const next = conversationConditions("ゲームなしで探して", first);
    expect(next.preferred).not.toContain("game");
    expect(next.excluded).toContain("game");
    expect(
      conversationConditions("条件をクリアして写真を撮りたい", next).excluded,
    ).toEqual([]);
  });
});
