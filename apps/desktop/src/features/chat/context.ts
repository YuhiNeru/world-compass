import type { Conditions } from "../../../../../packages/contracts";
import { parseConditions } from "../search/engine";

export function conversationConditions(
  text: string,
  previous?: Conditions,
  parsed = parseConditions(text),
): Conditions {
  if (!previous || /最初から|リセット|条件をクリア/.test(text)) return parsed;
  const mentioned = new Set([
    ...parsed.preferred,
    ...parsed.required,
    ...parsed.excluded,
  ]);
  return {
    people: parsed.people ?? previous.people,
    platform: /PCのみ|PCだけ|PC必須/i.test(text)
      ? "pc"
      : /機種.*(問わ|指定なし)|Quest.*不要/i.test(text)
        ? null
        : (parsed.platform ?? previous.platform),
    preferred: [
      ...previous.preferred.filter((k) => !mentioned.has(k)),
      ...parsed.preferred,
    ],
    required: [
      ...previous.required.filter((k) => !mentioned.has(k)),
      ...parsed.required,
    ],
    excluded: [
      ...previous.excluded.filter((k) => !mentioned.has(k)),
      ...parsed.excluded,
    ],
  };
}
