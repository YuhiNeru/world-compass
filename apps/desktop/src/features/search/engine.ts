import {
  ConditionsSchema,
  ReasonsSchema,
  type Conditions,
  type WorldRecord,
  type Personal,
  emptyPersonal,
  type Reasons,
} from "../../../../../packages/contracts";
export const labels = {
  rain: "雨音",
  night: "夜",
  chill: "くつろぐ",
  photo: "撮影",
  nature: "自然",
  game: "ゲーム",
  light: "実測で軽い",
};
const words: Record<keyof typeof labels, string[]> = {
  rain: ["雨", "rain"],
  night: ["夜", "星", "night"],
  chill: [
    "ゆっくり",
    "静か",
    "チル",
    "ちる",
    "くつろ",
    "落ち着",
    "chill",
    "relax",
  ],
  photo: ["写真", "撮影", "photo"],
  nature: ["自然", "森", "海", "nature"],
  game: ["ゲーム", "game"],
  light: ["軽い", "軽量", "light"],
};
export function parseConditions(query: string): Conditions {
  const q = query.toLowerCase();
  const preferred = (Object.keys(words) as (keyof typeof labels)[]).filter(
    (k) => words[k].some((w) => q.includes(w)),
  );
  const excluded = (Object.keys(words) as (keyof typeof labels)[]).filter((k) =>
    words[k].some((w) =>
      new RegExp(`${w}[^、。]{0,5}(なし|以外|除外|避け|不要)`).test(q),
    ),
  );
  const required = (Object.keys(words) as (keyof typeof labels)[]).filter(
    (k) =>
      !excluded.includes(k) &&
      words[k].some((w) => new RegExp(`${w}[^、。]{0,4}(必須|絶対)`).test(q)),
  );
  const people = q.match(/(\d{1,3})\s*人/);
  const japanese = q.match(/([一二三四五六七八九十])人/);
  const japanesePeople = japanese
    ? "一二三四五六七八九十".indexOf(japanese[1]) + 1
    : /ひとり|一人/.test(q)
      ? 1
      : /ふたり/.test(q)
        ? 2
        : null;
  return ConditionsSchema.parse({
    people: people ? Math.min(100, Math.max(1, +people[1])) : japanesePeople,
    platform: /quest|クエスト/.test(q)
      ? "quest"
      : /pc.*(必須|のみ|だけ|対応)/.test(q)
        ? "pc"
        : null,
    required,
    preferred: preferred.filter(
      (k) => !excluded.includes(k) && !required.includes(k),
    ),
    excluded,
  });
}
export function mergeConditions(query: string, raw: unknown): Conditions {
  const ai = ConditionsSchema.parse(raw),
    explicit = parseConditions(query);
  if (
    ai.people !== explicit.people ||
    ai.platform !== explicit.platform ||
    ai.required.some((k) => !explicit.required.includes(k)) ||
    ai.excluded.some((k) => !explicit.excluded.includes(k))
  )
    throw Error("入力に根拠のない必須条件");
  return {
    ...explicit,
    preferred: [...new Set([...explicit.preferred, ...ai.preferred])].filter(
      (k) => !explicit.required.includes(k) && !explicit.excluded.includes(k),
    ),
  };
}
export function groundConditions(query: string, raw: unknown): Conditions {
  const explicit = parseConditions(query),
    soft = ConditionsSchema.parse(raw);
  return {
    ...explicit,
    preferred: [...new Set([...explicit.preferred, ...soft.preferred])].filter(
      (k) => !explicit.required.includes(k) && !explicit.excluded.includes(k),
    ),
  };
}
export function matches(
  w: WorldRecord,
  c: Conditions,
): { ok: boolean; unknown: string[] } {
  const unknown: string[] = [];
  if (c.platform) {
    if (w.platforms === null) unknown.push("対応機種");
    else if (!w.platforms.includes(c.platform)) return { ok: false, unknown };
  }
  if (c.people) {
    if (w.capacity === null) unknown.push("定員");
    else if (w.capacity < c.people) return { ok: false, unknown };
  }
  for (const k of c.required) {
    const v = w.attributes[k]?.value;
    if (v === false) return { ok: false, unknown };
    if (v == null) unknown.push(labels[k]);
  }
  for (const k of c.excluded) {
    const v = w.attributes[k]?.value;
    if (v === true) return { ok: false, unknown };
    if (v == null) unknown.push(`${labels[k]}の除外`);
  }
  return { ok: true, unknown };
}
export function rankWorlds(
  worlds: WorldRecord[],
  c: Conditions,
  query: string,
  personal: Record<string, Personal>,
  vectors: Record<string, number> = {},
) {
  return worlds
    .filter((w) => !w.deleted && !personal[w.worldId]?.excluded)
    .map((w) => {
      const p = personal[w.worldId] ?? emptyPersonal;
      const m = matches(w, c);
      const text = [w.name, w.description, ...w.tags].join(" ").toLowerCase();
      const keyword = query.trim().toLowerCase();
      const matched = c.preferred.filter(
        (k) => w.attributes[k]?.value === true,
      );
      const described = c.preferred.filter((k) =>
        words[k].some((word) => text.includes(word)),
      );
      const structured =
        c.preferred.length + c.required.length + c.excluded.length > 0 ||
        c.people !== null ||
        c.platform !== null;
      const relevant =
        structured ||
        !keyword ||
        text.includes(keyword) ||
        (vectors[w.worldId] ?? 0) > 0.2;
      const score =
        matched.length * 4 +
        described.length * 2 +
        (keyword && text.includes(keyword) ? 6 : 0) +
        (p.favorite ? 2 : 0) +
        (!p.visited ? 1 : 0) +
        (vectors[w.worldId] ?? 0) * 5;
      return {
        world: w,
        personal: p,
        ...m,
        ok: m.ok && relevant,
        score,
        matched,
      };
    })
    .filter((r) => r.ok)
    .sort(
      (a, b) =>
        a.unknown.length - b.unknown.length ||
        b.score - a.score ||
        a.world.worldId.localeCompare(b.world.worldId),
    );
}
export function validateReasons(
  raw: unknown,
  candidates: WorldRecord[],
  c: Conditions,
): Reasons {
  const parsed = ReasonsSchema.parse(raw);
  const seen = new Set();
  for (const r of parsed.items) {
    const w = candidates.find((w) => w.worldId === r.worldId);
    if (
      !w ||
      seen.has(r.worldId) ||
      !matches(w, c).ok ||
      matches(w, c).unknown.length
    )
      throw Error("候補ID・必須条件が不正");
    seen.add(r.worldId);
    for (const e of r.evidence) {
      if (e.field === "capacity" || e.field === "platforms") {
        if (w[e.field] === null || e.sourceId !== null) throw Error("根拠なし");
      } else {
        const a = w.attributes[e.field];
        if (
          a?.value !== true ||
          a.sourceId !== e.sourceId ||
          !w.sources.some((s) => s.id === e.sourceId)
        )
          throw Error("根拠の参照が不正");
      }
    }
    for (const field of r.unknown) {
      if (field === "capacity" || field === "platforms") {
        if (w[field] !== null) throw Error("不明属性の誤り");
      } else if (w.attributes[field]?.value != null)
        throw Error("不明属性の誤り");
    }
  }
  return parsed;
}
export function evidenceText(w: WorldRecord, r: Reasons["items"][number]) {
  return r.evidence
    .map((e) =>
      e.field === "capacity"
        ? `カタログ定員 ${w.capacity}人`
        : e.field === "platforms"
          ? `対応機種: ${w.platforms?.join(" / ")}`
          : `${labels[e.field]}：${w.sources.find((s) => s.id === e.sourceId)?.label}`,
    )
    .join("。");
}
export function officialUrl(w: WorldRecord) {
  return /^wrld_[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(
    w.worldId,
  )
    ? `https://vrchat.com/home/world/${w.worldId}`
    : null;
}
