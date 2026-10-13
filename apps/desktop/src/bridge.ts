import { invoke, isTauri } from "@tauri-apps/api/core";
import {
  CatalogSchema,
  PersonalSchema,
  type Personal,
} from "../../../packages/contracts";
import fixture from "../../../fixtures/catalog/worlds.json";
export const native = isTauri();
let inferenceQueue: Promise<unknown> = Promise.resolve();
export function invokeInference<T = unknown>(
  command: string,
  args: Record<string, unknown> = {},
  cancelled: () => boolean = () => false,
): Promise<T> {
  const next = inferenceQueue
    .catch(() => {})
    .then(() => {
      if (cancelled()) throw Error("応答を停止しました");
      return invoke<T>(command, args);
    });
  inferenceQueue = next.catch(() => {});
  return next;
}
export async function loadData() {
  if (native)
    return await invoke<{
      catalog: unknown;
      personal: Record<string, Personal>;
    }>("load_data");
  let personal: Record<string, Personal> = {};
  try {
    const raw = JSON.parse(
      localStorage.getItem("world-compass-personal-v1") ?? "{}",
    );
    personal = Object.fromEntries(
      Object.entries(raw).map(([id, v]) => [id, PersonalSchema.parse(v)]),
    );
  } catch {
    /* Corrupted preview data is ignored. Native data remains in SQLite. */
  }
  const catalog = CatalogSchema.parse(fixture);
  return { catalog, personal };
}
export async function savePersonal(
  id: string,
  value: Personal,
  all: Record<string, Personal>,
) {
  PersonalSchema.parse(value);
  if (native) return invoke("save_personal", { id, value });
  localStorage.setItem(
    "world-compass-personal-v1",
    JSON.stringify({ ...all, [id]: value }),
  );
}
export { invoke };
