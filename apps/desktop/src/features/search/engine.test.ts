import { describe, it, expect, vi } from "vitest";
import {
  CatalogSchema,
  ConditionsSchema,
  emptyPersonal,
  WorldSchema,
} from "../../../../../packages/contracts";
import fixture from "../../../../../fixtures/catalog/worlds.json";
import {
  parseConditions,
  mergeConditions,
  groundConditions,
  rankWorlds,
  validateReasons,
  officialUrl,
} from "./engine";
const catalog = CatalogSchema.parse(fixture),
  world = catalog.worlds[0];
describe("検索・LLM境界", () => {
  it("ちるわをチルとして検索し、作者の説明を順位に使っても確認済み属性にしない", () => {
    const conditions = parseConditions("ちるわのおすすめ");
    expect(conditions.preferred).toContain("chill");
    const unknown = WorldSchema.parse({
      ...world,
      attributes: Object.fromEntries(
        Object.entries(world.attributes).map(([key, value]) => [
          key,
          { ...value, value: null },
        ]),
      ),
    });
    const rows = rankWorlds(
      [
        {
          ...unknown,
          worldId: "fixture-unrelated",
          name: "Arcade",
          description: "Games",
          tags: [],
        },
        {
          ...unknown,
          worldId: "fixture-chill",
          name: "Chill room",
          description: "Relax with friends",
          tags: [],
        },
      ],
      conditions,
      "ちるわのおすすめ",
      {},
    );
    expect(rows[0].world.worldId).toBe("fixture-chill");
    expect(rows[0].matched).toEqual([]);
  });
  it("4Bが足した必須・除外・人数・機種を検索判定に採用しない", () => {
    const q = "静かな森で一人でぼーっとしたい",
      explicit = parseConditions(q);
    const c = groundConditions(q, {
      ...explicit,
      people: 80,
      platform: "quest",
      required: ["nature", "chill"],
      excluded: ["game"],
      preferred: ["nature", "chill"],
    });
    expect(c.people).toBe(1);
    expect(c.platform).toBe(null);
    expect(c.required).toEqual([]);
    expect(c.excluded).toEqual([]);
    expect(c.preferred).toContain("nature");
    const rain = groundConditions(
      "人数は6人、雨音は絶対必要。軽さは未確認でいい",
      {
        people: null,
        platform: null,
        required: ["light"],
        excluded: ["photo"],
        preferred: [],
      },
    );
    expect(rain.people).toBe(6);
    expect(rain.required).toEqual(["rain"]);
    expect(rain.excluded).toEqual([]);
  });
  it("日本語条件を抽出し、別々の属性として保持", () => {
    expect(parseConditions("雨の夜に2人でゆっくりできる場所")).toMatchObject({
      people: 2,
      preferred: ["rain", "night", "chill"],
      platform: null,
    });
    expect(parseConditions("Questでゲームなし")).toMatchObject({
      platform: "quest",
      excluded: ["game"],
    });
  });
  it("雨音・Questがunknownの候補を適合枠に入れない", () => {
    const c = ConditionsSchema.parse({
      people: 2,
      platform: "quest",
      required: ["rain"],
      preferred: [],
      excluded: [],
    });
    const rows = rankWorlds(catalog.worlds, c, "", {});
    expect(
      rows.filter((r) => !r.unknown.length).map((r) => r.world.worldId),
    ).toEqual(["fixture-rain-room"]);
    expect(rows.some((r) => r.unknown.length)).toBe(true);
  });
  it("Questや定員から軽さを作らない", () => {
    const c = { ...parseConditions(""), required: ["light" as const] };
    expect(
      rankWorlds(catalog.worlds, c, "", {}).every((r) =>
        r.unknown.includes("実測で軽い"),
      ),
    ).toBe(true);
  });
  it("架空IDに公式リンクを捏造しない", () => {
    expect(officialUrl(world)).toBe(null);
    expect(
      officialUrl({
        ...world,
        worldId: "wrld_00000000-0000-0000-0000-000000000000",
      }),
    ).toBe(
      "https://vrchat.com/home/world/wrld_00000000-0000-0000-0000-000000000000",
    );
  });
  it("架空ID・自由文・根拠捏造・不正JSONを拒否", () => {
    const c = parseConditions("雨");
    const good = {
      items: [
        {
          worldId: world.worldId,
          evidence: [{ field: "rain", sourceId: "fixture:rain-room" }],
          unknown: ["light"],
        },
      ],
    };
    expect(validateReasons(good, [world], c)).toEqual(good);
    expect(() => validateReasons("broken", [world], c)).toThrow();
    expect(() =>
      validateReasons(
        { items: [{ ...good.items[0], worldId: "missing" }] },
        [world],
        c,
      ),
    ).toThrow();
    expect(() =>
      validateReasons(
        { items: [{ ...good.items[0], reason: "勝手な断定" }] },
        [world],
        c,
      ),
    ).toThrow();
    expect(() =>
      validateReasons(
        {
          items: [
            {
              ...good.items[0],
              evidence: [{ field: "light", sourceId: "fixture:rain-room" }],
            },
          ],
        },
        [world],
        c,
      ),
    ).toThrow();
  });
  it("説明文の命令を実行せずオフラインで検索", () => {
    const network = vi.fn(() => {
      throw Error("no network");
    });
    vi.stubGlobal("fetch", network);
    const results = rankWorlds(
      catalog.worlds,
      parseConditions("ゲーム"),
      "ゲーム",
      {},
    );
    expect(
      results.some((r) => r.world.worldId === "fixture-injection-lab"),
    ).toBe(true);
    expect(network).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  });
  it("個人データで順位・除外・削除を反映", () => {
    const p = { [world.worldId]: { ...emptyPersonal, excluded: true } };
    expect(
      rankWorlds(catalog.worlds, parseConditions(""), " ", p).some(
        (r) => r.world.worldId === world.worldId,
      ),
    ).toBe(false);
    expect(
      rankWorlds([{ ...world, deleted: true }], parseConditions(""), "", {}),
    ).toEqual([]);
  });
  it("定員不一致と結果ゼロを扱う", () => {
    expect(
      rankWorlds(
        catalog.worlds,
        { ...parseConditions(""), people: 100 },
        "",
        {},
      ).every((r) => r.unknown.includes("定員")),
    ).toBe(true);
    expect(rankWorlds([], parseConditions(""), "", {})).toEqual([]);
  });
  it("カタログの重複IDと根拠欠損を拒否", () => {
    expect(() =>
      CatalogSchema.parse({ ...catalog, worlds: [world, world] }),
    ).toThrow();
    expect(() =>
      CatalogSchema.parse({ ...catalog, worlds: [{ ...world, sources: [] }] }),
    ).toThrow();
  });
  it("AIが明示人数・機種・必須条件を改変できない", () => {
    const q = "雨の夜に2人でゆっくりできる場所",
      base = parseConditions(q);
    expect(() => mergeConditions(q, { ...base, platform: "quest" })).toThrow();
    expect(() => mergeConditions(q, { ...base, people: 20 })).toThrow();
    expect(
      mergeConditions(q, { ...base, preferred: ["night", "chill"] }),
    ).toEqual(base);
  });
  it("未登録のキーワードはゼロ件", () => {
    expect(
      rankWorlds(
        catalog.worlds,
        parseConditions("未登録の宇宙船"),
        "未登録の宇宙船",
        {},
      ),
    ).toEqual([]);
  });
});
